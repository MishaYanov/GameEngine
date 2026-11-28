//! Decode bounded, catalogued PNG sprites in the Editor, Player, and compiler.

use std::{io::Cursor, path::Path};

use engine_core::asset_catalog::{
	AssetCatalog, AssetId, AssetKind, CATALOG_FILE, MAX_CATALOG_BYTES,
};
use image::{ImageFormat, ImageReader, Limits};
use sha2::{Digest, Sha256};

use crate::asset_io::{contained_file, is_link, read_bounded};

pub const MAX_SPRITE_BYTES: usize = 4 * 1024 * 1024;
const MAX_DIMENSION: u32 = 1024;

pub struct SpriteImage {
	pub width: u32,
	pub height: u32,
	pub rgba8: Vec<u8>,
}

pub fn decode(bytes: &[u8]) -> Result<SpriteImage, String> {
	if bytes.len() > MAX_SPRITE_BYTES || !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
		return Err("Particle sprites require a PNG within 4 MiB".into());
	}
	let mut reader = ImageReader::with_format(Cursor::new(bytes), ImageFormat::Png);
	let mut limits = Limits::default();
	limits.max_image_width = Some(MAX_DIMENSION);
	limits.max_image_height = Some(MAX_DIMENSION);
	limits.max_alloc = Some(16 * 1024 * 1024);
	reader.limits(limits);
	let image = reader
		.decode()
		.map_err(|error| format!("Cannot decode particle sprite PNG (up to 1024 × 1024): {error}"))?
		.into_rgba8();
	Ok(SpriteImage {
		width: image.width(),
		height: image.height(),
		rgba8: image.into_raw(),
	})
}

pub fn read_external(path: &Path) -> Result<Vec<u8>, String> {
	if !path.is_absolute() || !png_path(path) {
		return Err("Choose an absolute .png sprite file".into());
	}
	let bytes = read_bounded(path, MAX_SPRITE_BYTES)?;
	decode(&bytes)?;
	Ok(bytes)
}

pub fn read_project(root: &Path, id: &AssetId) -> Result<(Vec<u8>, SpriteImage), String> {
	let metadata = std::fs::symlink_metadata(root).map_err(|error| error.to_string())?;
	if !metadata.is_dir() || is_link(&metadata) {
		return Err("Particle sprites require an ordinary asset directory".into());
	}
	let root = root.canonicalize().map_err(|error| error.to_string())?;
	let catalog = read_bounded(&contained_file(&root, CATALOG_FILE)?, MAX_CATALOG_BYTES)?;
	let catalog =
		AssetCatalog::from_json(std::str::from_utf8(&catalog).map_err(|error| error.to_string())?)?;
	let entry = catalog
		.by_id(id)
		.filter(|entry| entry.kind == AssetKind::Texture && png_path(Path::new(&entry.path)))
		.ok_or("Particle sprite requires a catalogued PNG texture")?;
	let bytes = read_bounded(&contained_file(&root, &entry.path)?, MAX_SPRITE_BYTES)?;
	if bytes.len() as u64 != entry.bytes
		|| format!("{:x}", Sha256::digest(&bytes)) != entry.fingerprint
	{
		return Err(
			"Particle sprite changed outside its catalog; import the reviewed PNG again".into(),
		);
	}
	let sprite = decode(&bytes)?;
	Ok((bytes, sprite))
}

fn png_path(path: &Path) -> bool {
	path.extension()
		.and_then(|extension| extension.to_str())
		.is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
}
