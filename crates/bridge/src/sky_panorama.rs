//! Bounded 2:1 sRGB PNG input for the single active sky texture.
use engine_core::components::{
	MAX_SKY_PANORAMA_FILE_BYTES, MAX_SKY_PANORAMA_HEIGHT, MAX_SKY_PANORAMA_WIDTH,
};
use image::{ImageFormat, ImageReader, Limits};
use std::{
	fs::{self, File},
	io::{Cursor, Read},
	path::{Path, PathBuf},
};

use crate::asset_io;

// W3C PNG Third Edition, table 17. White point, red, green and blue x/y pairs.
const SRGB_CHROMATICITIES: [u32; 8] = [
	31_270, 32_900, 64_000, 33_000, 30_000, 60_000, 15_000, 6_000,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct PanoramaInfo {
	pub width: u32,
	pub height: u32,
}

pub struct PanoramaImage {
	pub info: PanoramaInfo,
	pub rgba8: Vec<u8>,
}

fn info(bytes: &[u8]) -> Result<PanoramaInfo, String> {
	if bytes.len() < 33
		|| &bytes[..16] != b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR"
		|| bytes[24] != 8
		|| !matches!(bytes[25], 2 | 6)
	{
		return Err("Sky Panorama requires an 8-bit RGB or RGBA PNG".into());
	}
	let width = u32::from_be_bytes(bytes[16..20].try_into().expect("PNG header length checked"));
	let height = u32::from_be_bytes(bytes[20..24].try_into().expect("PNG header length checked"));
	if height == 0
		|| height > MAX_SKY_PANORAMA_HEIGHT
		|| width > MAX_SKY_PANORAMA_WIDTH
		|| width != height * 2
	{
		return Err("Sky Panorama requires an exact 2:1 image up to 2048 × 1024 pixels".into());
	}
	Ok(PanoramaInfo { width, height })
}

fn validate_color(bytes: &[u8]) -> Result<(), String> {
	let mut offset = 8usize;
	let mut has_data = false;
	for _ in 0..4096 {
		let header = bytes
			.get(offset..offset.saturating_add(8))
			.ok_or("Sky Panorama contains an incomplete PNG chunk")?;
		let length = u32::from_be_bytes(header[..4].try_into().expect("PNG chunk header")) as usize;
		let next = offset
			.checked_add(length.saturating_add(12))
			.filter(|next| *next <= bytes.len())
			.ok_or("Sky Panorama contains an invalid PNG chunk")?;
		match &header[4..8] {
			b"IDAT" => has_data = true,
			b"IEND" if length == 0 && has_data && next == bytes.len() => return Ok(()),
			b"iCCP" | b"cICP" | b"acTL" | b"zTXt" | b"iTXt" => {
				return Err(
					"Export a static sRGB PNG without embedded profiles or compressed text".into(),
				);
			}
			b"gAMA" if bytes.get(offset + 8..next - 4) != Some(&45_455u32.to_be_bytes()) => {
				return Err("Sky Panorama PNG must use sRGB color encoding".into());
			}
			b"cHRM" => {
				let payload = &bytes[offset + 8..next - 4];
				if payload.len() != std::mem::size_of_val(&SRGB_CHROMATICITIES)
					|| payload
						.chunks_exact(4)
						.zip(SRGB_CHROMATICITIES)
						.any(|(encoded, expected)| encoded != expected.to_be_bytes())
				{
					return Err("Sky Panorama PNG must use sRGB chromaticities".into());
				}
			}
			_ => {}
		}
		offset = next;
	}
	Err("Sky Panorama PNG exceeds the supported chunk count".into())
}

pub fn decode(bytes: &[u8]) -> Result<PanoramaImage, String> {
	if bytes.len() > MAX_SKY_PANORAMA_FILE_BYTES {
		return Err("Sky Panorama exceeds the 12 MiB PNG file budget".into());
	}
	let info = info(bytes)?;
	validate_color(bytes)?;
	let mut reader = ImageReader::with_format(Cursor::new(bytes), ImageFormat::Png);
	let mut limits = Limits::default();
	limits.max_image_width = Some(MAX_SKY_PANORAMA_WIDTH);
	limits.max_image_height = Some(MAX_SKY_PANORAMA_HEIGHT);
	limits.max_alloc = Some(24 * 1024 * 1024);
	reader.limits(limits);
	let rgba8 = reader
		.decode()
		.map_err(|error| format!("Cannot decode Sky Panorama PNG: {error}"))?
		.into_rgba8()
		.into_raw();
	Ok(PanoramaImage { info, rgba8 })
}

pub fn read_external(path: &Path) -> Result<Vec<u8>, String> {
	if !path.is_absolute()
		|| !path
			.extension()
			.and_then(|ext| ext.to_str())
			.is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
	{
		return Err("Choose an absolute Sky Panorama .png file".into());
	}
	let bytes = asset_io::read_bounded(path, MAX_SKY_PANORAMA_FILE_BYTES)?;
	decode(&bytes)?;
	Ok(bytes)
}

pub fn project_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
	let metadata = fs::symlink_metadata(root).map_err(|error| error.to_string())?;
	if !metadata.is_dir() || asset_io::is_link(&metadata) {
		return Err("Sky Panorama requires an ordinary asset directory".into());
	}
	let root = root.canonicalize().map_err(|error| error.to_string())?;
	asset_io::contained_file(&root, relative)
}

pub fn project_info(
	root: &Path,
	relative: &str,
	expected_bytes: u64,
) -> Result<PanoramaInfo, String> {
	let path = project_path(root, relative)?;
	let mut file = File::open(path).map_err(|error| error.to_string())?;
	if file.metadata().map_err(|error| error.to_string())?.len() != expected_bytes {
		return Err("Sky Panorama file changed".into());
	}
	let mut header = [0; 33];
	file.read_exact(&mut header)
		.map_err(|error| error.to_string())?;
	info(&header)
}

pub fn read_project_bytes(root: &Path, relative: &str) -> Result<Vec<u8>, String> {
	asset_io::read_bounded(&project_path(root, relative)?, MAX_SKY_PANORAMA_FILE_BYTES)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn with_chromaticities(values: [u32; 8], crc: u32) -> Vec<u8> {
		let original = include_bytes!("../../../compiler/tests/fixtures/sky-panorama-4x2.png");
		let mut png = original[..33].to_vec();
		png.extend_from_slice(&32u32.to_be_bytes());
		png.extend_from_slice(b"cHRM");
		png.extend(values.into_iter().flat_map(u32::to_be_bytes));
		png.extend_from_slice(&crc.to_be_bytes());
		png.extend_from_slice(&original[33..]);
		png
	}

	#[test]
	fn accepts_explicit_srgb_and_rejects_wide_gamut_chromaticities() {
		let srgb = with_chromaticities(SRGB_CHROMATICITIES, 0x9cba_513c);
		assert_eq!(
			decode(&srgb).unwrap().info,
			PanoramaInfo {
				width: 4,
				height: 2
			}
		);
		let wide = with_chromaticities(
			[
				31_270, 32_900, 68_000, 32_000, 26_500, 69_000, 15_000, 6_000,
			],
			0xf1f8_ca57,
		);
		assert!(decode(&wide).err().unwrap().contains("sRGB chromaticities"));
	}
}
