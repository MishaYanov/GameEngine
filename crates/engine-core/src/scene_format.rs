//! Canonical version-4 disk format and the separate flat editor projection.
use crate::{
	EntityKey,
	components::{ComponentRecord, NO_MESH_PATH},
	scene_data::{
		CURRENT_SCHEMA_VERSION, CameraData, EntityData, LightData, SceneData, TransformData,
	},
};
use serde::{
	Deserialize, Deserializer, Serialize, Serializer,
	ser::{SerializeMap, SerializeSeq},
};
use std::collections::BTreeMap;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectionScene {
	schema_version: u32,
	bindings: BTreeMap<String, EntityKey>,
	entities: Vec<EntityData>,
	camera: CameraData,
	#[serde(default)]
	lights: Vec<LightData>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredScene {
	schema_version: u32,
	bindings: BTreeMap<String, EntityKey>,
	entities: Vec<StoredEntity>,
	camera: CameraData,
	#[serde(default)]
	lights: Vec<LightData>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredEntity {
	key: EntityKey,
	name: String,
	transform: TransformData,
	#[serde(default)]
	parent: Option<EntityKey>,
	components: Vec<ComponentRecord>,
	#[serde(default)]
	linked_prefab: Option<crate::linked_prefab::LinkedPrefabInstance>,
}
impl StoredEntity {
	fn into_entity(self) -> Result<EntityData, String> {
		if self.components.len() > crate::components::MAX_COMPONENTS_PER_OBJECT {
			return Err("Object component count exceeds its limit".into());
		}
		let mut entity = EntityData::new(self.key, self.name, NO_MESH_PATH)
			.with_transform(self.transform)
			.with_parent(self.parent)
			.with_renderable(false);
		let ids = crate::components::singleton_ids(&self.components);
		for component in self.components {
			entity.put_component(component, false)?;
		}
		entity.component_ids = ids;
		entity.linked_prefab = self.linked_prefab;
		Ok(entity)
	}
}
impl<'de> Deserialize<'de> for SceneData {
	fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
		let value = serde_json::Value::deserialize(deserializer)?;
		decode_scene(value).map_err(serde::de::Error::custom)
	}
}
fn decode_scene(value: serde_json::Value) -> Result<SceneData, String> {
	let version = value
		.get("schema_version")
		.and_then(|value| value.as_u64())
		.ok_or("Scene requires an integer schema_version")?;
	if version != 3 && version != CURRENT_SCHEMA_VERSION as u64 {
		return Err("Direct scene DTO requires schema 3 or 4; use the historical scene loader for earlier versions".into());
	}
	let canonical = value
		.get("entities")
		.and_then(|value| value.as_array())
		.is_some_and(|entities| {
			entities
				.iter()
				.any(|entity| entity.get("components").is_some())
		});
	if canonical {
		if version != CURRENT_SCHEMA_VERSION as u64 {
			return Err("Typed component sources require scene schema 4".into());
		}
		let stored: StoredScene =
			serde_json::from_value(value).map_err(|error| error.to_string())?;
		Ok(SceneData {
			schema_version: stored.schema_version,
			bindings: stored.bindings,
			entities: stored
				.entities
				.into_iter()
				.map(StoredEntity::into_entity)
				.collect::<Result<_, _>>()?,
			camera: stored.camera,
			lights: stored.lights,
		})
	} else {
		let projection: ProjectionScene =
			serde_json::from_value(value).map_err(|error| error.to_string())?;
		if projection.schema_version == 3
			&& projection.entities.iter().any(|entity| {
				!entity.native_behaviors.is_empty()
					|| !entity.component_ids.is_empty()
					|| entity.linked_prefab.is_some()
			}) {
			return Err(
				"Native component attachments and linked instances require scene schema 4".into(),
			);
		}
		Ok(SceneData {
			schema_version: CURRENT_SCHEMA_VERSION,
			bindings: projection.bindings,
			entities: projection.entities,
			camera: projection.camera,
			lights: projection.lights,
		})
	}
}

struct StoredEntityRef<'a>(&'a EntityData);
impl Serialize for StoredEntityRef<'_> {
	fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
		let entity = self.0;
		let mut map = serializer.serialize_map(None)?;
		map.serialize_entry("key", &entity.key)?;
		map.serialize_entry("name", &entity.name)?;
		map.serialize_entry("transform", &entity.transform)?;
		if let Some(parent) = &entity.parent {
			map.serialize_entry("parent", parent)?;
		}
		if let Some(link) = &entity.linked_prefab {
			map.serialize_entry("linked_prefab", link)?;
		}
		map.serialize_entry("components", &entity.component_records())?;
		map.end()
	}
}
struct StoredEntities<'a>(&'a [EntityData]);
impl Serialize for StoredEntities<'_> {
	fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
		let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
		for entity in self.0 {
			sequence.serialize_element(&StoredEntityRef(entity))?;
		}
		sequence.end()
	}
}
pub(crate) struct StoredSceneRef<'a>(pub &'a SceneData);
impl Serialize for StoredSceneRef<'_> {
	fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
		let scene = self.0;
		let mut map = serializer.serialize_map(Some(5))?;
		map.serialize_entry("schema_version", &CURRENT_SCHEMA_VERSION)?;
		map.serialize_entry("bindings", &scene.bindings)?;
		map.serialize_entry("entities", &StoredEntities(&scene.entities))?;
		map.serialize_entry("camera", &scene.camera)?;
		map.serialize_entry("lights", &scene.lights)?;
		map.end()
	}
}
/// Prefab writers can use the same canonical entity codec without a second
/// component representation. Flat bridge DTOs continue using EntityData serde.
pub fn serialize_entities<S: Serializer>(
	entities: &[EntityData],
	serializer: S,
) -> Result<S::Ok, S::Error> {
	StoredEntities(entities).serialize(serializer)
}
pub fn deserialize_entities<'de, D: Deserializer<'de>>(
	deserializer: D,
) -> Result<Vec<EntityData>, D::Error> {
	let value = serde_json::Value::deserialize(deserializer)?;
	let list = value
		.as_array()
		.ok_or_else(|| serde::de::Error::custom("Entity collection must be an array"))?;
	let canonical = list.iter().any(|entity| entity.get("components").is_some());
	if canonical {
		serde_json::from_value::<Vec<StoredEntity>>(value)
			.map_err(serde::de::Error::custom)?
			.into_iter()
			.map(|entity| entity.into_entity().map_err(serde::de::Error::custom))
			.collect()
	} else {
		serde_json::from_value(value).map_err(serde::de::Error::custom)
	}
}
