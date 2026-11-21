use std::path::Path;

use renderer::WindowRenderer;

use crate::{
	Camera,
	assets::AssetManager,
	scene::{EntityId, Scene},
};

mod document;
mod native;

pub use document::{load_data, save_data, save_project_data};
pub use native::{instantiate, instantiate_with_progress};

pub struct LoadedScene {
	pub scene: Scene,
	pub assets: AssetManager,
	pub camera: Camera,
	pub entity_ids: Vec<EntityId>,
	pub source_schema_version: u32,
}

pub fn load(
	path: &Path,
	asset_root: &Path,
	renderer: &WindowRenderer,
) -> Result<LoadedScene, String> {
	let parsed = load_data(path)?;
	let mut assets = AssetManager::new(asset_root).map_err(|error| {
		format!(
			"Failed to open asset root '{}': {error}",
			asset_root.display()
		)
	})?;
	let (scene, camera) = instantiate(&parsed.data, &mut assets, renderer)?;
	let entity_ids = scene.entity_ids().collect();
	Ok(LoadedScene {
		scene,
		assets,
		camera,
		entity_ids,
		source_schema_version: parsed.source_schema_version,
	})
}

pub fn save(path: &Path, scene: &Scene, camera: &Camera) -> Result<(), String> {
	save_data(path, &scene.to_data(camera)?)
}

#[cfg(test)]
#[path = "../tests/unit/scene_io.rs"]
mod tests;
