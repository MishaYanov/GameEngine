use std::{fs, path::Path};

use engine_core::asset_catalog::{
	AssetCatalog, AssetId, AssetKind, CATALOG_FILE, MAX_CATALOG_BYTES, PersistentAsset,
};

use super::prefab::prefab_dependencies;
use super::{
	MAX_SOURCE, asset_path, catalog_bytes, fingerprint, install_new, load_catalog, read, root,
	signatures, unique, write_new,
};

// Bound the combined bytes examined by one legacy catalog migration.
const MAX_MIGRATION_SOURCE_BYTES: u64 = 512 * 1024 * 1024;

/// Register legacy on-disk models without rewriting authored scene documents.
/// This is explicit, bounded migration; aliases resolve old paths after moves.
pub fn migrate_catalog(asset_root: &Path) -> Result<AssetCatalog, String> {
	migrate_catalog_with_budget(asset_root, MAX_MIGRATION_SOURCE_BYTES)
}
pub(super) fn migrate_catalog_with_budget(
	asset_root: &Path,
	source_budget: u64,
) -> Result<AssetCatalog, String> {
	let asset_root = root(asset_root)?;
	let original_catalog = catalog_bytes(&asset_root)?;
	let mut catalog = load_catalog(&asset_root)?;
	let initial = catalog.clone();
	let mut total = catalog.assets.iter().try_fold(0u64, |total, asset| {
		total
			.checked_add(asset.bytes)
			.ok_or("Asset migration size overflow")
	})?;
	if total > source_budget {
		return Err("Asset migration source budget exceeded".into());
	}
	for model in crate::assets::catalog::scan_models(&asset_root)? {
		if catalog.by_path(&model.path).is_some() {
			continue;
		}
		let path = asset_path(&asset_root, &model.path, true)?;
		let bytes = read(&path, MAX_SOURCE)?;
		total = total
			.checked_add(bytes.len() as u64)
			.ok_or("Asset migration size overflow")?;
		if total > source_budget {
			return Err("Asset migration source budget exceeds 512 MiB".into());
		}
		// Migration validates metadata/geometry; it does not force .gltf conversion.
		let meshes = signatures(&path, &asset_root, false)?;
		catalog.replace(PersistentAsset {
			id: AssetId::generate(),
			path: model.path,
			kind: AssetKind::Model,
			aliases: Vec::new(),
			revision: 1,
			fingerprint: fingerprint(&bytes),
			bytes: bytes.len() as u64,
			source_path: None,
			meshes,
			dependencies: Vec::new(),
		})?;
	}
	for relative in prefab_paths(&asset_root)? {
		if catalog.by_path(&relative).is_some() {
			continue;
		}
		let path = asset_path(&asset_root, &relative, true)?;
		let bytes = read(&path, engine_core::editor::PrefabData::MAX_JSON_BYTES)?;
		total = total
			.checked_add(bytes.len() as u64)
			.ok_or("Asset migration size overflow")?;
		if total > source_budget {
			return Err("Asset migration source budget exceeded while examining JSON".into());
		}
		let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
			continue;
		};
		if value.get("entities").is_none()
			|| value.get("schema_version").is_none()
			|| value.get("camera").is_some()
		{
			continue;
		}
		let prefab = engine_core::editor::PrefabData::from_json(
			std::str::from_utf8(&bytes).map_err(|e| e.to_string())?,
		)?;
		let dependencies = prefab_dependencies(&catalog, &prefab)?;
		catalog.replace(PersistentAsset {
			id: AssetId::generate(),
			path: relative,
			kind: AssetKind::Prefab,
			aliases: Vec::new(),
			revision: 1,
			fingerprint: fingerprint(&bytes),
			bytes: bytes.len() as u64,
			source_path: None,
			meshes: Vec::new(),
			dependencies,
		})?;
	}
	if catalog != initial {
		if catalog_bytes(&asset_root)? != original_catalog {
			return Err("Asset catalog changed while preparing migration; retry".into());
		}
		let target = asset_root.join(CATALOG_FILE);
		let temp = unique(&asset_root, "catalog.tmp");
		write_new(&temp, catalog.to_json()?.as_bytes())?;
		let backup = unique(&asset_root, "catalog-previous.tmp");
		if let Some(expected) = &original_catalog {
			if let Err(error) = fs::rename(&target, &backup) {
				let _ = fs::remove_file(&temp);
				return Err(error.to_string());
			}
			if !read(&backup, MAX_CATALOG_BYTES).is_ok_and(|bytes| bytes == *expected) {
				let recovery = install_new(&backup, &target);
				let _ = fs::remove_file(&temp);
				return Err(format!(
					"Asset catalog changed at the migration boundary; original retained at {}; recovery: {recovery:?}",
					backup.display()
				));
			}
		}
		if let Err(error) = install_new(&temp, &target) {
			let recovery = if original_catalog.is_some() {
				install_new(&backup, &target)
			} else {
				Ok(())
			};
			let _ = fs::remove_file(&temp);
			return Err(format!(
				"{error}; original catalog retained at {}; recovery: {recovery:?}",
				backup.display()
			));
		}
		let _ = fs::remove_file(temp);
		let _ = fs::remove_file(backup);
	}
	Ok(catalog)
}
fn prefab_paths(asset_root: &Path) -> Result<Vec<String>, String> {
	let mut pending = vec![(asset_root.to_path_buf(), 0)];
	let mut paths = Vec::new();
	let mut visited = 0;
	while let Some((directory, depth)) = pending.pop() {
		if depth > 32 {
			return Err("Asset directory nesting exceeds 32 levels".into());
		}
		for entry in fs::read_dir(directory).map_err(|e| e.to_string())? {
			let entry = entry.map_err(|e| e.to_string())?;
			visited += 1;
			if visited > 10_000 {
				return Err("Asset catalog exceeds 10000 entries".into());
			}
			let kind = entry.file_type().map_err(|e| e.to_string())?;
			if kind.is_symlink() {
				continue;
			}
			let path = entry.path();
			if kind.is_dir() {
				pending.push((path, depth + 1));
			} else if kind.is_file()
				&& path
					.extension()
					.and_then(|value| value.to_str())
					.is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
				&& path.file_name().and_then(|value| value.to_str()) != Some(CATALOG_FILE)
			{
				paths.push(
					path.strip_prefix(asset_root)
						.map_err(|e| e.to_string())?
						.to_str()
						.ok_or("Asset path must be UTF-8")?
						.replace('\\', "/"),
				);
			}
		}
	}
	paths.sort();
	Ok(paths)
}
