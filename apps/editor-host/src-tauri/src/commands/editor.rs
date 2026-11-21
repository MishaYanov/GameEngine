use crate::project_location::ProjectLocation;
use crate::{
	editor::{EditorWorkspace, ReloadPolicy},
	state::{EditorState, PendingProjectBuild, ProjectBuildState},
};
use bridge::editor::{
	EditorCapabilitiesDto, EditorCommandRequest, EditorSnapshotDto, ViewportBoundsDto,
	ViewportStatusDto,
};
use runtime::{
	collision::{Contact, DebugShape},
	editor::{CommandFactory, PrefabData},
};
use serde::Serialize;
use std::{
	path::Path,
	process::Command,
	sync::{
		atomic::{AtomicU64, Ordering},
		Arc, Mutex,
	},
	time::Instant,
};
use tauri::{Emitter, State, WebviewWindow};
mod asset_authoring;
pub use asset_authoring::*;
mod linked_prefabs;
pub use linked_prefabs::*;
mod animation;
pub use animation::*;
mod camera;
pub use camera::*;
mod build;
pub use build::*;
mod documents;
pub use documents::*;
mod viewport;
pub use viewport::*;

pub(super) fn trusted_window(window: &WebviewWindow) -> Result<(), String> {
	let url = window.url().map_err(|e| e.to_string())?;
	let packaged = (url.scheme() == "tauri" && url.host_str() == Some("localhost"))
		|| (matches!(url.scheme(), "http" | "https") && url.host_str() == Some("tauri.localhost"));
	let development = cfg!(debug_assertions)
		&& url.scheme() == "http"
		&& url.host_str() == Some("localhost")
		&& url.port() == Some(4200);
	if window.label() != "main" || !(packaged || development) {
		return Err("Editor commands require the trusted main application window".into());
	}
	Ok(())
}

async fn workspace<T: Send + 'static>(
	state: State<'_, EditorState>,
	invalidate: bool,
	diagnostic: Option<&'static str>,
	operation: impl FnOnce(&mut EditorWorkspace) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
	workspace_with_error(state, invalidate, diagnostic, operation).await
}

async fn workspace_with_error<T: Send + 'static, E: From<String> + Send + 'static>(
	state: State<'_, EditorState>,
	invalidate: bool,
	diagnostic: Option<&'static str>,
	operation: impl FnOnce(&mut EditorWorkspace) -> Result<T, E> + Send + 'static,
) -> Result<T, E> {
	let shared = Arc::clone(&state.editor);
	let viewport = Arc::clone(&state.viewport);
	tauri::async_runtime::spawn_blocking(move || {
		let started = diagnostic
			.filter(|_| log::log_enabled!(target: "editor", log::Level::Debug))
			.map(|_| Instant::now());
		let (result, preview_closed) = {
			let mut slot = shared.lock().map_err(|_| "Editor state lock failed".to_owned())?;
			let workspace = slot.as_mut().ok_or_else(|| "Open a scene before using the editor".to_owned())?;
			workspace.with_camera_preview_cleanup(operation)
		};
		// Never hold the workspace mutex while taking the viewport mutex.
		if preview_closed || (invalidate && result.is_ok()) {
			viewport
				.lock()
				.map_err(|_| "Viewport state lock failed".to_owned())?
				.invalidate();
		}
		if let (Some(operation), Some(started)) = (diagnostic, started) {
			engine_logger::debug!(target: "editor", "Operation {operation}: accepted={}, elapsed_us={}", result.is_ok(), started.elapsed().as_micros());
		}
		result
	})
		.await
		.map_err(|e| {
			if matches!(&e, tauri::Error::JoinError(join) if join.is_panic()) {
				engine_logger::debug!(target: "editor", "Editor workspace worker panicked; panic hook owns the error record");
			} else {
				engine_logger::error!(target: "editor", "Editor workspace worker cancelled");
			}
			format!("Editor worker failed: {e}")
		})?
}

#[tauri::command]
pub fn editor_capabilities(window: WebviewWindow) -> Result<EditorCapabilitiesDto, String> {
	trusted_window(&window)?;
	Ok(capabilities())
}

fn capabilities() -> EditorCapabilitiesDto {
	EditorCapabilitiesDto {
		state_machines: Some(runtime::state_machine::descriptor()),
		component_types: runtime::components::descriptors(),
		incremental_commands: true,
		protocol_version: 2,
		scene_schema_version: runtime::scene_data::CURRENT_SCHEMA_VERSION,
		commands: CommandFactory::capabilities(),
		gameplay_modules: vec![courtyard_gameplay::descriptor()],
		max_selection: 4096,
		max_history_entries: 128,
		history_budget_bytes: 16 * 1024 * 1024,
		embedded_viewport_supported: cfg!(target_os = "windows"),
	}
}

#[tauri::command]
pub async fn editor_snapshot(
	window: WebviewWindow,
	state: State<'_, EditorState>,
) -> Result<EditorSnapshotDto, String> {
	trusted_window(&window)?;
	workspace(state, false, None, |w| {
		w.backend.snapshot().map_err(|e| e.to_string())
	})
	.await
}
#[tauri::command]
pub async fn editor_execute(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	request: EditorCommandRequest,
) -> Result<EditorSnapshotDto, String> {
	trusted_window(&window)?;
	workspace(state, true, Some("execute"), move |w| w.execute(request)).await
}
fn backend_boundary(message: String) -> runtime::BackendError {
	runtime::BackendError {
		code: runtime::BackendErrorCode::InvalidCommand,
		message,
		current_revision: None,
	}
}
/// Atomically execute and capture only the changed UI data. A separate query
/// could observe another edit or workspace between receipt and synchronization.
#[tauri::command]
pub async fn editor_execute_incremental(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	request: EditorCommandRequest,
) -> Result<bridge::editor::EditorCommandUpdateDto, runtime::BackendError> {
	trusted_window(&window).map_err(backend_boundary)?;
	let update = workspace(state, true, Some("execute_incremental"), move |w| {
		Ok(w.execute_incremental(request))
	})
	.await
	.map_err(backend_boundary)??;
	let cursor = match &update {
		bridge::editor::EditorCommandUpdateDto::Snapshot { receipt, .. }
		| bridge::editor::EditorCommandUpdateDto::Delta { receipt, .. } => receipt.change_cursor,
	};
	let _ = window.emit("editor://changes-ready", cursor);
	Ok(update)
}
/// Small typed response for frequent commands. Full snapshots remain an
/// explicit query, so pointer motion and play input do not serialize the scene.
#[tauri::command]
pub async fn editor_execute_receipt(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	request: EditorCommandRequest,
) -> Result<runtime::CommandReceipt, runtime::BackendError> {
	trusted_window(&window).map_err(backend_boundary)?;
	let receipt = workspace(state, true, Some("execute_receipt"), move |w| {
		Ok(w.execute_receipt(request))
	})
	.await
	.map_err(backend_boundary)??;
	let _ = window.emit("editor://changes-ready", receipt.change_cursor);
	Ok(receipt)
}
#[tauri::command]
pub async fn editor_changes(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	cursor: u64,
) -> Result<runtime::events::ChangePage, runtime::BackendError> {
	trusted_window(&window).map_err(backend_boundary)?;
	workspace(state, false, None, move |w| {
		Ok(w.backend.changes_since(cursor))
	})
	.await
	.map_err(backend_boundary)?
}
#[tauri::command]
pub async fn editor_submit_import(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	path: String,
	position: [f32; 3],
	expected_revision: u64,
	reserved_bytes: usize,
) -> Result<crate::editor::AssetTicket, String> {
	trusted_window(&window)?;
	workspace(state, false, Some("submit_import"), move |w| {
		w.submit_import(path, position, expected_revision, reserved_bytes)
	})
	.await
}
#[tauri::command]
pub async fn editor_cancel_import(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	job_id: runtime::asset_jobs::AssetJobId,
	workspace_generation: u64,
) -> Result<bool, String> {
	trusted_window(&window)?;
	workspace(state, false, None, move |w| {
		w.cancel_import(job_id, workspace_generation)
	})
	.await
}
#[tauri::command]
pub async fn editor_drain_imports(
	window: WebviewWindow,
	state: State<'_, EditorState>,
) -> Result<Vec<crate::editor::AssetCompletion>, String> {
	trusted_window(&window)?;
	workspace(state, true, None, move |w| {
		w.drain_asset_jobs();
		Ok(w.recent_completions())
	})
	.await
}
#[derive(Serialize)]
pub struct ModelAssetDto {
	name: String,
	path: String,
	kind: &'static str,
	asset_id: Option<runtime::asset_catalog::AssetId>,
	fingerprint: Option<String>,
	revision: Option<u64>,
	source_path: Option<String>,
	mesh_count: Option<usize>,
}
impl ModelAssetDto {
	fn from_asset(asset: &runtime::asset_catalog::PersistentAsset) -> Self {
		Self {
			name: Path::new(&asset.path)
				.file_stem()
				.and_then(|name| name.to_str())
				.unwrap_or("Asset")
				.into(),
			path: asset.path.clone(),
			kind: if asset.kind == runtime::asset_catalog::AssetKind::Prefab {
				"prefab"
			} else {
				"model"
			},
			asset_id: Some(asset.id.clone()),
			fingerprint: Some(asset.fingerprint.clone()),
			revision: Some(asset.revision),
			source_path: asset.source_path.clone(),
			mesh_count: (asset.kind == runtime::asset_catalog::AssetKind::Model)
				.then_some(asset.meshes.len()),
		}
	}
}
#[cfg(test)]
#[path = "../../tests/unit/commands/editor/asset_dto_contract.rs"]
mod asset_dto_contract;
#[tauri::command]
pub async fn editor_assets(
	window: WebviewWindow,
	state: State<'_, EditorState>,
) -> Result<Vec<ModelAssetDto>, String> {
	trusted_window(&window)?;
	let shared = Arc::clone(&state.editor);
	tauri::async_runtime::spawn_blocking(move || {
		let slot = shared.lock().map_err(|_| "Editor state lock failed")?;
		let workspace = slot
			.as_ref()
			.ok_or("Open a scene before using the editor")?;
		let catalog = runtime::assets::project_assets::migrate_catalog(&workspace.asset_root)?;
		let mut assets: Vec<_> = catalog
			.assets
			.iter()
			.filter(|asset| {
				matches!(
					asset.kind,
					runtime::asset_catalog::AssetKind::Model
						| runtime::asset_catalog::AssetKind::Prefab
				)
			})
			.map(ModelAssetDto::from_asset)
			.collect();
		assets.sort_by(|a, b| a.path.cmp(&b.path));
		Ok(assets)
	})
	.await
	.map_err(|error| format!("Asset catalog worker failed: {error}"))?
}
#[tauri::command]
pub async fn editor_import_model(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	path: String,
	position: [f32; 3],
	expected_revision: u64,
) -> Result<EditorSnapshotDto, String> {
	trusted_window(&window)?;
	workspace(state, true, Some("import_model"), move |w| {
		w.import_model(&path, position, expected_revision)
	})
	.await
}
#[tauri::command]
pub async fn editor_export_prefab(
	window: WebviewWindow,
	state: State<'_, EditorState>,
) -> Result<PrefabData, String> {
	trusted_window(&window)?;
	workspace(state, false, Some("export_prefab"), |w| {
		w.backend.session().export_prefab()
	})
	.await
}
#[tauri::command]
pub async fn editor_instantiate_prefab(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	prefab: PrefabData,
	position: [f32; 3],
	expected_revision: u64,
) -> Result<EditorSnapshotDto, String> {
	trusted_window(&window)?;
	workspace(state, true, Some("instantiate_prefab"), move |w| {
		w.instantiate_prefab(prefab, position, expected_revision)
	})
	.await
}
#[derive(Serialize)]
pub struct PrefabPathDto {
	path: String,
}
#[tauri::command]
pub async fn editor_save_prefab(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	path: String,
	expected_revision: u64,
) -> Result<PrefabPathDto, String> {
	trusted_window(&window)?;
	workspace(state, false, Some("save_prefab"), move |w| {
		w.save_prefab(&path, expected_revision)
			.map(|path| PrefabPathDto { path })
	})
	.await
}
#[tauri::command]
pub async fn editor_load_prefab(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	path: String,
	position: [f32; 3],
	expected_revision: u64,
) -> Result<EditorSnapshotDto, String> {
	trusted_window(&window)?;
	workspace(state, true, Some("load_prefab"), move |w| {
		w.load_prefab(&path, position, expected_revision)
	})
	.await
}
#[cfg(test)]
#[path = "../../tests/unit/commands/tests_gameplay_metadata.rs"]
mod tests_gameplay_metadata;
