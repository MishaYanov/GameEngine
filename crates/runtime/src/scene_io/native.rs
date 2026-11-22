use std::{borrow::Cow, collections::HashMap, sync::Arc};

use renderer::WindowRenderer;

use crate::{
	Camera, SceneData, Transform,
	assets::{AssetManager, Model, PreparedModelBatch},
	model_instance::ModelInstance,
	scene::{MAX_SCENE_RENDER_OBJECTS, Scene},
};

use super::references::{PreparedSceneReferences, reference, validate_layout};

/// GPU factory used by both file loading and editor document revisions. Reusing
/// an AssetManager retains mesh/texture caches across document reinstantiation.
pub fn instantiate(
	data: &SceneData,
	assets: &mut AssetManager,
	renderer: &WindowRenderer,
) -> Result<(Scene, Camera), String> {
	instantiate_with_progress(data, assets, renderer, |_, _| {})
}

/// Reports actual scene objects as they finish construction. An object's GPU
/// resources are either prepared here or reused from the asset cache.
pub fn instantiate_with_progress(
	data: &SceneData,
	assets: &mut AssetManager,
	renderer: &WindowRenderer,
	prepared: impl FnMut(usize, usize),
) -> Result<(Scene, Camera), String> {
	instantiate_cancellable(data, assets, renderer, prepared, || false)?
		.ok_or_else(|| "Scene construction was cancelled".into())
}

/// Stops at bounded scene assembly boundaries and leaves publication to the caller.
pub fn instantiate_cancellable(
	data: &SceneData,
	assets: &mut AssetManager,
	renderer: &WindowRenderer,
	prepared: impl FnMut(usize, usize),
	cancelled: impl FnMut() -> bool,
) -> Result<Option<(Scene, Camera)>, String> {
	instantiate_with_references(data, assets, renderer, None, prepared, cancelled)
}

/// Use checked, bounded CPU references only for the exact retained document.
pub fn instantiate_with_references(
	data: &SceneData,
	assets: &mut AssetManager,
	renderer: &WindowRenderer,
	references: Option<PreparedSceneReferences>,
	prepared: impl FnMut(usize, usize),
	cancelled: impl FnMut() -> bool,
) -> Result<Option<(Scene, Camera)>, String> {
	instantiate_with_warming(
		data, assets, renderer, references, None, prepared, cancelled,
	)
}

/// Adopt optional CPU model payloads for the exact retained scene. Ordinary
/// assembly and GPU cache lookups remain authoritative when a payload is absent.
pub fn instantiate_with_warming(
	data: &SceneData,
	assets: &mut AssetManager,
	renderer: &WindowRenderer,
	references: Option<PreparedSceneReferences>,
	mut warming: Option<&mut PreparedModelBatch>,
	mut prepared: impl FnMut(usize, usize),
	mut cancelled: impl FnMut() -> bool,
) -> Result<Option<(Scene, Camera)>, String> {
	if cancelled() {
		return Ok(None);
	}
	let references = references.filter(|references| references.matches(data));
	if references.is_none() {
		warming = None;
		validate_layout(data)?;
	}
	let total = data.entities.len();
	prepared(0, total);
	let mut scene = Scene::default();
	scene.set_animation_root(assets.root().to_path_buf());
	let pins: engine_core::animation::PreparedAnimationBindings = data
		.entities
		.iter()
		.filter_map(|entity| {
			entity
				.animation
				.as_ref()
				.and_then(|c| assets.cached_animation(&c.settings.source))
				.map(|asset| (entity.key.clone(), asset))
		})
		.collect();
	if cancelled() {
		return Ok(None);
	}
	let animations = bridge::animation_asset::prepare_scene_with_pins(assets.root(), data, &pins)?;
	scene.synchronize_camera_settings(&data.camera);
	let mut models: HashMap<engine_core::asset_registry::AssetRef, Arc<Model>> = HashMap::new();
	let group_model = Arc::new(Model::new(Vec::new()));
	let mut render_objects = 0usize;
	for (index, entity) in data.entities.iter().enumerate() {
		if cancelled() {
			return Ok(None);
		}
		let cache_key = if let Some(references) = &references {
			Cow::Borrowed(references.asset(index))
		} else {
			Cow::Owned(reference(entity)?)
		};
		let model = if let Some(asset) = animations.get(&entity.key) {
			assets
				.load_animation(renderer, asset.clone())
				.map_err(|e| e.to_string())?
		} else if !entity.has_mesh_component()
			|| (!entity.renderable
				&& !assets
					.asset_exists(cache_key.as_ref())
					.map_err(|error| error.to_string())?)
		{
			Arc::clone(&group_model)
		} else if let Some(model) = models.get(cache_key.as_ref()) {
			Arc::clone(model)
		} else {
			let warm_model = if entity.renderable && entity.animation.is_none() {
				warming.as_deref_mut()
			} else {
				None
			};
			let loaded = assets.load_asset_prepared(renderer, cache_key.as_ref(), warm_model);
			let model = loaded.map_err(|error| {
				format!(
					"Scene entity {index} ({:?}), model '{}': {error}",
					entity.name, entity.model_path
				)
			})?;
			models.insert(cache_key.into_owned(), Arc::clone(&model));
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
		let mut instance = ModelInstance::new(model, transform);
		if let Some(asset) = animations.get(&entity.key) {
			let component = entity.animation.as_ref().unwrap();
			let clip_index = match component.settings.playback {
				engine_core::animation::AnimationPlayback::SingleClip { clip_index, .. } => {
					clip_index
				}
				engine_core::animation::AnimationPlayback::Locomotion3 { idle, .. } => idle,
			};
			let mut pose = engine_core::animation::sample_pose(
				asset.clone(),
				clip_index,
				0.,
				glam::Mat4::IDENTITY,
				0,
			)?;
			if !component.enabled {
				pose.local = asset.nodes.iter().map(|n| n.rest).collect();
				pose.globals = engine_core::animation::global_pose(asset, &pose.local)?;
				pose.palette = engine_core::animation::mesh_local_palette(asset, &pose.globals)?;
				pose.mesh_matrix = pose.globals[asset.skin.mesh_node as usize];
			}
			instance.set_animation_pose(&pose, true)?;
		}
		let id = scene
			.spawn_keyed_model(
				entity.key.clone(),
				entity.name.clone(),
				entity.model_path.clone(),
				instance,
			)
			.map_err(|error| format!("Scene entity {index}: {error}"))?;
		scene.set_entity_metadata(id, entity)?;
		if cancelled() {
			return Ok(None);
		}
		prepared(index + 1, total);
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
	scene.set_organization(&data.organization);

	let camera = Camera {
		position: data.camera.position,
		yaw: data.camera.yaw,
		pitch: data.camera.pitch,
		vertical_fov: data.camera.vertical_fov,
		near: data.camera.near_plane,
		far: data.camera.far_plane,
	};

	Ok(Some((scene, camera)))
}
