use super::*;

pub fn prepare_particle_sprite_import(
	asset_root: &Path,
	source: &Path,
	destination: &str,
) -> Result<PreparedAssetChange, String> {
	if !destination.to_ascii_lowercase().ends_with(".png") {
		return Err("Particle sprite destinations use .png".into());
	}
	let bytes = bridge::particle_sprite::read_external(source)?;
	prepare_binary_import(asset_root, destination, AssetKind::Texture, &bytes)
}
