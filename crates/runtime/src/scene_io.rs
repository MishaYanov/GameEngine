use std::path::Path;

use renderer::WindowRenderer;

use crate::{
	Camera,
	assets::AssetManager,
	scene::{EntityId, Scene},
};

mod document;
mod native;
pub(crate) mod paths;
mod references;
pub use paths::resolve_confined_scene_path;

pub use document::{load_data, replace_bytes, save_data, save_new_bytes, save_project_data};
pub use native::{
	instantiate, instantiate_cancellable, instantiate_with_progress, instantiate_with_references,
	instantiate_with_warming,
};
pub use references::PreparedSceneReferences;

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
	load_parsed(&parsed, asset_root, renderer)
}

pub fn load_parsed(
	parsed: &crate::scene_data::ParsedScene,
	asset_root: &Path,
	renderer: &WindowRenderer,
) -> Result<LoadedScene, String> {
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
