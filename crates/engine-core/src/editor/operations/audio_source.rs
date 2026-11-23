use super::*;
use crate::components::{ComponentData, ComponentId, ComponentRecord, NO_MESH_PATH};

pub(super) fn create_audio_source(data: &mut SceneData) -> Result<EntityKey, String> {
	if data.entities.len() >= MAX_SCENE_ENTITIES {
		return Err(
			"Scene entity limit reached; remove entities before creating an Audio Source".into(),
		);
	}
	let key = fresh_key(data);
	let mut object =
		EntityData::new(key.clone(), "Audio Source", NO_MESH_PATH).with_renderable(false);
	object.put_component(
		ComponentRecord {
			id: ComponentId::new("audio")?,
			schema_version: 1,
			enabled: true,
			data: ComponentData::AudioSource {
				settings: Default::default(),
			},
		},
		false,
	)?;
	data.entities.push(object);
	Ok(key)
}
