use crate::project_location::ProjectLocation;
use crate::{
	editor::{EditorWorkspace, ReloadPolicy},
	state::EditorState,
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
use std::{sync::Arc, time::Instant};
use tauri::{Emitter, State, WebviewWindow};

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
	let shared = Arc::clone(&state.editor);
	let viewport = Arc::clone(&state.viewport);
	tauri::async_runtime::spawn_blocking(move || {
		let started = diagnostic
			.filter(|_| log::log_enabled!(target: "editor", log::Level::Debug))
			.map(|_| Instant::now());
		let result = {
			let mut slot = shared.lock().map_err(|_| "Editor state lock failed")?;
			operation(
				slot.as_mut()
					.ok_or("Open a scene before using the editor")?,
			)
		};
		// Never hold the workspace mutex while taking the viewport mutex.
		if invalidate && result.is_ok() {
			viewport
				.lock()
				.map_err(|_| "Viewport state lock failed")?
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
	Ok(EditorCapabilitiesDto {
		incremental_commands: true,
		protocol_version: 2,
		scene_schema_version: runtime::scene_data::CURRENT_SCHEMA_VERSION,
		commands: CommandFactory::capabilities(),
		max_selection: 4096,
		max_history_entries: 128,
		history_budget_bytes: 16 * 1024 * 1024,
		embedded_viewport_supported: cfg!(target_os = "windows"),
	})
}

/// Prepare a candidate off the WebView thread, then install it through the
/// same revision, completion and viewport path for scene and project opening.
async fn open_workspace(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	prepare: impl FnOnce(Box<dyn Fn() + Send>) -> Result<EditorWorkspace, String> + Send + 'static,
) -> Result<EditorSnapshotDto, String> {
	trusted_window(&window)?;
	let shared = Arc::clone(&state.editor);
	let viewport = Arc::clone(&state.viewport);
	tauri::async_runtime::spawn_blocking(move || {
		let notify_editor = Arc::clone(&shared);
		let notify_viewport = Arc::clone(&viewport);
		let notify_window = window.clone();
		let mut candidate = prepare(Box::new(move || {
			let editor = Arc::clone(&notify_editor);
			let viewport = Arc::clone(&notify_viewport);
			let window = notify_window.clone();
			tauri::async_runtime::spawn_blocking(move || {
				let completions = editor.lock().ok().and_then(|mut slot| slot.as_mut().map(EditorWorkspace::drain_asset_jobs)).unwrap_or_default();
				if !completions.is_empty() {
					let _ = window.emit("editor://asset-completions", &completions);
					if let Ok(viewport) = viewport.lock() { viewport.invalidate(); }
				}
			});
		}))?;
		let snapshot = {
			let mut current = shared.lock().map_err(|_| "Editor state lock failed")?;
			if let Some(old) = current.as_ref() {
				candidate
					.backend
					.supersede_revision(old.backend.session().revision()).map_err(|e| e.to_string())?;
			}
			let ready = candidate.drain_asset_jobs();
			if !ready.is_empty() { let _ = window.emit("editor://asset-completions", ready); }
			let snapshot = candidate.backend.snapshot().map_err(|e| e.to_string())?;
			*current = Some(candidate);
			snapshot
		};
		viewport
			.lock()
			.map_err(|_| "Viewport state lock failed")?
			.invalidate();
		Ok(snapshot)
	})
		.await
		.map_err(|e| {
			if matches!(&e, tauri::Error::JoinError(join) if join.is_panic()) {
				engine_logger::debug!(target: "editor", "Editor open worker panicked; panic hook owns the error record");
			} else {
				engine_logger::error!(target: "editor", "Editor open worker cancelled");
			}
			format!("Editor worker failed: {e}")
		})?
}

#[tauri::command]
pub async fn editor_open(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	asset_root: String,
	scene_path: String,
) -> Result<EditorSnapshotDto, String> {
	open_workspace(window, state, move |on_ready| {
		EditorWorkspace::open_with_notifier(&asset_root, &scene_path, on_ready)
	})
	.await
}

#[tauri::command]
pub async fn editor_open_project(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	manifest_path: String,
) -> Result<EditorSnapshotDto, String> {
	open_workspace(window, state, move |on_ready| {
		let location = ProjectLocation::from_manifest(std::path::Path::new(&manifest_path))
			.map_err(|error| error.to_string())?;
		EditorWorkspace::open_project_with_notifier(location, on_ready)
	})
	.await
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
pub async fn editor_save_project(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	expected_revision: u64,
) -> Result<EditorSnapshotDto, String> {
	trusted_window(&window)?;
	workspace(state, false, Some("save_project"), move |w| {
		w.save_project(expected_revision)
	})
	.await
}
#[derive(Serialize)]
pub struct ProjectBuildDto {
	path: String,
	asset_count: usize,
	asset_bytes: u64,
}
#[tauri::command]
pub async fn editor_build_project(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	expected_revision: u64,
	destination: String,
) -> Result<ProjectBuildDto, String> {
	trusted_window(&window)?;
	let prepared = workspace(state, false, Some("prepare_build"), move |w| {
		w.prepare_build(expected_revision)
	})
	.await?;
	tauri::async_runtime::spawn_blocking(move || {
		let asset_count = prepared.asset_count();
		let asset_bytes = prepared.total_bytes();
		engine_logger::info!(target:"project_build","Build started: assets={asset_count}, bytes={asset_bytes}");
		let path = compiler::install_prepared_project(&prepared, &destination).inspect_err(|_| { engine_logger::error!(target:"project_build","Build failed: operation=install_authored_project"); })?;
		engine_logger::info!(target:"project_build","Build completed: assets={asset_count}, bytes={asset_bytes}");
		Ok(ProjectBuildDto {
			path: path.to_string_lossy().into_owned(),
			asset_count,
			asset_bytes,
		})
	})
		.await
		.map_err(|e| format!("Project build worker failed: {e}"))?
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
#[tauri::command]
pub async fn editor_save(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	expected_revision: u64,
) -> Result<EditorSnapshotDto, String> {
	trusted_window(&window)?;
	workspace(state, false, Some("save"), move |w| {
		w.save(expected_revision)
	})
	.await
}
#[tauri::command]
pub async fn editor_reload(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	expected_revision: u64,
	policy: Option<ReloadPolicy>,
) -> Result<EditorSnapshotDto, String> {
	trusted_window(&window)?;
	workspace(state, true, Some("reload"), move |w| {
		w.reload_with_policy(expected_revision, policy.unwrap_or_default())
	})
	.await
}

#[tauri::command]
pub async fn editor_register_scene(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	path: String,
	expected_revision: u64,
) -> Result<EditorSnapshotDto, String> {
	trusted_window(&window)?;
	workspace(state, true, Some("register_scene"), move |w| {
		w.register_scene(&path, expected_revision)
	})
	.await
}
#[tauri::command]
pub async fn editor_save_scene_as(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	path: String,
	expected_revision: u64,
) -> Result<EditorSnapshotDto, String> {
	trusted_window(&window)?;
	workspace(state, true, Some("save_scene_as"), move |w| {
		w.save_scene_as(&path, expected_revision)
	})
	.await
}

#[derive(Serialize)]
pub struct ModelAssetDto {
	name: String,
	path: String,
}
#[tauri::command]
pub async fn editor_assets(
	window: WebviewWindow,
	state: State<'_, EditorState>,
) -> Result<Vec<ModelAssetDto>, String> {
	trusted_window(&window)?;
	workspace(state, false, None, |w| {
		runtime::assets::catalog::scan_models(&w.asset_root).map(|assets| {
			assets
				.into_iter()
				.map(|a| ModelAssetDto {
					name: a.name,
					path: a.path,
				})
				.collect()
		})
	})
	.await
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
#[derive(Serialize)]
pub struct CollisionSnapshotDto {
	contacts: Vec<Contact>,
	shapes: Vec<DebugShape>,
}
#[tauri::command]
pub async fn editor_collisions(
	window: WebviewWindow,
	state: State<'_, EditorState>,
) -> Result<CollisionSnapshotDto, String> {
	trusted_window(&window)?;
	workspace(state, false, None, |w| {
		let document = w.backend.session().render_document();
		Ok(CollisionSnapshotDto {
			contacts: runtime::collision::query_overlaps(&document)?,
			shapes: runtime::collision::debug_shapes(&document)?,
		})
	})
	.await
}

#[tauri::command]
pub fn editor_viewport_attach(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	bounds: ViewportBoundsDto,
) -> Result<ViewportStatusDto, String> {
	trusted_window(&window)?;
	state
		.viewport
		.lock()
		.map_err(|_| "Viewport state lock failed")?
		.attach(&window, bounds)
}
#[tauri::command]
pub fn editor_viewport_bounds(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	bounds: ViewportBoundsDto,
) -> Result<ViewportStatusDto, String> {
	trusted_window(&window)?;
	state
		.viewport
		.lock()
		.map_err(|_| "Viewport state lock failed")?
		.bounds(bounds)
}
#[tauri::command]
pub fn editor_viewport_detach(
	window: WebviewWindow,
	state: State<'_, EditorState>,
) -> Result<ViewportStatusDto, String> {
	trusted_window(&window)?;
	let mut viewport = state
		.viewport
		.lock()
		.map_err(|_| "Viewport state lock failed")?;
	viewport.detach();
	Ok(viewport.status())
}
#[tauri::command]
pub fn editor_viewport_status(
	window: WebviewWindow,
	state: State<'_, EditorState>,
) -> Result<ViewportStatusDto, String> {
	trusted_window(&window)?;
	Ok(state
		.viewport
		.lock()
		.map_err(|_| "Viewport state lock failed")?
		.status())
}
