use super::*;
use engine_core::input::{INPUT_ACTION_MAP_SUFFIX, InputActionMap, MAX_INPUT_ACTION_MAP_BYTES};

pub use bridge::input_actions::{load as load_input_actions, load_scene as active_input_actions};

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
	prepare_typed_input_asset(
		asset_root,
		destination,
		previous,
		map.to_json()?,
		AssetKind::InputActions,
		MAX_INPUT_ACTION_MAP_BYTES,
	)
}

pub(super) fn prepare_typed_input_asset(
	asset_root: &Path,
	destination: &str,
	previous: Option<(&AssetId, &str)>,
	bytes: Vec<u8>,
	kind: AssetKind,
	limit: usize,
) -> Result<PreparedAssetChange, String> {
	let root = root(asset_root)?;
	let target = asset_path(&root, destination, false)?;
	let original_catalog = catalog_bytes(&root)?;
	let mut catalog = load_catalog(&root)?;
	let old = match previous {
		Some((id, reviewed)) => {
			let asset = catalog.by_id(id).ok_or("Asset identity is unavailable")?;
			if asset.kind != kind || asset.path != destination || asset.fingerprint != reviewed {
				return Err("Asset changed after opening; reopen it before saving".into());
			}
			let disk = read(&target, limit)?;
			if disk.len() as u64 != asset.bytes || fingerprint(&disk) != reviewed {
				return Err("Asset changed on disk; reload it before saving".into());
			}
			Some(asset.clone())
		}
		None => {
			if catalog.by_path(destination).is_some() || target.exists() {
				return Err("Asset destination is already in use".into());
			}
			None
		}
	};
	let asset = PersistentAsset {
		id: old
			.as_ref()
			.map_or_else(AssetId::generate, |current| current.id.clone()),
		path: destination.into(),
		kind,
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
