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

pub(super) fn create_light(data: &mut SceneData, kind: LightKind) -> Result<(), String> {
	if data.lights.len() >= MAX_SCENE_LIGHTS {
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
	data.lights.push(light);
	Ok(())
}
