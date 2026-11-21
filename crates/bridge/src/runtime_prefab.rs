//! Shared CPU-only runtime prefab loading. Hash exactly the bytes that parse.
use engine_core::{
	asset_catalog::{AssetCatalog, AssetKind, CATALOG_FILE, MAX_CATALOG_BYTES},
	editor::PrefabData,
	scene_data::validate_model_path,
};
use sha2::{Digest, Sha256};
use std::{fs, io::Read, path::Path};
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
pub(crate) fn contained_file(root: &Path, relative: &str) -> Result<std::path::PathBuf, String> {
	validate_model_path(relative)?;
	let mut path = root.to_path_buf();
	for component in Path::new(relative).components() {
		path.push(component);
		if fs::symlink_metadata(&path)
			.map_err(|error| error.to_string())?
			.file_type()
			.is_symlink()
		{
			return Err("Runtime prefab paths must not contain symbolic links".into());
		}
	}
	let path = path.canonicalize().map_err(|error| error.to_string())?;
	if !path.starts_with(root) {
		return Err("Runtime prefab escapes its asset root".into());
	}
	Ok(path)
}
pub(crate) fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
	let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
	if !metadata.is_file() || metadata.len() > limit as u64 {
		return Err("Runtime prefab/catalog must be a regular file within its byte limit".into());
	}
	let file = fs::File::open(path).map_err(|error| error.to_string())?;
	let metadata = file.metadata().map_err(|error| error.to_string())?;
	if !metadata.is_file() || metadata.len() > limit as u64 {
		return Err("Runtime prefab/catalog must be a regular file within its byte limit".into());
	}
	let mut bytes = Vec::new();
	file.take(limit as u64 + 1)
		.read_to_end(&mut bytes)
		.map_err(|error| error.to_string())?;
	if bytes.len() > limit || bytes.len() as u64 != metadata.len() {
		return Err("Runtime prefab/catalog changed size while loading".into());
	}
	Ok(bytes)
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::path::PathBuf;
	fn fixture() -> PathBuf {
		let root = std::env::temp_dir().join(format!(
			"runtime-prefab-{}",
			engine_core::asset_catalog::AssetId::generate().as_str()
		));
		fs::create_dir(&root).unwrap();
		root
	}
	#[test]
	fn shared_loader_hashes_the_same_validated_bytes_and_rejects_escaping_or_wrong_kind() {
		let root = fixture();
		let source = r#"{"schema_version":1,"entities":[{"key":"prop","name":"Prop","model_path":"props/cube.glb","transform":{"position":[0,0,0],"rotation":[0,0,0],"scale":[1,1,1]}}]}"#;
		fs::write(root.join("item.json"), source).unwrap();
		let (prefab, fingerprint) = load(&root, "item.json").unwrap();
		assert_eq!(prefab.entities[0].key.as_str(), "prop");
		assert_eq!(
			fingerprint,
			format!("{:x}", Sha256::digest(source.as_bytes()))
		);
		for path in ["../item.json", "/item.json", "item.glb", "missing.json"] {
			assert!(load(&root, path).is_err());
		}
		fs::write(root.join("invalid.json"), "{}").unwrap();
		assert!(load(&root, "invalid.json").is_err());
		assert!(load(&root, ".").is_err());
		fs::remove_dir_all(root).unwrap();
	}
	#[test]
	fn shared_loader_rejects_oversized_sources_before_parsing() {
		let root = fixture();
		let file = fs::File::create(root.join("large.json")).unwrap();
		file.set_len(PrefabData::MAX_JSON_BYTES as u64 + 1).unwrap();
		drop(file);
		assert!(
			load(&root, "large.json")
				.unwrap_err()
				.contains("byte limit")
		);
		fs::remove_dir_all(root).unwrap();
	}
	#[test]
	fn shared_loader_resolves_reserved_prefab_move_aliases_without_retargeting_identity() {
		let root = fixture();
		let source = r#"{"schema_version":1,"entities":[{"key":"prop","name":"Prop","model_path":"props/cube.glb","transform":{"position":[0,0,0],"rotation":[0,0,0],"scale":[1,1,1]}}]}"#;
		fs::write(root.join("moved.json"), source).unwrap();
		let asset = engine_core::asset_catalog::PersistentAsset {
			id: engine_core::asset_catalog::AssetId::generate(),
			path: "moved.json".into(),
			kind: AssetKind::Prefab,
			aliases: vec!["original.json".into()],
			revision: 2,
			fingerprint: format!("{:x}", Sha256::digest(source.as_bytes())),
			bytes: source.len() as u64,
			source_path: None,
			meshes: vec![],
			dependencies: vec![],
		};
		let catalog = AssetCatalog {
			schema_version: 1,
			assets: vec![asset],
		};
		fs::write(root.join(CATALOG_FILE), catalog.to_json().unwrap()).unwrap();
		let (prefab, hash) = load(&root, "ORIGINAL.json").unwrap();
		assert_eq!(prefab.entities[0].key.as_str(), "prop");
		assert_eq!(hash, format!("{:x}", Sha256::digest(source.as_bytes())));
		assert!(!root.join("original.json").exists());
		fs::remove_dir_all(root).unwrap();
	}
	#[cfg(unix)]
	#[test]
	fn shared_loader_rejects_symbolic_links_even_when_the_target_is_contained() {
		let root = fixture();
		fs::write(root.join("data.json"), "{}").unwrap();
		std::os::unix::fs::symlink(root.join("data.json"), root.join("linked.json")).unwrap();
		assert!(load(&root, "linked.json").unwrap_err().contains("symbolic"));
		fs::remove_dir_all(root).unwrap();
	}
	#[cfg(windows)]
	#[test]
	#[ignore = "requires permission to create a temporary Windows directory junction"]
	fn shared_loader_rejects_directory_junctions_before_following_their_prefab() {
		use std::os::windows::process::CommandExt;
		let root = fixture();
		let target = root.join("actual");
		let junction = root.join("linked");
		fs::create_dir(&target).unwrap();
		fs::write(target.join("item.json"), "{}").unwrap();
		let output = std::process::Command::new("cmd")
			.args(["/C", "mklink", "/J"])
			.arg(&junction)
			.arg(&target)
			.creation_flags(0x08000000)
			.output()
			.unwrap();
		assert!(
			output.status.success(),
			"{}",
			String::from_utf8_lossy(&output.stderr)
		);
		assert!(
			load(&root, "linked/item.json")
				.unwrap_err()
				.contains("symbolic")
		);
		// Remove the junction itself before deleting the exclusively created fixture.
		fs::remove_dir(&junction).unwrap();
		assert_eq!(
			root.canonicalize().unwrap().parent(),
			Some(std::env::temp_dir().canonicalize().unwrap().as_path())
		);
		fs::remove_dir_all(root).unwrap();
	}
}
