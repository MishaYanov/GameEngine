//! Canonical component-based disk format and the separate flat editor projection.
use crate::{
	EntityKey,
	components::{ComponentRecord, NO_MESH_PATH},
	scene_data::{
		CURRENT_SCHEMA_VERSION, CameraData, EntityData, LightData, LightKind, MAX_SCENE_BYTES,
		ParsedScene, SceneData, TransformData,
	},
};
use serde::{
	Deserialize, Deserializer, Serialize, Serializer,
	ser::{SerializeMap, SerializeSeq},
};
use std::collections::{BTreeMap, HashSet};

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
	#[serde(rename = "schema_version")]
	_schema_version: u32,
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
		let version = camera_schema_version(&value).map_err(serde::de::Error::custom)?;
		decode_scene(value, version).map_err(serde::de::Error::custom)
	}
}
pub(crate) fn decode_scene(value: serde_json::Value, version: u64) -> Result<SceneData, String> {
	if !matches!(version, 3..=5) {
		return Err("Direct scene DTO requires schema 3, 4 or 5".into());
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
		if !matches!(version, 4 | 5) {
			return Err("Typed component sources require scene schema 4 or 5".into());
		}
		let stored: StoredScene =
			serde_json::from_value(value).map_err(|error| error.to_string())?;
		Ok(SceneData {
			schema_version: CURRENT_SCHEMA_VERSION,
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
					|| entity.character_controller.is_some()
					|| entity.animation.is_some()
					|| entity.interactable.is_some()
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

pub(crate) fn camera_schema_version(value: &serde_json::Value) -> Result<u64, String> {
	let version = value
		.get("schema_version")
		.and_then(serde_json::Value::as_u64)
		.ok_or("Scene requires an integer schema_version")?;
	let contains_orbit = value
		.get("camera")
		.and_then(serde_json::Value::as_object)
		.is_some_and(|camera| camera.contains_key("orbit"));
	if version < 5 && contains_orbit {
		return Err("Orbit camera fields require scene schema 5".into());
	}
	Ok(version)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SceneDataV1 {
	schema_version: u32,
	entities: Vec<EntityDataV1>,
	camera: CameraData,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EntityDataV1 {
	name: String,
	model_path: String,
	transform: TransformData,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SceneDataV2 {
	schema_version: u32,
	bindings: BTreeMap<String, EntityKey>,
	entities: Vec<EntityDataV2>,
	camera: CameraData,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EntityDataV2 {
	key: EntityKey,
	name: String,
	model_path: String,
	transform: TransformData,
}

pub fn parse_json(source: &str) -> Result<ParsedScene, String> {
	if source.len() > MAX_SCENE_BYTES {
		return Err("Scene JSON exceeds the 32 MiB size limit".into());
	}
	let value: serde_json::Value =
		serde_json::from_str(source).map_err(|error| format!("Invalid scene JSON: {error}"))?;

	let source_version = crate::scene_format::camera_schema_version(&value)?;

	let parsed = match source_version {
		1 => {
			let legacy: SceneDataV1 = serde_json::from_value(value)
				.map_err(|error| format!("Invalid version-1 scene JSON: {error}"))?;

			let entities = legacy
				.entities
				.into_iter()
				.enumerate()
				.map(|(index, entity)| {
					Ok(EntityData::new(
						EntityKey::new(format!("legacy-entity-{index}"))?,
						entity.name,
						entity.model_path,
					)
					.with_transform(entity.transform))
				})
				.collect::<Result<Vec<_>, String>>()?;

			let lights = legacy_lights(&entities)?;

			ParsedScene {
				source_schema_version: legacy.schema_version,
				data: SceneData {
					schema_version: CURRENT_SCHEMA_VERSION,
					bindings: BTreeMap::new(),
					entities,
					camera: legacy.camera,
					lights,
				},
			}
		}

		2 => {
			let legacy: SceneDataV2 = serde_json::from_value(value)
				.map_err(|error| format!("Invalid version-2 scene JSON: {error}"))?;

			let entities = legacy
				.entities
				.into_iter()
				.map(|entity| {
					EntityData::new(entity.key, entity.name, entity.model_path)
						.with_transform(entity.transform)
				})
				.collect::<Vec<_>>();

			let lights = legacy_lights(&entities)?;

			ParsedScene {
				source_schema_version: legacy.schema_version,
				data: SceneData {
					schema_version: CURRENT_SCHEMA_VERSION,
					bindings: legacy.bindings,
					entities,
					camera: legacy.camera,
					lights,
				},
			}
		}

		3..=5 => {
			let data = crate::scene_format::decode_scene(value, source_version)
				.map_err(|error| format!("Invalid scene JSON: {error}"))?;

			ParsedScene {
				// Safe here: this branch accepts only 3, 4, or 5.
				source_schema_version: source_version as u32,
				data,
			}
		}

		version => {
			return Err(format!("Unsupported schema version: {version}"));
		}
	};

	parsed.data.validate()?;
	if parsed.source_schema_version != CURRENT_SCHEMA_VERSION {
		engine_logger::debug!(target: "engine_core::scene", "Scene schema migrated: from={}, to={CURRENT_SCHEMA_VERSION}", parsed.source_schema_version);
	}

	Ok(parsed)
}

fn legacy_lights(entities: &[EntityData]) -> Result<Vec<LightData>, String> {
	let occupied: HashSet<_> = entities.iter().map(|entity| entity.key.as_str()).collect();
	for index in 0..=entities.len() {
		let candidate = if index == 0 {
			"legacy-default-directional-light".to_string()
		} else {
			format!("legacy-default-directional-light-{index}")
		};
		if !occupied.contains(candidate.as_str()) {
			return Ok(vec![LightData::new(
				EntityKey::new(candidate)?,
				"Default directional light",
				LightKind::Directional,
			)]);
		}
	}
	Err("Cannot allocate a migrated default light identity".into())
}

impl SceneData {
	pub fn from_json(source: &str) -> Result<Self, String> {
		parse_json(source).map(|parsed| parsed.data)
	}

	pub fn to_json(&self) -> Result<String, String> {
		self.validate()?;
		let mut writer = SceneJsonWriter::new(Vec::new());
		serde_json::to_writer_pretty(&mut writer, &crate::scene_format::StoredSceneRef(self))
			.map_err(|error| format!("Failed to serialize scene: {error}"))?;
		String::from_utf8(writer.output).map_err(|error| format!("Failed to encode scene: {error}"))
	}

	/// Exact pretty-JSON length, with the same validation and size limit as `to_json`.
	pub fn serialized_size(&self) -> Result<usize, String> {
		self.validate()?;
		let mut writer = SceneJsonWriter::new(std::io::sink());
		serde_json::to_writer_pretty(&mut writer, &crate::scene_format::StoredSceneRef(self))
			.map_err(|error| format!("Failed to serialize scene: {error}"))?;
		Ok(writer.bytes)
	}
}

pub(crate) fn validate_authored_size(scene: &SceneData) -> Result<(), String> {
	let mut count = SceneJsonWriter::new(std::io::sink());
	serde_json::to_writer(&mut count, &crate::scene_format::StoredSceneRef(scene))
		.map_err(|error| format!("Scene authored byte validation failed: {error}"))?;
	Ok(())
}

// Stop serialization at the same limit accepted by the loader, instead of
// constructing a potentially huge String and rejecting it after allocation.
struct SceneJsonWriter<W> {
	output: W,
	bytes: usize,
}
impl<W> SceneJsonWriter<W> {
	fn new(output: W) -> Self {
		Self { output, bytes: 0 }
	}
}
impl<W: std::io::Write> std::io::Write for SceneJsonWriter<W> {
	fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
		// Atomic scene saves append one conventional trailing newline.
		if bytes.len() > (MAX_SCENE_BYTES - 1).saturating_sub(self.bytes) {
			return Err(std::io::Error::other(
				"Scene JSON exceeds the 32 MiB size limit",
			));
		}
		let written = self.output.write(bytes)?;
		self.bytes += written;
		Ok(written)
	}
	fn flush(&mut self) -> std::io::Result<()> {
		self.output.flush()
	}
}

#[cfg(test)]
#[path = "../tests/unit/scene_format/serialization_tests.rs"]
mod serialization_tests;
