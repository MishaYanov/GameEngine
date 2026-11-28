//! Project paths and Inspector declarations for attachable Rust source assets.

use std::path::{Path, PathBuf};

use engine_core::{
	asset_catalog::{
		AssetCatalog, AssetId, AssetKind, CATALOG_FILE, MAX_CATALOG_BYTES, PersistentAsset,
	},
	components::{MAX_RUST_SCRIPT_BYTES, ScriptDescriptor, ScriptSettings, project_script_id},
};
use sha2::{Digest, Sha256};

use crate::asset_io::{contained_file, is_link, read_bounded};

pub const MAX_SCRIPT_ARTIFACT_BYTES: usize = 8 * 1024 * 1024;
const DECLARATION: &str = "// engine-script ";

pub struct ScriptMetadata {
	pub descriptor: ScriptDescriptor,
	pub particle_sprites: Vec<AssetId>,
}

pub fn catalog(root: &Path) -> Result<AssetCatalog, String> {
	let root = checked_root(root)?;
	let path = root.join(CATALOG_FILE);
	match std::fs::symlink_metadata(path) {
		Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(AssetCatalog::default()),
		Err(error) => Err(error.to_string()),
		Ok(_) => {
			let bytes = read_bounded(&contained_file(&root, CATALOG_FILE)?, MAX_CATALOG_BYTES)?;
			AssetCatalog::from_json(std::str::from_utf8(&bytes).map_err(|error| error.to_string())?)
		}
	}
}

pub fn read_source(root: &Path, asset: &PersistentAsset) -> Result<String, String> {
	if asset.kind != AssetKind::RustScript {
		return Err("Choose a Rust script asset".into());
	}
	asset.validate()?;
	let path = contained_file(&checked_root(root)?, &asset.path)?;
	String::from_utf8(read_bounded(&path, MAX_RUST_SCRIPT_BYTES)?)
		.map_err(|error| format!("{}: {error}", asset.path))
}

pub fn read_packaged_source(root: &Path, asset: &PersistentAsset) -> Result<String, String> {
	let source = read_source(root, asset)?;
	if source.len() as u64 != asset.bytes
		|| format!("{:x}", Sha256::digest(source.as_bytes())) != asset.fingerprint
	{
		return Err(format!("{} changed after packaging", asset.path));
	}
	Ok(source)
}

pub fn descriptor(asset: &PersistentAsset, source: &str) -> Result<ScriptDescriptor, String> {
	metadata(asset, source).map(|metadata| metadata.descriptor)
}

pub fn metadata(asset: &PersistentAsset, source: &str) -> Result<ScriptMetadata, String> {
	let label = Path::new(&asset.path)
		.file_stem()
		.and_then(|value| value.to_str())
		.unwrap_or("Rust Script");
	let mut declaration = serde_json::json!({
		"id": project_script_id(&asset.id), "label": label, "config_version": 1,
		"fields": [], "references": [], "receive_local_actions": true,
	});
	let mut sprites = Vec::new();
	if let Some(text) = source
		.trim_start_matches('\u{feff}')
		.lines()
		.next()
		.and_then(|line| line.strip_prefix(DECLARATION))
	{
		let mut properties: serde_json::Map<String, serde_json::Value> =
			serde_json::from_str(text).map_err(|error| format!("{}:1: {error}", asset.path))?;
		if let Some(value) = properties.remove("particle_sprites") {
			sprites = serde_json::from_value(value)
				.map_err(|error| format!("{}:1: {error}", asset.path))?;
		}
		for (key, value) in properties {
			if key != "id" {
				declaration[&key] = value;
			}
		}
	}
	let descriptor: ScriptDescriptor = serde_json::from_value(declaration)
		.map_err(|error| format!("{}:1: {error}", asset.path))?;
	let defaults = ScriptSettings {
		script_id: descriptor.id.clone(),
		config_version: descriptor.config_version,
		properties: Default::default(),
		references: Default::default(),
	};
	defaults.validate()?;
	if descriptor.fields.len() > 32 || descriptor.references.len() > 32 || sprites.len() > 32 {
		return Err(format!(
			"{}:1: Script settings or references exceed their limit",
			asset.path
		));
	}
	Ok(ScriptMetadata {
		descriptor,
		particle_sprites: sprites,
	})
}

pub fn artifact_path(asset: &AssetId) -> String {
	format!("script-compiled/{}.wasm", asset.as_str())
}

pub fn read_artifact(root: &Path, id: &AssetId) -> Result<Vec<u8>, String> {
	read_bounded(
		&contained_file(&checked_root(root)?, &artifact_path(id))?,
		MAX_SCRIPT_ARTIFACT_BYTES,
	)
}

fn checked_root(root: &Path) -> Result<PathBuf, String> {
	let metadata = std::fs::symlink_metadata(root).map_err(|error| error.to_string())?;
	if !metadata.is_dir() || is_link(&metadata) {
		return Err("Rust scripts require an ordinary asset directory".into());
	}
	root.canonicalize().map_err(|error| error.to_string())
}
