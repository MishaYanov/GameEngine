use super::*;
use crate::assets::ImageAsset;
use bridge::sky_panorama;

pub fn prepare_sky_panorama_import(
	asset_root: &Path,
	source: &Path,
	destination: &str,
) -> Result<PreparedAssetChange, String> {
	if !destination.to_ascii_lowercase().ends_with(".png") {
		return Err("Sky Panorama destinations use .png".into());
	}
	let bytes = sky_panorama::read_external(source)?;
	prepare_binary_import(asset_root, destination, AssetKind::SkyPanorama, &bytes)
}

pub fn read_sky_panorama(
	asset_root: &Path,
	asset: &PersistentAsset,
) -> Result<(Vec<u8>, ImageAsset), String> {
	if asset.kind != AssetKind::SkyPanorama {
		return Err("Choose a Sky Panorama asset".into());
	}
	let bytes = sky_panorama::read_project_bytes(asset_root, &asset.path)?;
	if bytes.len() as u64 != asset.bytes || fingerprint(&bytes) != asset.fingerprint {
		return Err(
			"Sky Panorama changed outside its catalog; import the reviewed PNG again".into(),
		);
	}
	let decoded = sky_panorama::decode(&bytes)?;
	let image = ImageAsset::from_rgba8(decoded.info.width, decoded.info.height, decoded.rgba8)
		.map_err(|error| error.to_string())?;
	Ok((bytes, image))
}
