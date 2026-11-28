//! Native application of a staged, same-world region change.

use super::SceneEditor;
use crate::{
	assets::AssetManager, scene::Scene, scene_catalog::SceneId, world_streaming::RegionOperation,
};
use renderer::WindowRenderer;

impl SceneEditor {
	pub fn streaming_focus(&self) -> Option<[f32; 3]> {
		self.session.as_ref()?.streaming_focus()
	}

	pub fn resident_regions(&self) -> Vec<SceneId> {
		self.session
			.as_ref()
			.map_or_else(Vec::new, |session| session.resident_regions())
	}

	pub fn can_detach_region(&self, scene_id: &SceneId) -> bool {
		self.session
			.as_ref()
			.is_some_and(|session| session.can_detach_region(scene_id))
	}

	pub fn apply_world_region(
		&mut self,
		operation: RegionOperation,
		scene: &mut Scene,
		assets: &mut AssetManager,
		renderer: &WindowRenderer,
	) -> Result<(), String> {
		let session = self.session.as_mut().ok_or("Play has stopped")?;
		let mut prepared = match operation {
			RegionOperation::Attach { region, document } => {
				session.prepare_region_attach(&region, document)?
			}
			RegionOperation::Detach { region } => session.prepare_region_detach(&region)?,
		};
		let install = (|| {
			let mut candidate = scene.prepare_region_geometry(&mut prepared, assets, renderer)?;
			if let Some(bank) = session.animations() {
				candidate.synchronize_animation(bank)?;
			}
			session.commit_region_change(prepared)?;
			*scene = candidate;
			Ok(())
		})();
		assets.prune_unused();
		install
	}
}
