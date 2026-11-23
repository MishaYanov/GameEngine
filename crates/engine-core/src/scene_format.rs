//! Canonical component-based disk format and the separate flat editor projection.
use crate::{
	EntityKey,
	components::{ComponentRecord, NO_MESH_PATH},
	linked_prefab::LinkedPrefabProperty,
	scene_data::{
		CURRENT_SCHEMA_VERSION, CameraData, EntityData, LightData, LightKind, MAX_SCENE_BYTES,
		ParsedScene, SceneData, SceneEnvironment, SceneOrganization, TransformData,
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
	default_game_camera: Option<EntityKey>,
	#[serde(default)]
	lights: Vec<LightData>,
	#[serde(default)]
	organization: SceneOrganization,
	#[serde(default)]
	environment: SceneEnvironment,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredScene {
	#[serde(rename = "schema_version")]
	_schema_version: u32,
	bindings: BTreeMap<String, EntityKey>,
	entities: Vec<StoredEntity>,
	#[serde(default)]
	camera: Option<CameraData>,
	#[serde(default)]
	default_game_camera: Option<EntityKey>,
	#[serde(default)]
	lights: Vec<LightData>,
	#[serde(default)]
	organization: SceneOrganization,
	#[serde(default)]
	environment: SceneEnvironment,
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
		let version = scene_schema_version(&value).map_err(serde::de::Error::custom)?;
		let mut scene = decode_scene(value, version).map_err(serde::de::Error::custom)?;
		if version < 7 {
			scene.validate().map_err(serde::de::Error::custom)?;
			scene
				.migrate_game_camera()
				.map_err(serde::de::Error::custom)?;
		}
		Ok(scene)
	}
}

impl Serialize for SceneData {
	fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
		let mut map = serializer.serialize_map(None)?;
		map.serialize_entry("schema_version", &self.schema_version)?;
		map.serialize_entry("bindings", &self.bindings)?;
		map.serialize_entry("entities", &self.entities)?;
		let camera = self
			.authored_game_camera()
			.map_err(serde::ser::Error::custom)?;
		map.serialize_entry("camera", &camera)?;
		if let Some(default) = &self.default_game_camera {
			map.serialize_entry("default_game_camera", default)?;
		}
		map.serialize_entry("lights", &self.lights)?;
		if !self.organization.is_empty() {
			map.serialize_entry("organization", &self.organization)?;
		}
		if !self.environment.is_empty() {
			map.serialize_entry("environment", &self.environment)?;
		}
		map.end()
	}
}
pub(crate) fn decode_scene(value: serde_json::Value, version: u64) -> Result<SceneData, String> {
	if !(3..=u64::from(CURRENT_SCHEMA_VERSION)).contains(&version) {
		return Err(format!(
			"Direct scene DTO requires a schema between 3 and {CURRENT_SCHEMA_VERSION}"
		));
	}
	let canonical = (version >= 7 && value.get("camera").is_none())
		|| value
			.get("entities")
			.and_then(|value| value.as_array())
			.is_some_and(|entities| {
				entities
					.iter()
					.any(|entity| entity.get("components").is_some())
			});
	let mut scene = if canonical {
		if version < 4 {
			return Err("Typed component sources require scene schema 4 or newer".into());
		}
		let stored: StoredScene =
			serde_json::from_value(value).map_err(|error| error.to_string())?;
		let camera = match (stored.default_game_camera.as_ref(), stored.camera) {
			(Some(_), None) => CameraData::default(),
			(None, Some(camera)) => camera,
			(Some(_), Some(_)) => return Err("Canonical scene cannot contain both a Game Camera assignment and a legacy camera record".into()),
			(None, None) => return Err("Scene requires a Game Camera assignment or a compatibility camera record".into()),
		};
		SceneData {
			schema_version: CURRENT_SCHEMA_VERSION,
			bindings: stored.bindings,
			entities: stored
				.entities
				.into_iter()
				.map(StoredEntity::into_entity)
				.collect::<Result<_, _>>()?,
			camera,
			default_game_camera: stored.default_game_camera,
			lights: stored.lights,
			organization: stored.organization,
			environment: stored.environment,
		}
	} else {
		let projection: ProjectionScene =
			serde_json::from_value(value).map_err(|error| error.to_string())?;
		if projection.schema_version == 3
			&& projection.entities.iter().any(|entity| {
				!entity.native_behaviors.is_empty()
					|| !entity.scripts.is_empty()
					|| !entity.component_ids.is_empty()
					|| entity.linked_prefab.is_some()
					|| entity.character_controller.is_some()
					|| entity.animation.is_some()
					|| entity.animator.is_some()
					|| entity.interactable.is_some()
			}) {
			return Err(
				"Native component attachments and linked instances require scene schema 4".into(),
			);
		}
		SceneData {
			schema_version: CURRENT_SCHEMA_VERSION,
			bindings: projection.bindings,
			entities: projection.entities,
			camera: projection.camera,
			default_game_camera: projection.default_game_camera,
			lights: projection.lights,
			organization: projection.organization,
			environment: projection.environment,
		}
	};
	if version < 7 && scene.entities.iter().any(|entity| entity.camera.is_some()) {
		return Err("Camera objects require scene schema 7".into());
	}
	if version < 7
		&& scene
			.entities
			.iter()
			.any(|entity| entity.player_controller.is_some())
	{
		return Err("Player Controller requires scene schema 7".into());
	}
	if version < 7
		&& scene
			.entities
			.iter()
			.any(|entity| entity.audio_source.is_some())
	{
		return Err("Audio Source requires scene schema 7".into());
	}
	if version < 7
		&& (!scene.environment.is_empty()
			|| scene.entities.iter().any(|entity| entity.sky.is_some()))
	{
		return Err("Sky requires scene schema 7".into());
	}
	if version < 8 && uses_particle_emitter_schema(&scene.entities) {
		return Err("Particle Emitter requires scene schema 8".into());
	}
	scene.refresh_game_camera_projection()?;
	Ok(scene)
}

/// Version checks cover retained source records as well as the materialized scene.
fn uses_particle_emitter_schema(entities: &[EntityData]) -> bool {
	let mut pending = vec![entities];
	while let Some(entities) = pending.pop() {
		for entity in entities {
			if entity.particle_emitter.is_some() {
				return true;
			}
			if let Some(link) = &entity.linked_prefab {
				if link
					.overrides
					.iter()
					.any(|target| target.property == LinkedPrefabProperty::ParticleEmitter)
				{
					return true;
				}
				let prefab = &link.source.prefab;
				pending.push(&prefab.entities);
				pending.extend(prefab.composition.pins.iter().map(|pin| pin.entities.as_slice()));
			}
		}
	}
	false
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
		let mut map = serializer.serialize_map(Some(
			5 + usize::from(!scene.organization.is_empty())
				+ usize::from(!scene.environment.is_empty()),
		))?;
		map.serialize_entry("schema_version", &CURRENT_SCHEMA_VERSION)?;
		map.serialize_entry("bindings", &scene.bindings)?;
		map.serialize_entry("entities", &StoredEntities(&scene.entities))?;
		if let Some(default) = &scene.default_game_camera {
			map.serialize_entry("default_game_camera", default)?;
		} else {
			map.serialize_entry("camera", &scene.camera)?;
		}
		map.serialize_entry("lights", &scene.lights)?;
		if !scene.organization.is_empty() {
			map.serialize_entry("organization", &scene.organization)?;
		}
		if !scene.environment.is_empty() {
			map.serialize_entry("environment", &scene.environment)?;
		}
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

fn scene_schema_version(value: &serde_json::Value) -> Result<u64, String> {
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
	if version < 6 && value.get("organization").is_some() {
		return Err("Scene organization requires scene schema 6".into());
	}
	if version < 7 && value.get("default_game_camera").is_some() {
		return Err("Game Camera assignment requires scene schema 7".into());
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

	let source_version = scene_schema_version(&value)?;

	let mut parsed = match source_version {
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
					default_game_camera: None,
					environment: Default::default(),
					lights,
					organization: SceneOrganization::default(),
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
					default_game_camera: None,
					environment: Default::default(),
					lights,
					organization: SceneOrganization::default(),
				},
			}
		}

		3.. if source_version <= u64::from(CURRENT_SCHEMA_VERSION) => {
			let data = crate::scene_format::decode_scene(value, source_version)
				.map_err(|error| format!("Invalid scene JSON: {error}"))?;

			ParsedScene {
				// Only supported canonical versions reach this branch.
				source_schema_version: source_version as u32,
				data,
			}
		}

		version => {
			return Err(format!("Unsupported schema version: {version}"));
		}
	};

	parsed.data.validate()?;
	if source_version < 7 {
		parsed.data.migrate_game_camera()?;
	}
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
		self.check_legacy_camera_save_budget()?;
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

	/// Historical files that already use the authored budget can still be
	/// inspected and played with their old camera until enough space is freed.
	pub(crate) fn migrate_game_camera(&mut self) -> Result<(), String> {
		let legacy = self.camera.clone();
		if self.entities.len() >= crate::scene_data::MAX_SCENE_ENTITIES {
			return Ok(());
		}
		let object = self.legacy_camera_object()?;
		self.default_game_camera = Some(object.key.clone());
		self.entities.push(object);
		let validation = self
			.refresh_game_camera_projection()
			.and_then(|_| self.validate());
		if let Err(error) = validation {
			self.entities.pop();
			self.default_game_camera = None;
			self.camera = legacy;
			if !is_camera_migration_budget_error(&error) {
				return Err(error);
			}
		}
		Ok(())
	}

	fn check_legacy_camera_save_budget(&self) -> Result<(), String> {
		if self.default_game_camera.is_some() {
			return Ok(());
		}
		let mut candidate = self.clone();
		candidate.migrate_game_camera()?;
		if candidate.default_game_camera.is_none() {
			return Err("This scene uses the legacy camera because the Game Camera object would exceed its authored limits. Free scene space before saving".into());
		}
		Ok(())
	}
}

fn is_camera_migration_budget_error(error: &str) -> bool {
	error.contains("authored-data budget") || error.contains("Scene JSON exceeds")
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
