use super::*;
use crate::components::{ComponentData, ComponentId, ComponentRecord, NO_MESH_PATH};

pub(super) fn create_sky(data: &mut SceneData) -> Result<EntityKey, String> {
	if data.entities.len() >= MAX_SCENE_ENTITIES {
		return Err("Scene entity limit reached; remove entities before creating a Sky".into());
	}
	let key = fresh_key(data);
	let mut sky = EntityData::new(key.clone(), "Sky", NO_MESH_PATH).with_renderable(false);
	sky.put_component(
		ComponentRecord {
			id: ComponentId::new("sky")?,
			schema_version: 1,
			enabled: true,
			data: ComponentData::Sky {
				settings: Default::default(),
			},
		},
		false,
	)?;
	data.entities.push(sky);
	Ok(key)
}
