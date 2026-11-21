use std::{collections::HashMap, sync::Arc};

use renderer::WindowRenderer;

use crate::{
	Camera, SceneData, Transform,
	assets::{AssetManager, Model},
	model_instance::ModelInstance,
	scene::{MAX_SCENE_RENDER_OBJECTS, Scene, validate_animation_placements},
};

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
	mut prepared: impl FnMut(usize, usize),
) -> Result<(Scene, Camera), String> {
	let placements = data.validated_world_matrices()?;
	validate_animation_placements(data, &placements)?;
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
	let animations = bridge::animation_asset::prepare_scene_with_pins(assets.root(), data, &pins)?;
	scene.synchronize_camera_settings(&data.camera);
	let mut models: HashMap<engine_core::asset_registry::AssetRef, Arc<Model>> = HashMap::new();
	let group_model = Arc::new(Model::new(Vec::new()));
	let mut render_objects = 0usize;
	for (index, entity) in data.entities.iter().enumerate() {
		let cache_key = engine_core::asset_registry::AssetRef::new(
			entity.model_path.clone(),
			Some(entity.mesh_index.unwrap_or(0)),
		)?
		.with_asset_id(entity.asset_id.clone());
		let model = if let Some(asset) = animations.get(&entity.key) {
			assets
				.load_animation(renderer, asset.clone())
				.map_err(|e| e.to_string())?
		} else if !entity.has_mesh_component()
			|| (!entity.renderable
				&& !assets
					.asset_exists(&cache_key)
					.map_err(|error| error.to_string())?)
		{
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
