use super::*;
use engine_core::animation::{AuthoredClip, AuthoredClipOwner, MAX_AUTHORED_CLIP_BYTES};

pub fn read_animation_clip(
	asset_root: &Path,
	asset: &PersistentAsset,
) -> Result<AuthoredClip, String> {
	if asset.kind != AssetKind::AnimationClip
		|| !asset.path.to_ascii_lowercase().ends_with(".clip.json")
	{
		return Err("Asset is not an authored Animation Clip".into());
	}
	let path = asset_path(asset_root, &asset.path, true)?;
	let bytes = read(&path, MAX_AUTHORED_CLIP_BYTES)?;
	if bytes.len() as u64 != asset.bytes || fingerprint(&bytes) != asset.fingerprint {
		return Err("Animation Clip bytes changed outside the asset catalog".into());
	}
	AuthoredClip::from_json(&bytes)
}

pub fn prepare_animation_clip(
	asset_root: &Path,
	destination: &str,
	clip: &AuthoredClip,
	replacement: Option<(&AssetId, &str)>,
) -> Result<PreparedAssetChange, String> {
	let asset_root = root(asset_root)?;
	if !destination.to_ascii_lowercase().ends_with(".clip.json") {
		return Err("Authored Animation Clip files use .clip.json".into());
	}
	let target = asset_path(&asset_root, destination, false)?;
	let bytes = clip.to_json()?;
	let original_catalog = catalog_bytes(&asset_root)?;
	let mut catalog = load_catalog(&asset_root)?;
	let old = catalog.by_path(destination);
	let original_asset = match replacement {
		None if old.is_none() && !target.exists() => None,
		Some((id, expected)) => {
			let old = old
				.filter(|asset| {
					&asset.id == id && asset.path == destination && asset.fingerprint == expected
				})
				.ok_or("Animation Clip changed since it was opened")?;
			if read_animation_clip(&asset_root, old)?.owner != clip.owner {
				return Err("An existing Animation Clip must keep its owning Scene or Prop".into());
			}
			Some((target, expected.to_owned()))
		}
		None => return Err("Choose an unused path for the new Animation Clip".into()),
	};
	let dependencies = match &clip.owner {
		AuthoredClipOwner::Scene { .. } => Vec::new(),
		AuthoredClipOwner::Prop { asset_id } => {
			let owner = catalog
				.by_id(asset_id)
				.filter(|asset| asset.kind == AssetKind::Prefab)
				.ok_or("The owning Prop asset is missing")?;
			vec![owner.id.clone()]
		}
	};
	let asset = PersistentAsset {
		id: old.map_or_else(AssetId::generate, |asset| asset.id.clone()),
		path: destination.into(),
		kind: AssetKind::AnimationClip,
		aliases: old.map_or_else(Vec::new, |asset| asset.aliases.clone()),
		revision: old.map_or(Ok(1), |asset| {
			asset
				.revision
				.checked_add(1)
				.ok_or("Clip revision exhausted")
		})?,
		fingerprint: fingerprint(&bytes),
		bytes: bytes.len() as u64,
		source_path: None,
		meshes: Vec::new(),
		dependencies,
	};
	catalog.replace(asset.clone())?;
	let candidate = unique(&asset_root, "candidate.tmp");
	write_new(&candidate, &bytes)?;
	Ok(PreparedAssetChange {
		root: asset_root,
		operation_id: AssetId::generate().as_str().into(),
		candidate,
		destination: destination.into(),
		original_catalog,
		reimport: original_asset.is_some(),
		original_asset,
		catalog,
		asset,
	})
}
