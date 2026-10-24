use std::{
	collections::HashMap,
	fs::{self, File, OpenOptions},
	io::{self, Read, Write},
	path::{Path, PathBuf},
	sync::{
		Arc,
		atomic::{AtomicU64, Ordering},
	},
};

use renderer::WindowRenderer;

use crate::{
	Camera, Transform,
	assets::{AssetManager, Model},
	model_instance::ModelInstance,
	scene::{EntityId, MAX_SCENE_RENDER_OBJECTS, Scene},
	scene_data::{MAX_SCENE_BYTES, ParsedScene, SceneData, parse_json},
};

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

/// Read and validate without constructing GPU objects, for Tauri document commands.
pub fn load_data(path: &Path) -> Result<ParsedScene, String> {
	let file = File::open(path)
		.map_err(|error| format!("Failed to read scene '{}': {error}", path.display()))?;
	let mut source = String::new();
	file.take(MAX_SCENE_BYTES as u64 + 1)
		.read_to_string(&mut source)
		.map_err(|error| format!("Failed to read scene '{}': {error}", path.display()))?;
	parse_json(&source)
		.map_err(|error| format!("Failed to load scene '{}': {error}", path.display()))
}

/// GPU factory used by both file loading and editor document revisions. Reusing
/// an AssetManager retains mesh/texture caches across document reinstantiation.
pub fn instantiate(
	data: &SceneData,
	assets: &mut AssetManager,
	renderer: &WindowRenderer,
) -> Result<(Scene, Camera), String> {
	data.validate()?;
	let mut scene = Scene::default();
	let mut models: HashMap<engine_core::asset_registry::AssetRef, Arc<Model>> = HashMap::new();
	let group_model = Arc::new(Model::new(Vec::new()));
	let mut render_objects = 0usize;
	for (index, entity) in data.entities.iter().enumerate() {
		let cache_key = engine_core::asset_registry::AssetRef::new(
			entity.model_path.clone(),
			Some(entity.mesh_index.unwrap_or(0)),
		)?;
		let model = if !entity.renderable {
			Arc::clone(&group_model)
		} else if let Some(model) = models.get(&cache_key) {
			Arc::clone(model)
		} else {
			let loaded = assets.load_asset(renderer, &cache_key);
			let model = loaded.map_err(|error| {
				format!(
					"Scene entity {index} ({:?}), model '{}': {error}",
					entity.name, entity.model_path
				)
			})?;
			models.insert(cache_key, Arc::clone(&model));
			model
		};
		render_objects = render_objects
			.checked_add(model.parts().len())
			.ok_or("Scene render-object count overflow")?;
		if render_objects > MAX_SCENE_RENDER_OBJECTS {
			return Err(format!(
				"Scene requires more than {MAX_SCENE_RENDER_OBJECTS} render objects"
			));
		}
		let transform = Transform {
			position: entity.transform.position,
			rotation: entity.transform.rotation,
			scale: entity.transform.scale,
		};
		let id = scene
			.spawn_keyed_model(
				entity.key.clone(),
				entity.name.clone(),
				entity.model_path.clone(),
				ModelInstance::new(model, transform),
			)
			.map_err(|error| format!("Scene entity {index}: {error}"))?;
		scene.set_entity_metadata(id, entity)?;
	}

	for (name, key) in &data.bindings {
		let id = scene.entity_by_key(key).ok_or_else(|| {
			format!(
				"Scene binding {name:?}: missing entity key {:?}",
				key.as_str()
			)
		})?;
		scene.set_binding(name.clone(), id)?;
	}
	scene.set_lights(data.lights.clone());

	let camera = Camera {
		position: data.camera.position,
		yaw: data.camera.yaw,
		pitch: data.camera.pitch,
		vertical_fov: data.camera.vertical_fov,
		near: data.camera.near_plane,
		far: data.camera.far_plane,
	};

	Ok((scene, camera))
}

pub fn save(path: &Path, scene: &Scene, camera: &Camera) -> Result<(), String> {
	save_data(path, &scene.to_data(camera)?)
}

/// Atomic persistence shared by the CPU editor and live runtime scene wrapper.
pub fn save_data(path: &Path, data: &SceneData) -> Result<(), String> {
	let mut json = data.to_json()?;
	json.push('\n');
	save_json(path, &json)
}

/// The development harness persists authored configuration beside its assets.
/// Scene and project files are separate atomic replacements, not a two-file transaction.
pub fn save_project_data(path: &Path, data: &crate::project::ProjectData) -> Result<(), String> {
	let mut json = data.to_json()?;
	json.push('\n');
	save_json(path, &json)
}

fn save_json(path: &Path, json: &str) -> Result<(), String> {
	let (temporary_path, mut file) = create_temporary_file(path)?;
	let write_result = file
		.write_all(json.as_bytes())
		.and_then(|()| file.sync_all());
	// Close before rename, including on Windows where an open file can block it.
	drop(file);

	if let Err(error) = write_result {
		let _ = fs::remove_file(&temporary_path);
		return Err(format!(
			"Failed to write scene '{}' through '{}': {error}",
			path.display(),
			temporary_path.display()
		));
	}

	// The sibling temporary file is on the same filesystem. Never remove the
	// previous scene first: a failed replacement must leave it available.
	if let Err(error) = fs::rename(&temporary_path, path) {
		let _ = fs::remove_file(&temporary_path);
		return Err(format!(
			"Failed to replace scene '{}': {error}",
			path.display()
		));
	}

	Ok(())
}

fn create_temporary_file(path: &Path) -> Result<(PathBuf, File), String> {
	static NEXT_TEMPORARY_ID: AtomicU64 = AtomicU64::new(0);

	let file_name = path
		.file_name()
		.ok_or_else(|| format!("Scene path '{}' has no file name", path.display()))?;

	for _ in 0..128 {
		let sequence = NEXT_TEMPORARY_ID.fetch_add(1, Ordering::Relaxed);
		let mut temporary_name = file_name.to_os_string();
		temporary_name.push(format!(".tmp-{}-{sequence}", std::process::id()));
		let temporary_path = path.with_file_name(temporary_name);

		match OpenOptions::new()
			.write(true)
			.create_new(true)
			.open(&temporary_path)
		{
			Ok(file) => return Ok((temporary_path, file)),
			Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
			Err(error) => {
				return Err(format!(
					"Failed to create temporary scene file '{}': {error}",
					temporary_path.display()
				));
			}
		}
	}

	Err(format!(
		"Failed to save scene '{}': temporary file names are already in use",
		path.display()
	))
}

#[cfg(test)]
#[path = "tests_scene_io.rs"]
mod tests;
