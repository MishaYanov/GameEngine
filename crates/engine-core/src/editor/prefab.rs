mod export;
mod instantiate;
use super::EditorSession;
use crate::{
	EntityKey,
	scene_data::{CURRENT_SCHEMA_VERSION, CameraData, EntityData, SceneData, TransformData},
};
use serde::{Deserialize, Serialize};
use std::{
	collections::{BTreeMap, HashMap, HashSet},
	io::{self, Write},
};

pub const MAX_PREFAB_ENTITIES: usize = 4096;
pub const MAX_PREFAB_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrefabData {
	pub schema_version: u32,
	#[serde(deserialize_with = "crate::scene_format::deserialize_entities")]
	pub entities: Vec<EntityData>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub gameplay: Option<PrefabGameplay>,
	#[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
	pub reference_slots: BTreeMap<String, EntityKey>,
}

/// A portable gameplay assembly. References into opaque module settings are
/// explicit JSON pointers, so remapping never guesses that arbitrary text is an ID.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrefabGameplay {
	pub source_scene: crate::scene_catalog::SceneId,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub module: Option<String>,
	#[serde(default)]
	pub bindings: BTreeMap<String, EntityKey>,
	#[serde(default)]
	pub behaviors: Vec<crate::project::BehaviorSpec>,
	#[serde(default)]
	pub definitions: Vec<crate::project::BehaviorDefinition>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub scene_settings: Option<serde_json::Value>,
	#[serde(default)]
	pub entity_references: BTreeMap<String, EntityKey>,
	#[serde(default)]
	pub scene_references: BTreeMap<String, crate::scene_catalog::SceneId>,
}

impl PrefabData {
	pub const MAX_ENTITIES: usize = MAX_PREFAB_ENTITIES;
	pub const MAX_JSON_BYTES: usize = MAX_PREFAB_BYTES;
	pub fn from_json(source: &str) -> Result<Self, String> {
		if source.len() > MAX_PREFAB_BYTES {
			return Err("Prefab JSON exceeds the 16 MiB size limit".into());
		}
		serde_json::from_str::<Self>(source)
			.map_err(|error| format!("Invalid prefab JSON: {error}"))?
			.validated()
	}

	pub fn validate(&self) -> Result<(), String> {
		self.validate_settings()?;
		self.clone().validated().map(|_| ())
	}
	fn validate_settings(&self) -> Result<(), String> {
		if let Some(settings) = self
			.gameplay
			.as_ref()
			.and_then(|gameplay| gameplay.scene_settings.as_ref())
		{
			crate::project::validate_gameplay_payload(settings)?;
		}
		Ok(())
	}

	pub fn to_json(&self) -> Result<String, String> {
		self.validate()?;
		struct Canonical<'a>(&'a PrefabData);
		impl Serialize for Canonical<'_> {
			fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
				self.0.serialize_canonical(serializer)
			}
		}
		let source =
			serde_json::to_string_pretty(&Canonical(self)).map_err(|error| error.to_string())?;
		if source.len() > MAX_PREFAB_BYTES {
			return Err("Formatted prefab JSON exceeds the 16 MiB size limit".into());
		}
		Ok(source)
	}
	pub fn serialize_canonical<S: serde::Serializer>(
		&self,
		serializer: S,
	) -> Result<S::Ok, S::Error> {
		#[derive(Serialize)]
		struct Stored<'a> {
			schema_version: u32,
			#[serde(serialize_with = "crate::scene_format::serialize_entities")]
			entities: &'a [EntityData],
			gameplay: &'a Option<PrefabGameplay>,
			reference_slots: &'a BTreeMap<String, EntityKey>,
		}
		Stored {
			schema_version: 3,
			entities: &self.entities,
			gameplay: &self.gameplay,
			reference_slots: &self.reference_slots,
		}
		.serialize(serializer)
	}

	fn validated(self) -> Result<Self, String> {
		self.validate_settings()?;
		if !matches!(self.schema_version, 1..=3)
			|| self.entities.is_empty()
			|| self.entities.len() > MAX_PREFAB_ENTITIES
			|| (self.schema_version == 1 && self.gameplay.is_some())
			|| (self.schema_version < 3 && !self.reference_slots.is_empty())
			|| (self.schema_version < 3
			&& self.entities.iter().any(|e| {
			e.character_controller.is_some()
				|| e.animation.is_some()
				|| e.interactable.is_some()
		})) {
			return Err(
				"A version-1/2/3 prefab requires 1–4096 entities; gameplay requires version 2 and reference slots version 3".into(),
			);
		}
		// A byte-counting writer checks the owned DTO without allocating a
		// second JSON document. Direct bridge DTOs use the same size contract.
		self.serialize_canonical(&mut serde_json::Serializer::new(SizeBudget(0)))
			.map_err(|error| format!("Prefab size validation failed: {error}"))?;
		let camera = crate::Camera::default();
		if self
			.entities
			.iter()
			.any(|entity| entity.linked_prefab.is_some())
		{
			return Err("Prefab sources cannot contain linked instances; export a resolved copy or Unpack them first".into());
		}
		if self.reference_slots.len() > 128 {
			return Err("Prefab exceeds 128 reference slots".into());
		}
		let original_count = self.entities.len();
		let mut entities = self.entities;
		let mut keys: HashSet<_> = entities.iter().map(|entity| entity.key.clone()).collect();
		if entities.iter().any(|entity| {
			entity
				.parent
				.as_ref()
				.is_some_and(|parent| !keys.contains(parent))
		}) {
			return Err("Prefab parents must be owned source entities; reference slots bind component references, not external parents".into());
		}
		for (name, key) in &self.reference_slots {
			if name.is_empty()
				|| name.len() > 64
				|| !name
					.bytes()
					.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
				|| !keys.insert(key.clone())
			{
				return Err("Reference slots require unique external placeholder keys and 1–64 character ASCII names".into());
			}
			entities.push(
				EntityData::new(
					key.clone(),
					format!("Reference slot {name}"),
					crate::components::NO_MESH_PATH,
				)
				.with_renderable(false),
			);
		}
		let mut validation = SceneData {
			schema_version: CURRENT_SCHEMA_VERSION,
			organization: Default::default(),
			entities,
			bindings: self
				.gameplay
				.as_ref()
				.map_or_else(BTreeMap::new, |gameplay| gameplay.bindings.clone()),
			camera: CameraData {
				follow: None,
				orbit: None,
				position: camera.position,
				yaw: camera.yaw,
				pitch: camera.pitch,
				vertical_fov: camera.vertical_fov,
				near_plane: camera.near,
				far_plane: camera.far,
			},
			lights: Vec::new(),
		};
		// The isolated scene rejects duplicate/invalid keys, external parents,
		// cycles, invalid paths and every material/collider/transform defect.
		// External placeholders deliberately have no capabilities. Validate the
		// entity graph first, then restore all attachments for the slot-aware
		// component validation below (including disabled machine definitions).
		let attachments: Vec<_> = validation
			.entities
			.iter_mut()
			.map(|entity| std::mem::take(&mut entity.native_behaviors))
			.collect();
		validation.validate()?;
		for (entity, attachments) in validation.entities.iter_mut().zip(attachments) {
			entity.native_behaviors = attachments;
		}
		crate::components::validate_scene_components_with_slots(
			&validation,
			&self.reference_slots.values().cloned().collect(),
		)?;
		validation.entities.truncate(original_count);
		if let Some(gameplay) = &self.gameplay {
			let keys: HashSet<_> = validation
				.entities
				.iter()
				.map(|entity| &entity.key)
				.collect();
			let mut definitions = HashMap::new();
			for definition in &gameplay.definitions {
				definition.validate()?;
				if definitions
					.insert(&definition.id, &definition.settings)
					.is_some()
				{
					return Err("Duplicate prefab behavior definition".into());
				}
			}
			if gameplay.behaviors.len() > crate::project::MAX_BEHAVIORS
				|| gameplay.definitions.len() > crate::project::MAX_BEHAVIOR_DEFINITIONS
				|| gameplay.entity_references.len() + gameplay.scene_references.len()
					> MAX_PREFAB_ENTITIES
			{
				return Err("Prefab gameplay reference budget exceeded".into());
			}
			for behavior in &gameplay.behaviors {
				behavior.validate()?;
				if !keys.contains(behavior.entity()) {
					return Err("Prefab behavior targets an external entity".into());
				}
				if let crate::project::BehaviorSpec::Shared { definition, .. } = behavior
					&& !definitions.contains_key(definition)
				{
					return Err("Prefab shared behavior definition is unresolved".into());
				}
			}
			for (pointer, target) in gameplay
				.entity_references
				.iter()
				.map(|(pointer, target)| (pointer, target.as_str()))
				.chain(
					gameplay
						.scene_references
						.iter()
						.map(|(pointer, target)| (pointer, target.as_str())),
				) {
				if pointer.len() > 1024
					|| gameplay
						.scene_settings
						.as_ref()
						.and_then(|value| value.pointer(pointer))
						.and_then(|value| value.as_str())
						!= Some(target)
				{
					return Err("Prefab settings reference does not match its JSON pointer".into());
				}
			}
			if gameplay.module.as_ref().is_some_and(|module| {
				module.is_empty() || module.len() > 128 || module.chars().any(char::is_control)
			}) {
				return Err("Invalid prefab gameplay module".into());
			}
		}
		Ok(Self {
			schema_version: 3,
			entities: validation.entities,
			gameplay: self.gameplay,
			reference_slots: self.reference_slots,
		})
	}

	/// Pure ownership remapping for editor copies, linked instances and runtime
	/// spawn candidates. Component IDs remain stable within their remapped owner.
	pub fn remap_entities(
		&self,
		keys: &HashMap<EntityKey, EntityKey>,
		bindings: &BTreeMap<String, EntityKey>,
	) -> Result<Vec<EntityData>, String> {
		self.validate()?;
		if bindings.len() != self.reference_slots.len()
			|| bindings
				.keys()
				.any(|name| !self.reference_slots.contains_key(name))
		{
			return Err(
				"Supply exactly one entity binding for every declared prefab reference slot".into(),
			);
		}
		let mut remap = keys.clone();
		let mut unique = HashSet::new();
		for entity in &self.entities {
			let key = keys
				.get(&entity.key)
				.ok_or("Prefab entity mapping is incomplete")?;
			key.validate()?;
			if !unique.insert(key.clone()) {
				return Err("Prefab mapped entity keys must be distinct".into());
			}
		}
		for (name, placeholder) in &self.reference_slots {
			let target = bindings
				.get(name)
				.ok_or("A prefab reference slot is unbound")?;
			target.validate()?;
			if unique.contains(target) {
				return Err("Prefab reference slots must bind an existing external entity".into());
			}
			remap.insert(placeholder.clone(), target.clone());
		}
		self.entities
			.iter()
			.map(|source| {
				let mut entity = source.clone();
				entity.key = remap
					.get(&source.key)
					.expect("owned mapping checked")
					.clone();
				entity.parent = source
					.parent
					.as_ref()
					.map(|parent| {
						keys.get(parent)
							.cloned()
							.ok_or("Prefab parent is outside its owned entities")
					})
					.transpose()?;
				entity.remap_component_references(&remap)?;
				Ok(entity)
			})
			.collect()
	}

	pub fn into_component_source(mut self) -> Result<Self, String> {
		if self.gameplay.as_ref().is_some_and(|gameplay| {
			!gameplay.bindings.is_empty()
				|| !gameplay.behaviors.is_empty()
				|| !gameplay.definitions.is_empty()
				|| gameplay.scene_settings.is_some()
				|| !gameplay.entity_references.is_empty()
				|| !gameplay.scene_references.is_empty()
		}) {
			return Err("This selection contains legacy project gameplay. Use Instantiate Copy or convert its gameplay to entity components before creating a linked source".into());
		}
		self.gameplay = None;
		self.schema_version = 3;
		self.validated()
	}
}

struct SizeBudget(usize);
impl Write for SizeBudget {
	fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
		self.0 = self
			.0
			.checked_add(bytes.len())
			.filter(|count| *count <= MAX_PREFAB_BYTES)
			.ok_or_else(|| {
				io::Error::new(
					io::ErrorKind::InvalidInput,
					"Prefab exceeds the 16 MiB size limit",
				)
			})?;
		Ok(bytes.len())
	}
	fn flush(&mut self) -> io::Result<()> {
		Ok(())
	}
}
