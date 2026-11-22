use super::{Closure, Path, SceneData};
use engine_core::{
	animation::{AuthoredClip, AuthoredClipOwner},
	asset_catalog::{AssetKind, PersistentAsset},
	editor::PrefabData,
};

impl Closure<'_> {
	pub(super) fn record_animation_library(&mut self) -> Result<(), String> {
		let clips: Vec<_> = self
			.catalog
			.assets
			.iter()
			.filter(|asset| asset.kind == AssetKind::AnimationClip)
			.map(|asset| (asset.id.clone(), asset.path.clone()))
			.collect();
		for (id, path) in clips {
			self.record(&id)
				.map_err(|error| format!("Animation Clip {path}: {error}"))?;
		}
		Ok(())
	}

	pub(super) fn validate_animation_clip(
		&mut self,
		asset: &PersistentAsset,
		bytes: &[u8],
	) -> Result<(), String> {
		let clip = AuthoredClip::from_json(bytes)?;
		match &clip.owner {
			AuthoredClipOwner::Scene { scene_id } => {
				if !asset.dependencies.is_empty() {
					return Err("A Scene Animation Clip has unexpected catalog dependencies".into());
				}
				let path = self.project.scene_path(scene_id)?.to_owned();
				let bytes = self.file(Path::new(&path))?;
				let scene = SceneData::from_json(
					std::str::from_utf8(&bytes).map_err(|error| error.to_string())?,
				)?;
				clip.resolve_scene(self.project, scene_id, &scene)?;
			}
			AuthoredClipOwner::Prop { asset_id } => {
				if asset.dependencies.as_slice() != std::slice::from_ref(asset_id) {
					return Err("Animation Clip dependencies do not match its owning Prop".into());
				}
				let path = self
					.catalog
					.by_id(asset_id)
					.filter(|owner| owner.kind == AssetKind::Prefab)
					.ok_or("Animation Clip requires a registered owning Prop")?
					.path
					.clone();
				let bytes = self.file(Path::new(&path))?;
				let prop = PrefabData::from_json(
					std::str::from_utf8(&bytes).map_err(|error| error.to_string())?,
				)?;
				clip.resolve_prop(asset_id, &prop)?;
			}
		}
		Ok(())
	}
}
