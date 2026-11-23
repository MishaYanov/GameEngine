//! Stage a versioned embedded GLB through the normal project asset transaction.
use super::*;

use engine_core::PrimitiveKind;
use std::io::ErrorKind;

const PRIMITIVE_DIRECTORY: &str = "models/primitives/v1";

pub enum PreparedPrimitive {
	Existing(PersistentAsset),
	New(Box<PreparedAssetChange>),
}

/// Stage the built-in model for this shape, or reuse its verified catalog identity.
/// The host supplies the bundled bytes and serializes preparation with publication.
pub fn prepare_primitive(
	asset_root: &Path,
	kind: PrimitiveKind,
	bytes: &[u8],
) -> Result<PreparedPrimitive, String> {
	if bytes.len() > MAX_SOURCE || !bytes.starts_with(b"glTF") {
		return Err("Built-in primitive requires a binary GLB within its byte limit".into());
	}
	let destination = format!("{PRIMITIVE_DIRECTORY}/{}.glb", kind.as_str());
	let root = root(asset_root)?;
	let original_catalog = catalog_bytes(&root)?;
	let catalog = match original_catalog.as_deref() {
		Some(bytes) => {
			AssetCatalog::from_json(std::str::from_utf8(bytes).map_err(|error| error.to_string())?)?
		}
		None => AssetCatalog::default(),
	};
	let digest = fingerprint(bytes);
	if let Some(asset) = catalog.by_path(&destination) {
		let conflict = || {
			format!("Built-in primitive destination {destination} is reserved by a different asset")
		};
		if asset.kind != AssetKind::Model
			|| asset.fingerprint != digest
			|| asset.bytes != bytes.len() as u64
			|| asset.meshes.is_empty()
			|| !asset.dependencies.is_empty()
		{
			return Err(conflict());
		}
		// An asset moved by the author keeps its historical path reserved in the catalog.
		let path = asset_path(&root, &asset.path, false)?;
		read(&path, bytes.len())
			.map_err(|error| {
				format!(
					"Built-in primitive asset {} is unavailable: {error}",
					asset.path
				)
			})
			.and_then(|current| {
				if current == bytes {
					Ok(PreparedPrimitive::Existing(asset.clone()))
				} else {
					Err(conflict())
				}
			})
	} else {
		stage_new_primitive(root, destination, original_catalog, catalog, digest, bytes)
	}
}

fn stage_new_primitive(
	root: PathBuf,
	destination: String,
	original_catalog: Option<Vec<u8>>,
	mut catalog: AssetCatalog,
	digest: String,
	bytes: &[u8],
) -> Result<PreparedPrimitive, String> {
	let candidate = unique(&root, "candidate.tmp");
	write_new(&candidate, bytes)?;
	let result = (|| {
		let meshes = signatures(&candidate, &root, true)?;
		ensure_primitive_directory(&root)?;
		let path = asset_path(&root, &destination, false)?;
		match fs::symlink_metadata(path) {
			Ok(_) => {
				return Err(format!(
					"Built-in primitive destination {destination} already exists"
				));
			}
			Err(error) if error.kind() == ErrorKind::NotFound => {}
			Err(error) => return Err(error.to_string()),
		}
		let asset = PersistentAsset {
			id: AssetId::generate(),
			path: destination.clone(),
			kind: AssetKind::Model,
			aliases: Vec::new(),
			revision: 1,
			fingerprint: digest,
			bytes: bytes.len() as u64,
			source_path: None,
			meshes,
			dependencies: Vec::new(),
		};
		catalog.replace(asset.clone())?;
		Ok(PreparedPrimitive::New(Box::new(PreparedAssetChange {
			root,
			operation_id: AssetId::generate().as_str().into(),
			candidate: candidate.clone(),
			destination,
			original_catalog,
			original_asset: None,
			catalog,
			asset,
			reimport: false,
		})))
	})();
	if result.is_err() {
		let _ = fs::remove_file(candidate);
	}
	result
}

fn ensure_primitive_directory(root: &Path) -> Result<(), String> {
	let mut path = root.to_path_buf();
	for component in PRIMITIVE_DIRECTORY.split('/') {
		path.push(component);
		match fs::create_dir(&path) {
			Ok(()) => {}
			Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
			Err(error) => return Err(error.to_string()),
		}
		let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
		if !metadata.is_dir() || is_linked(&metadata) {
			return Err(format!(
				"Built-in primitive directory {PRIMITIVE_DIRECTORY} is blocked; expected ordinary folders"
			));
		}
	}
	Ok(())
}
