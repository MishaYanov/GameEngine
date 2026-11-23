//! External file work is prepared away from the workspace lock. Only a native
//! resource owner may publish an active asset after candidate GPU preparation.
use super::*;
use crate::editor::asset_sources::AssetSourceKind;
use runtime::asset_catalog::AssetId;
use runtime::assets::project_assets::{self, PreparedAssetChange};
use std::{collections::HashSet, fs, path::PathBuf};
use tauri_plugin_dialog::DialogExt;

#[derive(Serialize)]
pub struct AssetOperationDto {
	operation_id: String,
	asset: ModelAssetDto,
	snapshot: EditorSnapshotDto,
}

#[derive(Serialize)]
pub struct AudioClipDetailsDto {
	#[serde(flatten)]
	info: runtime::audio_clip::AudioClipInfo,
	references: Vec<AudioClipReferenceDto>,
}

#[derive(Serialize)]
struct AudioClipReferenceDto {
	asset_id: String,
	path: String,
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
pub async fn editor_choose_external_glb(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	expected_generation: u64,
) -> Result<Option<String>, String> {
	choose_external_file(window, state, expected_generation, AssetSourceKind::Glb).await
}

#[tauri::command]
pub async fn editor_choose_external_wav(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	expected_generation: u64,
) -> Result<Option<String>, String> {
	choose_external_file(window, state, expected_generation, AssetSourceKind::Wav).await
}

#[tauri::command]
pub async fn editor_choose_external_sky_png(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	expected_generation: u64,
) -> Result<Option<String>, String> {
	choose_external_file(window, state, expected_generation, AssetSourceKind::SkyPng).await
}

async fn choose_external_file(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	expected_generation: u64,
	kind: AssetSourceKind,
) -> Result<Option<String>, String> {
	trusted_window(&window)?;
	let shared = Arc::clone(&state.editor);
	tauri::async_runtime::spawn_blocking(move || {
		let check_scope = |slot: &Option<EditorWorkspace>| {
			let workspace = slot
				.as_ref()
				.ok_or("Open a project before choosing an asset source")?;
			check_asset_generation(workspace, expected_generation)
		};
		{
			let slot = shared.lock().map_err(|_| "Editor state lock failed")?;
			check_scope(&slot)?;
		}
		let (title, label, extension) = match kind {
			AssetSourceKind::Glb => ("Import binary glTF model", "Binary glTF", "glb"),
			AssetSourceKind::Wav => ("Import Audio Clip", "PCM WAV", "wav"),
			AssetSourceKind::SkyPng => ("Import Sky Panorama", "PNG image", "png"),
		};
		let Some(file) = window
			.dialog()
			.file()
			.set_parent(&window)
			.set_title(title)
			.add_filter(label, &[extension])
			.blocking_pick_file()
		else {
			return Ok(None);
		};
		let selected = file.into_path().map_err(|error| error.to_string())?;
		let metadata = fs::symlink_metadata(&selected).map_err(|error| error.to_string())?;
		if !metadata.is_file() || metadata.file_type().is_symlink() {
			return Err("Choose a regular asset source file".into());
		}
		let source = fs::canonicalize(&selected)
			.map_err(|error| format!("Could not open the selected asset source: {error}"))?;
		let display = source
			.to_str()
			.ok_or("The source file path is not valid Unicode")?
			.to_owned();
		trusted_window(&window)?;
		let mut slot = shared.lock().map_err(|_| "Editor state lock failed")?;
		let workspace = slot
			.as_mut()
			.ok_or("Open a project before choosing an asset source")?;
		check_asset_generation(workspace, expected_generation)?;
		workspace.asset_sources.select(kind, source);
		Ok(Some(display))
	})
	.await
	.map_err(|error| error.to_string())?
}

fn check_asset_generation(
	workspace: &EditorWorkspace,
	expected_generation: u64,
) -> Result<(), String> {
	if workspace.generation != expected_generation {
		return Err("The asset operation belongs to another workspace".into());
	}
	Ok(())
}

enum Change {
	Import {
		source: PathBuf,
		destination: String,
	},
	ImportAudioClip {
		source: PathBuf,
		destination: String,
	},
	ImportSkyPanorama {
		source: PathBuf,
		destination: String,
	},
	Reimport {
		id: AssetId,
		source: PathBuf,
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
	expected_generation: u64,
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
				check_asset_generation(w, expected_generation)?;
				w.check_revision(expected_revision)?;
				let selected = match &change {
					Change::Import { source, .. } | Change::Reimport { source, .. } => {
						Some((AssetSourceKind::Glb, source))
					}
					Change::ImportAudioClip { source, .. } => Some((AssetSourceKind::Wav, source)),
					Change::ImportSkyPanorama { source, .. } => {
						Some((AssetSourceKind::SkyPng, source))
					}
					Change::Move { .. } => None,
				};
				if let Some((kind, source)) = selected {
					w.asset_sources.require(kind, source)?;
				}
				w.backend
					.validate_asset_invalidation()
					.map_err(|e| e.to_string())?;
				if let Change::Import { destination, .. }
				| Change::ImportAudioClip { destination, .. }
				| Change::ImportSkyPanorama { destination, .. }
				| Change::Move { destination, .. } = &change
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
				} => project_assets::prepare_import(&root, &source, &destination)?,
				Change::ImportAudioClip {
					source,
					destination,
				} => project_assets::prepare_audio_clip_import(&root, &source, &destination)?,
				Change::ImportSkyPanorama {
					source,
					destination,
				} => project_assets::prepare_sky_panorama_import(&root, &source, &destination)?,
				Change::Reimport {
					id,
					source,
					fingerprint,
				} => project_assets::prepare_reimport(
					&root,
					&id,
					Some(&source),
					&fingerprint,
					&used,
				)?,
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
	expected_generation: u64,
) -> Result<AssetOperationDto, String> {
	operate(
		window,
		state,
		expected_revision,
		expected_generation,
		Change::Import {
			source: PathBuf::from(source_path),
			destination: destination_path,
		},
	)
	.await
}

#[tauri::command]
pub async fn editor_import_external_wav(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	source_path: String,
	destination_path: String,
	expected_revision: u64,
	expected_generation: u64,
) -> Result<AssetOperationDto, String> {
	operate(
		window,
		state,
		expected_revision,
		expected_generation,
		Change::ImportAudioClip {
			source: PathBuf::from(source_path),
			destination: destination_path,
		},
	)
	.await
}

#[tauri::command]
pub async fn editor_import_sky_panorama(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	source_path: String,
	destination_path: String,
	expected_revision: u64,
	expected_generation: u64,
) -> Result<AssetOperationDto, String> {
	operate(
		window,
		state,
		expected_revision,
		expected_generation,
		Change::ImportSkyPanorama {
			source: PathBuf::from(source_path),
			destination: destination_path,
		},
	)
	.await
}

#[derive(Serialize)]
pub struct SkyPanoramaAssetDto {
	id: AssetId,
	path: String,
	#[serde(flatten)]
	info: bridge::sky_panorama::PanoramaInfo,
}

#[tauri::command]
pub async fn editor_list_sky_panoramas(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	expected_revision: u64,
	expected_generation: u64,
) -> Result<Vec<SkyPanoramaAssetDto>, String> {
	trusted_window(&window)?;
	let check = move |workspace: &mut EditorWorkspace| {
		workspace.check_revision(expected_revision)?;
		if workspace.generation != expected_generation {
			return Err("Sky Panorama picker belongs to another workspace".into());
		}
		Ok(workspace.asset_root.clone())
	};
	let root = workspace(state.clone(), false, None, check).await?;
	let assets = tauri::async_runtime::spawn_blocking(move || {
		let catalog = project_assets::load_catalog(&root)?;
		let mut assets: Vec<_> = catalog
			.assets
			.into_iter()
			.filter(|asset| asset.kind == runtime::asset_catalog::AssetKind::SkyPanorama)
			.filter_map(|asset| {
				let info =
					bridge::sky_panorama::project_info(&root, &asset.path, asset.bytes).ok()?;
				Some(SkyPanoramaAssetDto {
					id: asset.id,
					path: asset.path,
					info,
				})
			})
			.collect();
		assets.sort_unstable_by(|left, right| left.path.cmp(&right.path));
		Ok::<_, String>(assets)
	})
	.await
	.map_err(|error| error.to_string())??;
	workspace(state.clone(), false, None, check).await?;
	if let Ok(viewport) = state.viewport.lock() {
		viewport.refresh_sky(expected_generation);
	}
	Ok(assets)
}

#[tauri::command]
pub async fn editor_audio_clip_info(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	asset_id: String,
	expected_revision: u64,
	expected_generation: Option<u64>,
) -> Result<AudioClipDetailsDto, String> {
	trusted_window(&window)?;
	let id = AssetId::new(asset_id)?;
	workspace(state, false, Some("audio_clip_info"), move |workspace| {
		workspace.check_revision(expected_revision)?;
		if expected_generation.is_some_and(|generation| workspace.generation != generation) {
			return Err("Audio Clip selection belongs to another workspace".into());
		}
		let catalog = project_assets::load_catalog(&workspace.asset_root)?;
		let asset = catalog.audio_clip(&id)?;
		let info = project_assets::load_audio_clip(&workspace.asset_root, asset)?.info();
		let mut references: Vec<_> = catalog
			.assets
			.iter()
			.filter(|owner| owner.dependencies.contains(&id))
			.map(|owner| AudioClipReferenceDto {
				asset_id: owner.id.as_str().into(),
				path: owner.path.clone(),
			})
			.collect();
		let session = workspace.backend.session();
		for entry in session.project().scenes.iter() {
			if session.scene_document(entry.id()).is_ok_and(|document| {
				document.entities.iter().any(|entity| {
					entity
						.audio_source
						.as_ref()
						.and_then(|source| source.settings.clip.as_ref())
						.is_some_and(|clip| clip.asset == id)
				})
			}) && !references
				.iter()
				.any(|reference| reference.path == entry.asset_path())
			{
				references.push(AudioClipReferenceDto {
					asset_id: format!("scene:{}", entry.id().as_str()),
					path: entry.asset_path().into(),
				});
			}
		}
		references.truncate(runtime::asset_catalog::MAX_CATALOG_ASSETS);
		Ok(AudioClipDetailsDto { info, references })
	})
	.await
}
#[tauri::command]
pub async fn editor_reimport_asset(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	asset_id: String,
	source_path: String,
	expected_fingerprint: String,
	expected_revision: u64,
	expected_generation: u64,
) -> Result<AssetOperationDto, String> {
	operate(
		window,
		state,
		expected_revision,
		expected_generation,
		Change::Reimport {
			id: AssetId::new(asset_id)?,
			source: PathBuf::from(source_path),
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
	expected_generation: u64,
) -> Result<AssetOperationDto, String> {
	operate(
		window,
		state,
		expected_revision,
		expected_generation,
		Change::Move {
			id: AssetId::new(asset_id)?,
			destination: destination_path,
		},
	)
	.await
}

#[cfg(test)]
#[path = "../../../tests/unit/commands/editor/asset_authoring/contract_tests.rs"]
mod contract_tests;
