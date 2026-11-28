use super::*;

pub fn prepare_rust_script(
	asset_root: &Path,
	destination: &str,
	source: &str,
	replacement: Option<(&AssetId, &str)>,
) -> Result<PreparedAssetChange, String> {
	let root = root(asset_root)?;
	let target = asset_path(&root, destination, false)?;
	let original_catalog = catalog_bytes(&root)?;
	let mut catalog = load_catalog(&root)?;
	let original = catalog.by_path(destination);
	let old_file = match replacement {
		None if original.is_none() && !target.exists() => None,
		Some((id, expected))
		if original.is_some_and(|asset| {
			&asset.id == id && asset.kind == AssetKind::RustScript && asset.path == destination
		}) =>
			{
				let actual = fingerprint(&read(
					&target,
					engine_core::components::MAX_RUST_SCRIPT_BYTES,
				)?);
				if actual != expected {
					return Err("Rust script changed on disk after it was opened".into());
				}
				Some((target, actual))
			}
		_ => return Err("Choose an unused path or reopen the changed script".into()),
	};
	let asset = PersistentAsset {
		id: original.map_or_else(AssetId::generate, |asset| asset.id.clone()),
		path: destination.into(),
		kind: AssetKind::RustScript,
		aliases: Vec::new(),
		revision: original.map_or(Ok(1), |asset| {
			asset
				.revision
				.checked_add(1)
				.ok_or("Asset revision exhausted")
		})?,
		fingerprint: fingerprint(source.as_bytes()),
		bytes: source.len() as u64,
		source_path: None,
		meshes: Vec::new(),
		dependencies: Vec::new(),
	};
	asset.validate()?;
	bridge::script_assets::descriptor(&asset, source)?;
	catalog.replace(asset.clone())?;
	let candidate = unique(&root, "candidate.tmp");
	write_new(&candidate, source.as_bytes())?;
	Ok(PreparedAssetChange {
		root,
		operation_id: AssetId::generate().as_str().into(),
		candidate,
		destination: destination.into(),
		original_catalog,
		reimport: old_file.is_some(),
		original_asset: old_file,
		catalog,
		asset,
	})
}
