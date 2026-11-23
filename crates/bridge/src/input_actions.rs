//! Shared, bounded Input Actions lookup for the editor, standalone Player and headless runner.
use crate::asset_io::{contained_file, is_link, read_bounded};
use engine_core::{
	SceneData,
	asset_catalog::{
		AssetCatalog, AssetId, AssetKind, CATALOG_FILE, MAX_CATALOG_BYTES, PersistentAsset,
	},
	components::InputActionsRef,
	input::{InputActionMap, MAX_INPUT_ACTION_MAP_BYTES},
};
use sha2::{Digest, Sha256};
use std::{
	fs,
	path::{Path, PathBuf},
};

pub(crate) fn canonical_root(root: &Path) -> Result<PathBuf, String> {
	let metadata = fs::symlink_metadata(root).map_err(|error| error.to_string())?;
	if !metadata.is_dir() || is_link(&metadata) {
		return Err("Asset root must be an ordinary directory".into());
	}
	root.canonicalize().map_err(|error| error.to_string())
}

pub fn load(root: &Path, asset: &PersistentAsset) -> Result<InputActionMap, String> {
	asset.validate()?;
	if asset.kind != AssetKind::InputActions {
		return Err("Player Controller requires an Input Actions asset".into());
	}
	let root = canonical_root(root)?;
	let path = contained_file(&root, &asset.path)?;
	let bytes = read_bounded(&path, MAX_INPUT_ACTION_MAP_BYTES)?;
	if asset.bytes != bytes.len() as u64
		|| asset.fingerprint != format!("{:x}", Sha256::digest(&bytes))
	{
		return Err("Input Actions file changed outside its catalog".into());
	}
	InputActionMap::from_json(&bytes)
}

pub fn load_scene(root: &Path, scene: &SceneData) -> Result<Option<InputActionMap>, String> {
	let Some(asset_id) = scene_asset_id(scene) else {
		return Ok(None);
	};
	let root = canonical_root(root)?;
	let catalog = read_catalog(&root)?;
	let asset = catalog
		.by_id(asset_id)
		.ok_or("Player Controller Input Actions identity is unavailable")?;
	load(&root, asset).map(Some)
}

pub fn scene_asset_id(scene: &SceneData) -> Option<&AssetId> {
	let component = scene.local_player()?.player_controller.as_ref()?;
	match &component.settings.input {
		InputActionsRef::Asset { asset_id } => Some(asset_id),
		InputActionsRef::ProjectDefault => None,
	}
}

pub(crate) fn read_catalog(root: &Path) -> Result<AssetCatalog, String> {
	let bytes = read_bounded(&contained_file(&root, CATALOG_FILE)?, MAX_CATALOG_BYTES)?;
	AssetCatalog::from_json(std::str::from_utf8(&bytes).map_err(|error| error.to_string())?)
}
