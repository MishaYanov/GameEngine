//! Asset publication boundaries shared by the native owner and CPU-only host.
//! Temporary GPU cache entries never replace currently rendered resources until
//! the captured workspace and authored revision have been checked again.
use std::{
	sync::mpsc,
	time::{Duration, Instant},
};

use bridge::editor::EditorSnapshotDto;
use runtime::{asset_catalog::PersistentAsset, assets::project_assets::PreparedAssetChange};

use crate::editor::EditorWorkspace;

pub(crate) type AssetChangeResult = Result<(PersistentAsset, EditorSnapshotDto), String>;

pub(crate) struct AssetChangeRequest {
	pub change: PreparedAssetChange,
	pub generation: u64,
	pub revision: u64,
	pub reply: mpsc::SyncSender<AssetChangeResult>,
	pub deadline: Instant,
}

impl AssetChangeRequest {
	pub fn new(
		change: PreparedAssetChange,
		generation: u64,
		revision: u64,
		reply: mpsc::SyncSender<AssetChangeResult>,
	) -> Self {
		Self {
			change,
			generation,
			revision,
			reply,
			deadline: Instant::now() + Duration::from_secs(30),
		}
	}
	pub fn validate(&self, workspace: &EditorWorkspace) -> Result<(), String> {
		if Instant::now() > self.deadline {
			return Err("Asset operation expired before publication; retry".into());
		}
		validate_context(workspace, &self.change, self.generation, self.revision)
	}
}

pub(crate) fn validate_context(
	workspace: &EditorWorkspace,
	change: &PreparedAssetChange,
	generation: u64,
	revision: u64,
) -> Result<(), String> {
	if workspace.generation != generation {
		return Err("Asset operation belongs to a different open project".into());
	}
	if workspace.backend.session().revision() != revision {
		return Err("Authored scene changed while preparing the asset; retry".into());
	}
	if change.candidate_path().parent() != Some(workspace.asset_root.as_path()) {
		return Err("Asset operation belongs to a different asset root".into());
	}
	workspace.validate_asset_destination(change.destination_path())?;
	workspace
		.backend
		.validate_asset_invalidation()
		.map_err(|error| error.to_string())
}

/// Hold the same workspace guard from final preflight through CPU publication,
/// infallible cache promotion and metadata invalidation. The caller owns GPU
/// upload/waits, which must happen before acquiring that final guard.
pub(crate) fn publish(
	change: PreparedAssetChange,
	workspace: &mut EditorWorkspace,
	promote: impl FnOnce(),
) -> AssetChangeResult {
	workspace.validate_asset_destination(change.destination_path())?;
	workspace
		.backend
		.validate_asset_invalidation()
		.map_err(|error| error.to_string())?;
	workspace
		.backend
		.snapshot()
		.map_err(|error| error.to_string())?;
	let asset = change.publish()?;
	promote();
	let snapshot = workspace.asset_published()?;
	Ok((asset, snapshot))
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn prefab_move_rechecks_unsaved_scene_destination_before_any_publication() {
		use bridge::editor::{EditorCommandDto as Command, EditorCommandRequest};
		use runtime::{
			asset_catalog::AssetId, assets::project_assets, scene_catalog::SceneId,
			scene_data::EntityData, EntityKey,
		};
		let root = std::env::temp_dir().join(format!(
			"prefab-scene-reservation-{}",
			AssetId::generate().as_str()
		));
		std::fs::create_dir_all(root.join("assets/scenes")).unwrap();
		let asset_root = root.join("assets");
		let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
			.join("../../game/assets/props/cube.glb");
		std::fs::copy(source, asset_root.join("cube.glb")).unwrap();
		let mut scene =
			runtime::SceneData::from_json(include_str!("../../../game/assets/scenes/robots.json"))
				.unwrap();
		scene.entities.clear();
		scene.bindings.clear();
		scene.camera.follow = None;
		std::fs::write(
			asset_root.join("scenes/main.json"),
			scene.to_json().unwrap(),
		)
		.unwrap();
		let prefab = runtime::editor::PrefabData {
			schema_version: 2,
			entities: vec![EntityData::new(
				EntityKey::new("prefab").unwrap(),
				"Cube",
				"cube.glb",
			)],
			gameplay: None,
		};
		let asset = project_assets::prepare_prefab(&asset_root, "assembly.json", &prefab)
			.unwrap()
			.publish()
			.unwrap();
		let mut workspace =
			EditorWorkspace::open(asset_root.to_str().unwrap(), "scenes/main.json").unwrap();
		workspace
			.backend
			.execute(EditorCommandRequest {
				expected_revision: workspace.backend.session().revision(),
				command: Command::AddScene {
					id: SceneId::new("reserved").unwrap(),
					asset_path: "scenes/future.json".into(),
					document: scene,
				},
			})
			.unwrap();
		let before = serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap();
		let cursor = workspace.backend.change_cursor();
		let catalog = std::fs::read(asset_root.join(runtime::asset_catalog::CATALOG_FILE)).unwrap();
		let original = std::fs::read(asset_root.join("assembly.json")).unwrap();
		let prepared =
			project_assets::prepare_move(&workspace.asset_root, &asset.id, "scenes/future.json")
				.unwrap();
		let candidate = prepared.candidate_path().to_owned();
		assert!(validate_context(
			&workspace,
			&prepared,
			workspace.generation,
			workspace.backend.session().revision()
		)
		.unwrap_err()
		.contains("reserved"));
		let error = publish(prepared, &mut workspace, || {
			panic!("Rejected publication must not promote GPU resources")
		})
		.err()
		.unwrap();
		assert!(error.contains("reserved"));
		assert!(!candidate.exists());
		assert!(!asset_root.join("scenes/future.json").exists());
		assert_eq!(
			std::fs::read(asset_root.join("assembly.json")).unwrap(),
			original
		);
		assert_eq!(
			std::fs::read(asset_root.join(runtime::asset_catalog::CATALOG_FILE)).unwrap(),
			catalog
		);
		assert_eq!(
			serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap(),
			before
		);
		assert_eq!(workspace.backend.change_cursor(), cursor);
		std::fs::remove_dir_all(root).unwrap();
	}
	#[test]
	fn publication_rejects_stale_workspace_or_revision_without_replacing_files() {
		let root = std::env::temp_dir().join(format!(
			"gpu-asset-boundary-{}",
			runtime::asset_catalog::AssetId::generate().as_str()
		));
		std::fs::create_dir_all(root.join("assets/scenes")).unwrap();
		let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
			.join("../../game/assets/props/cube.glb");
		let mut scene =
			runtime::SceneData::from_json(include_str!("../../../game/assets/scenes/robots.json"))
				.unwrap();
		scene.entities.clear();
		scene.bindings.clear();
		scene.camera.follow = None;
		std::fs::write(
			root.join("assets/scenes/main.json"),
			scene.to_json().unwrap(),
		)
		.unwrap();
		let mut workspace =
			EditorWorkspace::open(root.join("assets").to_str().unwrap(), "scenes/main.json")
				.unwrap();
		let change = runtime::assets::project_assets::prepare_import(
			&workspace.asset_root,
			&source,
			"cube.glb",
		)
		.unwrap();
		assert!(validate_context(&workspace, &change, workspace.generation + 1, 0).is_err());
		assert!(validate_context(&workspace, &change, workspace.generation, u64::MAX).is_err());
		let (reply, _) = mpsc::sync_channel(1);
		let request = AssetChangeRequest {
			change,
			generation: workspace.generation,
			revision: workspace.backend.session().revision(),
			reply,
			deadline: Instant::now() - Duration::from_secs(1),
		};
		assert!(request
			.validate(&workspace)
			.unwrap_err()
			.contains("expired"));
		assert!(!workspace.asset_root.join("cube.glb").exists());
		// The same prepared bytes can publish after a fresh context capture.
		validate_context(
			&workspace,
			&request.change,
			workspace.generation,
			workspace.backend.session().revision(),
		)
		.unwrap();
		let (_, snapshot) = publish(request.change, &mut workspace, || {}).unwrap();
		assert_eq!(snapshot.revision, 1);
		assert!(workspace.asset_root.join("cube.glb").is_file());
		std::fs::remove_dir_all(root).unwrap();
	}
}
