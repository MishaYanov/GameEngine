use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{self, Write},
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
    scene::{EntityId, Scene},
	scene_data::parse_json,
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
    let source = fs::read_to_string(path)
        .map_err(|error| format!("Failed to read scene '{}': {error}", path.display()))?;
	let parsed = parse_json(&source)
        .map_err(|error| format!("Failed to load scene '{}': {error}", path.display()))?;
	let data = parsed.data;
    let mut assets = AssetManager::new(asset_root).map_err(|error| {
        format!(
            "Failed to open asset root '{}': {error}",
            asset_root.display()
        )
    })?;
    let mut scene = Scene::default();
    let mut entity_ids = Vec::with_capacity(data.entities.len());
    let mut models: HashMap<String, Arc<Model>> = HashMap::new();

    for (index, entity) in data.entities.into_iter().enumerate() {
        let model = if let Some(model) = models.get(&entity.model_path) {
            Arc::clone(model)
        } else {
            let model = Arc::new(assets.load_model(renderer, &entity.model_path).map_err(
                |error| {
                    format!(
                        "Scene '{}', entity {index} ({:?}), model '{}': {error}",
                        path.display(),
                        entity.name,
                        entity.model_path
                    )
                },
            )?);
            models.insert(entity.model_path.clone(), Arc::clone(&model));
            model
        };
        let transform = Transform {
            position: entity.transform.position,
            rotation: entity.transform.rotation,
            scale: entity.transform.scale,
        };
		let id = scene
			.spawn_keyed_model(
				entity.key,
			    entity.name,
			    entity.model_path,
			    ModelInstance::new(model, transform),
			)
			.map_err(|error| format!("Scene '{}', entity {index}: {error}", path.display()))?;
		entity_ids.push(id);
	}

	for (name, key) in data.bindings {
		let id = scene.entity_by_key(&key).ok_or_else(|| {
			format!(
				"Scene '{}', binding {name:?}: missing entity key {:?}",
				path.display(),
				key.as_str()
			)
		})?;
		scene.set_binding(name, id)?;
    }

    let camera = Camera {
        position: data.camera.position,
        yaw: data.camera.yaw,
        pitch: data.camera.pitch,
        vertical_fov: data.camera.vertical_fov,
        near: data.camera.near_plane,
        far: data.camera.far_plane,
    };

    Ok(LoadedScene {
        scene,
        assets,
        camera,
        entity_ids,
		source_schema_version: parsed.source_schema_version,
    })
}

pub fn save(path: &Path, scene: &Scene, camera: &Camera) -> Result<(), String> {
    let mut json = scene.to_data(camera)?.to_json()?;
    json.push('\n');

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
