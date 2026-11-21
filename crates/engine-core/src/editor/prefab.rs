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
				&& self
					.entities
					.iter()
					.any(|e| e.character_controller.is_some() || e.animation.is_some()))
		{
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

impl EditorSession {
	pub fn export_prefab(&self) -> Result<PrefabData, String> {
		self.export_prefab_with_slots(BTreeMap::new())
	}
	pub fn export_prefab_with_slots(
		&self,
		reference_slots: BTreeMap<String, EntityKey>,
	) -> Result<PrefabData, String> {
		self.require_edit()?;
		self.require_no_transaction()?;
		for target in reference_slots.values() {
			if !self
				.data
				.entities
				.iter()
				.any(|entity| &entity.key == target)
			{
				return Err(
					"Prefab reference slot must declare an existing external scene entity".into(),
				);
			}
		}
		if self.selection.is_empty() {
			return Err("Select entities to create a prefab".into());
		}
		let data = self.data.as_ref();
		let indices: HashMap<_, _> = data
			.entities
			.iter()
			.enumerate()
			.map(|(index, entity)| (&entity.key, index))
			.collect();
		let mut children = vec![Vec::new(); data.entities.len()];
		for (index, entity) in data.entities.iter().enumerate() {
			if let Some(parent) = &entity.parent {
				children[*indices
					.get(parent)
					.ok_or("Prefab source contains a missing parent")?]
				.push(index);
			}
		}
		let mut pending = self
			.selection
			.iter()
			.map(|key| {
				indices
					.get(key)
					.copied()
					.ok_or("Prefab selection contains a missing entity")
			})
			.collect::<Result<Vec<_>, _>>()?;
		let mut included = vec![false; data.entities.len()];
		let mut count = 0;
		while let Some(index) = pending.pop() {
			if included[index] {
				continue;
			}
			included[index] = true;
			count += 1;
			if count > MAX_PREFAB_ENTITIES {
				return Err("Selected prefab subtree exceeds 4096 entities".into());
			}
			pending.extend_from_slice(&children[index]);
		}
		let world = data.world_matrices()?;
		let mut entities = Vec::with_capacity(count);
		for (index, entity) in data.entities.iter().enumerate() {
			if !included[index] {
				continue;
			}
			let mut entity = entity.clone();
			entity.linked_prefab = None;
			if entity
				.parent
				.as_ref()
				.is_some_and(|parent| !included[indices[parent]])
			{
				entity.parent = None;
				entity.transform = TransformData::from_matrix(world[index])?;
			}
			entities.push(entity);
		}
		let roots: Vec<_> = entities
			.iter()
			.filter(|entity| entity.parent.is_none())
			.map(|entity| entity.transform.position)
			.collect();
		let count = roots.len().max(1) as f64;
		let pivot: [f32; 3] = std::array::from_fn(|axis| {
			(roots
				.iter()
				.map(|position| position[axis] as f64 / count)
				.sum::<f64>()) as f32
		});
		for entity in &mut entities {
			if entity.parent.is_none() {
				entity.transform.position = (glam::Vec3::from_array(entity.transform.position)
					- glam::Vec3::from_array(pivot))
				.to_array();
			}
		}
		let included: HashSet<_> = entities.iter().map(|entity| entity.key.clone()).collect();
		let behaviors: Vec<_> = self
			.project
			.behaviors_for(&self.active_scene_id)
			.filter(|behavior| included.contains(behavior.entity()))
			.cloned()
			.collect();
		let definitions_used: HashSet<_> = behaviors
			.iter()
			.filter_map(|behavior| match behavior {
				crate::project::BehaviorSpec::Shared { definition, .. } => Some(definition),
				_ => None,
			})
			.collect();
		let definitions = self
			.project
			.behavior_definitions
			.iter()
			.filter(|definition| definitions_used.contains(&definition.id))
			.cloned()
			.collect();
		let scene_settings = self
			.project
			.gameplay_settings
			.as_ref()
			.and_then(|value| value.get("scenes"))
			.and_then(|scenes| scenes.get(self.active_scene_id.as_str()))
			.cloned();
		let mut gameplay = PrefabGameplay {
			source_scene: self.active_scene_id.clone(),
			module: self.project.gameplay_module.clone(),
			bindings: data
				.bindings
				.iter()
				.filter(|(_, key)| included.contains(*key))
				.map(|(role, key)| (role.clone(), key.clone()))
				.collect(),
			behaviors,
			definitions,
			scene_settings: None,
			entity_references: BTreeMap::new(),
			scene_references: BTreeMap::new(),
		};
		if let Some(settings) = scene_settings {
			let references = (self.module_resolver)(&self.project)?.prefab_references(&settings)?;
			if references
				.entities
				.values()
				.any(|key| included.contains(key))
			{
				gameplay.scene_settings = Some(settings);
				gameplay.entity_references = references.entities;
				gameplay.scene_references = references.scenes;
			}
		}
		PrefabData {
			schema_version: 3,
			reference_slots,
			entities,
			gameplay: Some(gameplay),
		}
		.validated()
	}

	pub fn instantiate_prefab(
		&mut self,
		prefab: PrefabData,
		position: [f32; 3],
	) -> Result<crate::command::EditorSnapshotDto, String> {
		self.instantiate_prefab_command(prefab, position)?;
		self.snapshot()
	}

	pub fn instantiate_prefab_command(
		&mut self,
		prefab: PrefabData,
		position: [f32; 3],
	) -> Result<(), String> {
		self.instantiate_prefab_command_with_bindings(prefab, position, &BTreeMap::new())
	}
	pub fn instantiate_prefab_command_with_bindings(
		&mut self,
		prefab: PrefabData,
		position: [f32; 3],
		bindings: &BTreeMap<String, EntityKey>,
	) -> Result<(), String> {
		self.require_edit()?;
		for target in bindings.values() {
			if !self
				.data
				.entities
				.iter()
				.any(|entity| &entity.key == target)
			{
				return Err("Prefab reference binding targets a missing scene entity".into());
			}
		}
		if !position.iter().all(|value| value.is_finite()) {
			return Err("Prefab position must be finite".into());
		}
		let prefab = prefab.validated()?;
		if self
			.data
			.entities
			.len()
			.checked_add(prefab.entities.len())
			.is_none_or(|count| count > crate::scene_data::MAX_SCENE_ENTITIES)
		{
			return Err("Instantiating this prefab would exceed the scene entity limit".into());
		}
		let mut used: HashSet<_> = self
			.data
			.entities
			.iter()
			.map(|entity| entity.key.clone())
			.chain(self.data.lights.iter().map(|light| light.key.clone()))
			.collect();
		let remap: HashMap<_, _> = prefab
			.entities
			.iter()
			.map(|entity| {
				let key = loop {
					let key = EntityKey::generate();
					if used.insert(key.clone()) {
						break key;
					}
				};
				(entity.key.clone(), key)
			})
			.collect();
		let mut entities = prefab.remap_entities(&remap, bindings)?;
		let mut selection = Vec::new();
		for entity in &mut entities {
			if entity.parent.is_none() {
				entity.transform.position = (glam::Vec3::from_array(entity.transform.position)
					+ glam::Vec3::from_array(position))
				.to_array();
				if !entity
					.transform
					.position
					.iter()
					.all(|value| value.is_finite())
				{
					return Err(
						"Prefab placement exceeds the supported finite position range".into(),
					);
				}
				selection.push(entity.key.clone());
			}
		}
		let mut next = (*self.data).clone();
		let mut project = (*self.project).clone();
		if let Some(gameplay) = prefab.gameplay {
			for (role, key) in gameplay.bindings {
				if next.bindings.contains_key(&role) {
					return Err(format!(
						"Prefab role '{role}' is already bound. Place the assembly in an empty/new scene or unbind the existing role."
					));
				}
				next.bindings.insert(
					role,
					remap
						.get(&key)
						.ok_or("Unresolved prefab role target")?
						.clone(),
				);
			}
			let mut definition_remap = HashMap::new();
			for mut definition in gameplay.definitions {
				let old = definition.id;
				definition.id = format!("prefab-{}", uuid::Uuid::new_v4());
				definition_remap.insert(old, definition.id.clone());
				project.behavior_definitions.push(definition);
			}
			for mut behavior in gameplay.behaviors {
				match &mut behavior {
					crate::project::BehaviorSpec::ControlledMove { entity, .. }
					| crate::project::BehaviorSpec::Spin { entity, .. } => {
						*entity = remap
							.get(entity)
							.ok_or("Unresolved prefab behavior target")?
							.clone()
					}
					crate::project::BehaviorSpec::Shared { entity, definition } => {
						*entity = remap
							.get(entity)
							.ok_or("Unresolved prefab behavior target")?
							.clone();
						*definition = definition_remap
							.get(definition)
							.ok_or("Unresolved prefab shared definition")?
							.clone();
					}
				}
				project.behaviors.push(crate::project::SceneBehavior {
					scene_id: self.active_scene_id.clone(),
					behavior,
				});
			}
			if let Some(mut settings) = gameplay.scene_settings {
				if gameplay.module != project.gameplay_module {
					return Err(
						"Prefab gameplay module differs from this project's selected module".into(),
					);
				}
				let declared = (self.module_resolver)(&project)?.prefab_references(&settings)?;
				if declared.entities != gameplay.entity_references
					|| declared.scenes != gameplay.scene_references
				{
					return Err("Prefab settings references differ from the native module's declared schema".into());
				}
				for (pointer, key) in gameplay.entity_references {
					let target=remap.get(&key).ok_or_else(||format!("Prefab has unresolved external entity '{}' at '{pointer}'. Include that entity when saving the prefab.",key.as_str()))?;
					*settings
						.pointer_mut(&pointer)
						.ok_or("Prefab settings pointer is unavailable")? =
						serde_json::Value::String(target.as_str().into());
				}
				for (pointer, id) in gameplay.scene_references {
					let target = if id == gameplay.source_scene {
						&self.active_scene_id
					} else {
						project.scenes.get(&id).map_err(|_|format!("Prefab destination scene '{}' is not in this project. Add that destination or resolve the prefab reference before instantiating.",id.as_str()))?;
						&id
					};
					*settings
						.pointer_mut(&pointer)
						.ok_or("Prefab settings pointer is unavailable")? =
						serde_json::Value::String(target.as_str().into());
				}
				let config = project
					.gameplay_settings
					.get_or_insert_with(|| serde_json::json!({"schema_version":1,"scenes":{}}));
				let scenes = config
					.get_mut("scenes")
					.and_then(|value| value.as_object_mut())
					.ok_or("Prefab settings require the module's scene configuration map")?;
				if scenes.contains_key(self.active_scene_id.as_str()) {
					return Err("This scene already has gameplay settings. Place the assembly in an empty/new scene or clear its existing scene configuration.".into());
				}
				scenes.insert(self.active_scene_id.as_str().into(), settings);
			}
		}
		next.entities.extend(entities);
		self.commit_project(next, project, selection, "Instantiate gameplay prefab")
	}
}
