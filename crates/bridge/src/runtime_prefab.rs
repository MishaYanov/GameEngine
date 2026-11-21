//! Shared CPU-only runtime prefab loading. Hash exactly the bytes that parse.
use engine_core::{
	asset_catalog::{AssetCatalog, AssetKind, CATALOG_FILE, MAX_CATALOG_BYTES},
	editor::PrefabData,
	scene_data::validate_model_path,
};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

use crate::asset_io::{contained_file, read_bounded};

pub fn load(root: &Path, relative: &str) -> Result<(PrefabData, String), String> {
	validate_model_path(relative)?;
	if !relative.to_ascii_lowercase().ends_with(".json") {
		return Err("Runtime prefab paths must use .json".into());
	}
	let metadata = fs::symlink_metadata(root).map_err(|error| error.to_string())?;
	if !metadata.is_dir() || metadata.file_type().is_symlink() {
		return Err("Runtime asset root must be a directory without symbolic links".into());
	}
	let root = root.canonicalize().map_err(|error| error.to_string())?;
	let resolved = match fs::symlink_metadata(root.join(CATALOG_FILE)) {
		Ok(_) => {
			let path = contained_file(&root, CATALOG_FILE)?;
			let bytes = read_bounded(&path, MAX_CATALOG_BYTES)?;
			let catalog = AssetCatalog::from_json(
				std::str::from_utf8(&bytes).map_err(|error| error.to_string())?,
			)?;
			match catalog.by_path(relative) {
				Some(asset) if asset.kind == AssetKind::Prefab => asset.path.clone(),
				Some(_) => {
					return Err("Runtime prefab reference resolves to a non-prefab asset".into());
				}
				None => relative.into(),
			}
		}
		Err(error) if error.kind() == std::io::ErrorKind::NotFound => relative.into(),
		Err(error) => return Err(error.to_string()),
	};
	if !resolved.to_ascii_lowercase().ends_with(".json") {
		return Err("Runtime prefab destination must use .json".into());
	}
	let path = contained_file(&root, &resolved)?;
	let bytes = read_bounded(&path, PrefabData::MAX_JSON_BYTES)?;
	let prefab =
		PrefabData::from_json(std::str::from_utf8(&bytes).map_err(|error| error.to_string())?)?;
	Ok((prefab, format!("{:x}", Sha256::digest(&bytes))))
}

#[cfg(test)]
#[path = "../tests/unit/runtime_prefab.rs"]
mod tests;
