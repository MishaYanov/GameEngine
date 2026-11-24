use crate::{
	components::{ComponentData, ComponentId, ComponentRecord, NO_MESH_PATH},
	scene_data::{LightData, LightKind, MAX_SCENE_LIGHTS},
};

use super::{EntityData, EntityKey, MAX_SCENE_ENTITIES, SceneData, fresh_key};

pub(super) fn create_object(data: &mut SceneData, emitter: bool) -> Result<EntityKey, String> {
	if data.entities.len() >= MAX_SCENE_ENTITIES {
		return Err("Scene entity limit reached; remove objects before creating another".into());
	}
	let name = if emitter {
		"Particle Emitter"
	} else {
		"Empty Object"
	};
	let key = fresh_key(data);
	let mut object = EntityData::new(key.clone(), name, NO_MESH_PATH).with_renderable(false);
	if emitter {
		object.put_component(
			ComponentRecord {
				id: ComponentId::new("particle-emitter")?,
				schema_version: 1,
				enabled: true,
				data: ComponentData::ParticleEmitter {
					settings: Default::default(),
				},
			},
			false,
		)?;
	}
	data.entities.push(object);
	Ok(key)
}

pub(super) fn create_light(data: &mut SceneData, kind: LightKind) -> Result<EntityKey, String> {
	if data.light_count() >= MAX_SCENE_LIGHTS {
		return Err("Scene light limit reached; remove a light before creating another".into());
	}
	let name = match kind {
		LightKind::Directional => "Directional Light",
		LightKind::Point => "Point Light",
	};
	let point = kind == LightKind::Point;
	let mut light = LightData::new(fresh_key(data), name, kind);
	if point {
		light.position = [0.0, 3.0, 0.0];
	}
	let key = light.key.clone();
	data.entities.push(light.to_object()?);
	Ok(key)
}

/// Compatibility support for the Scene Lights list. Object users edit world
/// direction and position with the regular transform controls.
pub(super) fn upsert_light(data: &mut SceneData, light: LightData) -> Result<(), String> {
	if let Some(existing) = data.lights.iter_mut().find(|entry| entry.key == light.key) {
		*existing = light;
		return Ok(());
	}
	let Some(index) = data
		.entities
		.iter()
		.position(|entry| entry.key == light.key)
	else {
		data.entities.push(light.to_object()?);
		return Ok(());
	};
	let entity = &data.entities[index];
	let current = entity
		.light
		.as_ref()
		.ok_or("Object has no Light component")?;
	let matrices = data.world_matrices()?;
	let previous = current
		.settings
		.project(&entity.key, &entity.name, matrices[index]);
	let mut transform = entity.transform.clone();
	if light.position != previous.position
		|| (light.kind == LightKind::Directional && light.direction != previous.direction)
	{
		let inverse = entity
			.parent
			.as_ref()
			.map_or(Ok(glam::Mat4::IDENTITY), |key| {
				let parent_index = super::entity_index(data, key.as_str())?;
				super::inverse(matrices[parent_index])
			})?;
		let mut local = light.clone();
		local.position = inverse
			.transform_point3(glam::Vec3::from_array(light.position))
			.to_array();
		local.direction = inverse
			.transform_vector3(
				glam::Vec3::from_array(light.direction) * transform.scale[2].signum(),
			)
			.to_array();
		let pose = local.object_transform()?;
		if light.position != previous.position {
			transform.position = pose.position;
		}
		if light.kind == LightKind::Directional && light.direction != previous.direction {
			transform.rotation = pose.rotation;
		}
	}

	let entity = &mut data.entities[index];
	entity.name.clone_from(&light.name);
	entity.transform = transform;
	let component = entity.light.as_mut().expect("validated light object");
	component.settings = crate::components::LightSettings::from(&light);
	Ok(())
}

pub(super) fn remove_light(data: &mut SceneData, key: &str) -> Result<(), String> {
	if let Some(index) = data
		.lights
		.iter()
		.position(|light| light.key.as_str() == key)
	{
		data.lights.remove(index);
		return Ok(());
	}
	let index = super::entity_index(data, key)?;
	data.entities[index]
		.light
		.take()
		.ok_or("Object has no Light component")?;
	Ok(())
}
