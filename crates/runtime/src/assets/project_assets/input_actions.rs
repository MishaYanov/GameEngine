use super::*;
use engine_core::input::{INPUT_ACTION_MAP_SUFFIX, InputActionMap, MAX_INPUT_ACTION_MAP_BYTES};

pub fn load_input_actions(
	asset_root: &Path,
	asset: &PersistentAsset,
) -> Result<InputActionMap, String> {
	if asset.kind != AssetKind::InputActions
		|| !asset
			.path
			.to_ascii_lowercase()
			.ends_with(INPUT_ACTION_MAP_SUFFIX)
	{
		return Err("Select an Input Actions asset".into());
	}
	let path = asset_path(asset_root, &asset.path, true)?;
	let bytes = read(&path, MAX_INPUT_ACTION_MAP_BYTES)?;
	if asset.bytes != bytes.len() as u64 || asset.fingerprint != fingerprint(&bytes) {
		return Err("Input Actions file changed outside its catalog".into());
	}
	InputActionMap::from_json(&bytes)
}

pub fn active_input_actions(
	asset_root: &Path,
	scene: &engine_core::SceneData,
) -> Result<Option<InputActionMap>, String> {
	let Some(settings) = scene
		.local_player()
		.and_then(|player| player.player_controller.as_ref())
		.map(|component| &component.settings)
	else {
		return Ok(None);
	};
	match &settings.input {
		engine_core::components::InputActionsRef::ProjectDefault => Ok(None),
		engine_core::components::InputActionsRef::Asset { asset_id } => {
			let catalog = load_catalog(asset_root)?;
			let asset = catalog
				.by_id(asset_id)
				.ok_or("Active Player Controller Input Actions identity is unavailable")?;
			load_input_actions(asset_root, asset).map(Some)
		}
	}
}

pub fn prepare_input_actions(
	asset_root: &Path,
	destination: &str,
	previous: Option<(&AssetId, &str)>,
	map: &InputActionMap,
) -> Result<PreparedAssetChange, String> {
	if !destination
		.to_ascii_lowercase()
		.ends_with(INPUT_ACTION_MAP_SUFFIX)
	{
		return Err("Input Actions assets use .input-actions.json".into());
	}
	let bytes = map.to_json()?;
	let root = root(asset_root)?;
	let target = asset_path(&root, destination, false)?;
	let original_catalog = catalog_bytes(&root)?;
	let mut catalog = load_catalog(&root)?;
	let old = match previous {
		Some((id, reviewed)) => {
			let asset = catalog
				.by_id(id)
				.ok_or("Input Actions identity is unavailable")?;
			if asset.kind != AssetKind::InputActions
				|| asset.path != destination
				|| asset.fingerprint != reviewed
			{
				return Err(
					"Input Actions changed after opening; reopen the asset before saving".into(),
				);
			}
			let disk = read(&target, MAX_INPUT_ACTION_MAP_BYTES)?;
			if fingerprint(&disk) != reviewed {
				return Err("Input Actions changed on disk; reload it before saving".into());
			}
			Some(asset.clone())
		}
		None => {
			if catalog.by_path(destination).is_some() || target.exists() {
				return Err("Input Actions destination is already in use".into());
			}
			None
		}
	};
	let asset = PersistentAsset {
		id: old
			.as_ref()
			.map_or_else(AssetId::generate, |current| current.id.clone()),
		path: destination.into(),
		kind: AssetKind::InputActions,
		aliases: old
			.as_ref()
			.map_or_else(Vec::new, |current| current.aliases.clone()),
		revision: old.as_ref().map_or(Ok(1), |current| {
			current
				.revision
				.checked_add(1)
				.ok_or("Asset revision exhausted")
		})?,
		fingerprint: fingerprint(&bytes),
		bytes: bytes.len() as u64,
		source_path: None,
		meshes: Vec::new(),
		dependencies: Vec::new(),
	};
	catalog.replace(asset.clone())?;
	let candidate = unique(&root, "candidate.tmp");
	write_new(&candidate, &bytes)?;
	Ok(PreparedAssetChange {
		root,
		operation_id: AssetId::generate().as_str().into(),
		candidate,
		destination: destination.into(),
		original_catalog,
		reimport: old.is_some(),
		original_asset: old.map(|current| (target, current.fingerprint)),
		catalog,
		asset,
	})
}
