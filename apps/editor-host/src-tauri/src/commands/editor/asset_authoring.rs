//! External file work is prepared away from the workspace lock. Only a native
//! resource owner may publish an active asset after candidate GPU preparation.
use super::*;
use runtime::asset_catalog::AssetId;
use runtime::assets::project_assets::{self, PreparedAssetChange};
use std::{
	collections::HashSet,
	path::{Path, PathBuf},
};
use tauri_plugin_dialog::DialogExt;

#[derive(Serialize)]
pub struct AssetOperationDto {
	operation_id: String,
	asset: ModelAssetDto,
	snapshot: EditorSnapshotDto,
}
#[derive(Clone, Serialize)]
struct Progress {
	operation_id: String,
	stage: &'static str,
	completed: u64,
	total: u64,
	error: Option<String>,
}
fn progress(
	window: &WebviewWindow,
	id: &str,
	stage: &'static str,
	completed: u64,
	error: Option<String>,
) {
	let _ = window.emit(
		"editor://asset-operation",
		Progress {
			operation_id: id.into(),
			stage,
			completed,
			total: 3,
			error,
		},
	);
}

#[tauri::command]
pub async fn editor_choose_external_glb(window: WebviewWindow) -> Result<Option<String>, String> {
	trusted_window(&window)?;
	tauri::async_runtime::spawn_blocking(move || {
		window
			.dialog()
			.file()
			.set_parent(&window)
			.set_title("Import binary glTF model")
			.add_filter("Binary glTF", &["glb"])
			.blocking_pick_file()
			.map(|file| {
				file.into_path()
					.map(|path| path.to_string_lossy().into_owned())
					.map_err(|e| e.to_string())
			})
			.transpose()
	})
	.await
	.map_err(|e| e.to_string())?
}
enum Change {
	Import {
		source: PathBuf,
		destination: String,
	},
	Reimport {
		id: AssetId,
		source: Option<PathBuf>,
		fingerprint: String,
	},
	Move {
		id: AssetId,
		destination: String,
	},
}
static ACTIVE_OPERATIONS: AtomicU64 = AtomicU64::new(0);
struct Admission;
impl Admission {
	fn acquire() -> Result<Self, String> {
		ACTIVE_OPERATIONS
			.fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
				(count < 4).then_some(count + 1)
			})
			.map(|_| Self)
			.map_err(|_| {
				"Asset authoring queue is full; wait for an active operation to finish".into()
			})
	}
}
impl Drop for Admission {
	fn drop(&mut self) {
		ACTIVE_OPERATIONS.fetch_sub(1, Ordering::AcqRel);
	}
}
async fn operate(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	expected_revision: u64,
	change: Change,
) -> Result<AssetOperationDto, String> {
	trusted_window(&window)?;
	let admission = Admission::acquire()?;
	let shared = Arc::clone(&state.editor);
	let viewport = Arc::clone(&state.viewport);
	tauri::async_runtime::spawn_blocking(move || {
		let _admission = admission;
		let operation_id = AssetId::generate().as_str().to_owned();
		progress(&window, &operation_id, "validating", 0, None);
		let result: Result<AssetOperationDto, String> = (|| {
			let (root, generation, used) = {
				let slot = shared.lock().map_err(|_| "Editor state lock failed")?;
				let w = slot
					.as_ref()
					.ok_or("Open a project before importing assets")?;
				w.check_revision(expected_revision)?;
				w.backend
					.validate_asset_invalidation()
					.map_err(|e| e.to_string())?;
				if let Change::Import { destination, .. } | Change::Move { destination, .. } =
					&change
				{
					w.validate_asset_destination(destination)?;
				}
				let used = match &change {
					Change::Reimport { id, .. } => w.used_asset_meshes(id)?,
					_ => HashSet::new(),
				};
				(w.asset_root.clone(), w.generation, used)
			};
			let prepared: PreparedAssetChange = match change {
				Change::Import {
					source,
					destination,
				} => {
					if !source.is_absolute() {
						return Err("Choose an absolute GLB source file".into());
					}
					project_assets::prepare_import(&root, &source, &destination)?
				}
				Change::Reimport {
					id,
					source,
					fingerprint,
				} => {
					if source.as_ref().is_some_and(|path| !path.is_absolute()) {
						return Err("Choose an absolute GLB source file".into());
					}
					project_assets::prepare_reimport(
						&root,
						&id,
						source.as_deref(),
						&fingerprint,
						&used,
					)?
				}
				Change::Move { id, destination } => {
					project_assets::prepare_move(&root, &id, &destination)?
				}
			};
			progress(&window, &operation_id, "publishing", 1, None);
			// Never take this mutex while holding the workspace mutex: the native worker
			// obtains the workspace guard for its final revision/publication transaction.
			let (asset, snapshot) = viewport
				.lock()
				.map_err(|_| "Viewport state lock failed")?
				.request_asset_change(prepared, generation, expected_revision)?;
			progress(&window, &operation_id, "complete", 3, None);
			Ok(AssetOperationDto {
				operation_id: operation_id.clone(),
				asset: ModelAssetDto::from_asset(&asset),
				snapshot,
			})
		})();
		if let Err(error) = &result {
			progress(&window, &operation_id, "failed", 0, Some(error.clone()));
		}
		result
	})
	.await
	.map_err(|e| format!("Asset authoring worker failed: {e}"))?
}
#[tauri::command]
pub async fn editor_import_external_glb(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	source_path: String,
	destination_path: String,
	expected_revision: u64,
) -> Result<AssetOperationDto, String> {
	operate(
		window,
		state,
		expected_revision,
		Change::Import {
			source: Path::new(&source_path).into(),
			destination: destination_path,
		},
	)
	.await
}
#[tauri::command]
pub async fn editor_reimport_asset(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	asset_id: String,
	source_path: Option<String>,
	expected_fingerprint: String,
	expected_revision: u64,
) -> Result<AssetOperationDto, String> {
	operate(
		window,
		state,
		expected_revision,
		Change::Reimport {
			id: AssetId::new(asset_id)?,
			source: source_path.map(PathBuf::from),
			fingerprint: expected_fingerprint,
		},
	)
	.await
}
#[tauri::command]
pub async fn editor_move_asset(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	asset_id: String,
	destination_path: String,
	expected_revision: u64,
) -> Result<AssetOperationDto, String> {
	operate(
		window,
		state,
		expected_revision,
		Change::Move {
			id: AssetId::new(asset_id)?,
			destination: destination_path,
		},
	)
	.await
}

#[cfg(test)]
mod contract_tests {
	use super::*;
	#[test]
	fn operation_result_and_progress_use_the_frontend_wire_field_names() {
		let scene = runtime::SceneData::from_json(include_str!(concat!(
			env!("CARGO_MANIFEST_DIR"),
			"/../../game/assets/scenes/robots.json"
		)))
		.unwrap();
		let project =
			runtime::project::ProjectData::for_scene("Asset DTO", "scenes/main.json", &scene)
				.unwrap();
		let snapshot = runtime::Engine::new(scene, project)
			.unwrap()
			.snapshot()
			.unwrap();
		let revision = snapshot.revision;
		let asset = runtime::asset_catalog::PersistentAsset {
			id: AssetId::generate(),
			path: "assembly.json".into(),
			kind: runtime::asset_catalog::AssetKind::Prefab,
			aliases: vec![],
			revision: 1,
			fingerprint: "a".repeat(64),
			bytes: 128,
			source_path: None,
			meshes: vec![],
			dependencies: vec![],
		};
		let value = serde_json::to_value(AssetOperationDto {
			operation_id: "operation-1".into(),
			asset: ModelAssetDto::from_asset(&asset),
			snapshot,
		})
		.unwrap();
		assert_eq!(
			value
				.as_object()
				.unwrap()
				.keys()
				.map(String::as_str)
				.collect::<std::collections::BTreeSet<_>>(),
			std::collections::BTreeSet::from(["asset", "operation_id", "snapshot"])
		);
		assert_eq!(value["operation_id"], "operation-1");
		assert!(value.get("operationId").is_none());
		assert_eq!(value["snapshot"]["revision"], revision);
		assert_eq!(value["asset"]["asset_id"], asset.id.as_str());
		assert_eq!(value["asset"]["kind"], "prefab");
		assert!(value["asset"]["mesh_count"].is_null());
		let progress = serde_json::to_value(Progress {
			operation_id: "operation-1".into(),
			stage: "validating",
			completed: 0,
			total: 3,
			error: None,
		})
		.unwrap();
		assert_eq!(
			progress,
			serde_json::json!({"operation_id":"operation-1","stage":"validating","completed":0,"total":3,"error":null})
		);
	}
}
