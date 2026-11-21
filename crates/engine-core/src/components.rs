//! Typed component adapters over the existing single entity backing store.
use crate::{
	EntityKey,
	asset_catalog::AssetId,
	scene_data::{ColliderData, EntityData, MaterialOverrideData, SceneData, validate_model_path},
};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

pub const MAX_COMPONENTS_PER_OBJECT: usize = 16;
pub const MAX_NATIVE_BEHAVIORS_PER_OBJECT: usize = 8;
pub const MAX_ACTIVE_NATIVE_BEHAVIORS: usize = 4096;
pub const MESH_TYPE: &str = "engine.mesh_renderer";
pub const COLLIDER_TYPE: &str = "engine.collider";
pub const NATIVE_TYPE: &str = "engine.native_behavior";
pub const CHARACTER_TYPE: &str = "engine.character_controller";
pub const ANIMATION_TYPE: &str = "engine.animation";
pub const NO_MESH_PATH: &str = "__no_mesh__";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ComponentId(String);
impl ComponentId {
	pub fn new(value: impl Into<String>) -> Result<Self, String> {
		let value = value.into();
		if value.is_empty()
			|| value.len() > 128
			|| !value
				.bytes()
				.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
		{
			return Err(
				"Component ID requires 1–128 ASCII letters, digits, underscores or hyphens".into(),
			);
		}
		Ok(Self(value.into_boxed_str().into_string()))
	}
	pub fn generate() -> Self {
		Self(uuid::Uuid::new_v4().to_string())
	}
	pub fn as_str(&self) -> &str {
		&self.0
	}
}
impl<'de> Deserialize<'de> for ComponentId {
	fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
		Self::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
	}
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeBehaviorComponent {
	pub id: ComponentId,
	#[serde(default = "enabled")]
	pub enabled: bool,
	pub behavior: NativeBehaviorSpec,
}
fn enabled() -> bool {
	true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NativeBehaviorSpec {
	StateMachine {
		definition: crate::state_machine::MachineDef,
	},
	KeyDoor {
		actor: EntityKey,
		key_trigger: EntityKey,
		door: EntityKey,
		#[serde(default)]
		key_visual: Option<EntityKey>,
	},
	Collectible {
		actor: EntityKey,
		trigger: EntityKey,
		#[serde(default)]
		visual: Option<EntityKey>,
		#[serde(default)]
		despawn_owner: bool,
	},
	TimedSpawner {
		prefab_path: String,
		interval_ticks: u32,
		max_alive: u32,
		lifetime_ticks: u32,
		spawn_offset: [f32; 3],
		#[serde(default)]
		bindings: BTreeMap<String, EntityKey>,
	},
}
impl NativeBehaviorSpec {
	pub fn entity_references(&self) -> Vec<&EntityKey> {
		match self {
			Self::StateMachine { definition } => definition.entity_references(),
			Self::KeyDoor {
				actor,
				key_trigger,
				door,
				key_visual,
			} => std::iter::once(actor)
				.chain(std::iter::once(key_trigger))
				.chain(std::iter::once(door))
				.chain(key_visual.iter())
				.collect(),
			Self::Collectible {
				actor,
				trigger,
				visual,
				..
			} => std::iter::once(actor)
				.chain(std::iter::once(trigger))
				.chain(visual.iter())
				.collect(),
			Self::TimedSpawner { bindings, .. } => bindings.values().collect(),
		}
	}
	pub fn validate(&self) -> Result<(), String> {
		if let Self::StateMachine { definition } = self {
			// Validate shape before allocating the bounded reference traversal.
			return definition.validate();
		}
		for key in self.entity_references() {
			validate_reference(key)?;
		}
		match self {
			Self::StateMachine { definition } => definition.validate(),
			Self::KeyDoor {
				actor,
				key_trigger,
				door,
				..
			} if actor == key_trigger || actor == door || key_trigger == door => {
				Err("Key/door actor, trigger and door must be distinct entities".into())
			}
			Self::Collectible { actor, trigger, .. } if actor == trigger => {
				Err("Collectible actor and trigger must be distinct entities".into())
			}
			Self::TimedSpawner {
				prefab_path,
				interval_ticks,
				max_alive,
				lifetime_ticks,
				spawn_offset,
				bindings,
			} => {
				validate_model_path(prefab_path)?;
				if !prefab_path.to_ascii_lowercase().ends_with(".json")
					|| !(1..=1_000_000).contains(interval_ticks)
					|| !(1..=128).contains(max_alive)
					|| !(1..=1_000_000).contains(lifetime_ticks)
					|| !spawn_offset
						.iter()
						.all(|value| value.is_finite() && value.abs() <= 1_000_000.)
				{
					return Err("Spawner requires a relative JSON prefab, interval/lifetime 1–1000000 ticks, max_alive 1–128 and finite bounded offset".into());
				}
				if bindings.len() > 32
					|| bindings.keys().any(|name| {
						name.is_empty()
							|| name.len() > 64 || !name
							.bytes()
							.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
					}) {
					return Err("Spawner bindings require at most 32 named reference slots with bounded ASCII names".into());
				}
				Ok(())
			}
			_ => Ok(()),
		}
	}
	pub fn remap_entity_references(&mut self, remap: &HashMap<EntityKey, EntityKey>) {
		fn one(key: &mut EntityKey, remap: &HashMap<EntityKey, EntityKey>) {
			if let Some(next) = remap.get(key) {
				*key = next.clone();
			}
		}
		match self {
			Self::StateMachine { definition } => definition.remap(remap),
			Self::KeyDoor {
				actor,
				key_trigger,
				door,
				key_visual,
			} => {
				one(actor, remap);
				one(key_trigger, remap);
				one(door, remap);
				if let Some(key) = key_visual {
					one(key, remap);
				}
			}
			Self::Collectible {
				actor,
				trigger,
				visual,
				..
			} => {
				one(actor, remap);
				one(trigger, remap);
				if let Some(key) = visual {
					one(key, remap);
				}
			}
			Self::TimedSpawner { bindings, .. } => {
				for key in bindings.values_mut() {
					one(key, remap);
				}
			}
		}
	}
}
fn validate_reference(key: &EntityKey) -> Result<(), String> {
	key.validate()?;
	if key.as_str().len() > 256 || key.as_str().chars().any(char::is_control) {
		return Err("Component entity reference exceeds its key budget".into());
	}
	Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ComponentData {
	CharacterController {
		settings: crate::character_controller::CharacterControllerSettings,
	},
	Animation {
		settings: crate::animation::AnimationSettings,
	},
	MeshRenderer {
		model_path: String,
		#[serde(default)]
		asset_id: Option<AssetId>,
		#[serde(default)]
		mesh_index: Option<usize>,
		#[serde(default)]
		material: Option<MaterialOverrideData>,
	},
	Collider {
		collider: ColliderData,
	},
	NativeBehavior {
		behavior: NativeBehaviorSpec,
	},
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComponentRecord {
	pub id: ComponentId,
	pub schema_version: u32,
	pub enabled: bool,
	pub data: ComponentData,
}
impl ComponentRecord {
	pub fn type_id(&self) -> &'static str {
		match self.data {
			ComponentData::CharacterController { .. } => CHARACTER_TYPE,
			ComponentData::Animation { .. } => ANIMATION_TYPE,
			ComponentData::MeshRenderer { .. } => MESH_TYPE,
			ComponentData::Collider { .. } => COLLIDER_TYPE,
			ComponentData::NativeBehavior { .. } => NATIVE_TYPE,
		}
	}
	pub fn validate(&self) -> Result<(), String> {
		ComponentId::new(self.id.as_str())?;
		if self.schema_version != 1 {
			return Err("Unsupported component schema version".into());
		}
		match &self.data {
			ComponentData::CharacterController { settings } => settings.validate(),
			ComponentData::Animation { settings } => settings.validate(),
			ComponentData::MeshRenderer {
				model_path,
				mesh_index,
				material,
				..
			} => {
				validate_model_path(model_path)?;
				if model_path == NO_MESH_PATH || mesh_index.is_some_and(|index| index > 1_000_000) {
					return Err("Invalid mesh component asset or mesh index".into());
				}
				if material.as_ref().is_some_and(|material| {
					!material
						.base_color_factor
						.iter()
						.all(|value| value.is_finite() && (0.0..=1.0).contains(value))
				}) {
					return Err("Mesh material requires finite 0–1 color components".into());
				}
				Ok(())
			}
			ComponentData::Collider { collider } => collider.validate(),
			ComponentData::NativeBehavior { behavior } => behavior.validate(),
		}
	}
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComponentTypeDescriptor {
	pub type_id: String,
	pub kind: String,
	pub label: String,
	pub description: String,
	pub max_per_object: usize,
}
pub fn descriptors() -> Vec<ComponentTypeDescriptor> {
	[
		(
			CHARACTER_TYPE,
			"character_controller",
			"Character Controller",
			"Grounded motion for an upright scene-root box body.",
			1,
		),
		(
			ANIMATION_TYPE,
			"animation",
			"Animation",
			"Deterministic native animation playback.",
			1,
		),
		(
			MESH_TYPE,
			"mesh_renderer",
			"Mesh Renderer",
			"Shared model, material and visible state.",
			1,
		),
		(
			COLLIDER_TYPE,
			"collider",
			"Collider",
			"Authored shape, sensor/filter data and enabled state.",
			1,
		),
		(
			NATIVE_TYPE,
			"native_behavior",
			"Native Behavior",
			"Independent native gameplay state for this attachment.",
			MAX_NATIVE_BEHAVIORS_PER_OBJECT,
		),
	]
	.into_iter()
	.map(
		|(type_id, kind, label, description, max_per_object)| ComponentTypeDescriptor {
			type_id: type_id.into(),
			kind: kind.into(),
			label: label.into(),
			description: description.into(),
			max_per_object,
		},
	)
	.collect()
}

/// Stable semantic addresses for flat linked-prefab overrides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PropertyId {
	CharacterController,
	Animation,
	Name,
	TransformPosition,
	TransformRotation,
	TransformScale,
	MeshModelPath,
	MeshAssetId,
	MeshIndex,
	MeshMaterial,
	Collider,
	ComponentEnabled,
	NativeBehavior,
}

impl EntityData {
	pub fn component_retained_bytes(&self) -> usize {
		let mut bytes = self.component_ids.len().saturating_mul(128);
		if let Some(component) = &self.character_controller {
			bytes = bytes
				.saturating_add(component.id.as_str().len())
				.saturating_add(component.settings.jump_action.capacity());
		}
		if let Some(component) = &self.animation {
			bytes = bytes
				.saturating_add(component.id.as_str().len())
				.saturating_add(component.settings.retained_bytes());
		}
		for (kind, id) in &self.component_ids {
			bytes = bytes
				.saturating_add(kind.capacity())
				.saturating_add(id.as_str().len());
		}
		bytes = bytes.saturating_add(
			self.native_behaviors
				.capacity()
				.saturating_mul(std::mem::size_of::<NativeBehaviorComponent>()),
		);
		for component in &self.native_behaviors {
			bytes = bytes.saturating_add(component.id.as_str().len());
			if let NativeBehaviorSpec::StateMachine { definition } = &component.behavior {
				bytes = bytes.saturating_add(definition.retained_bytes());
			}
			for key in component.behavior.entity_references() {
				bytes = bytes.saturating_add(key.as_str().len());
			}
			if let NativeBehaviorSpec::TimedSpawner {
				prefab_path,
				bindings,
				..
			} = &component.behavior
			{
				bytes = bytes
					.saturating_add(prefab_path.capacity())
					.saturating_add(bindings.len().saturating_mul(128));
				for name in bindings.keys() {
					bytes = bytes.saturating_add(name.capacity());
				}
			}
		}
		bytes
	}
	pub fn has_mesh_component(&self) -> bool {
		self.model_path != NO_MESH_PATH
	}
	pub fn component_id(&self, type_id: &str) -> ComponentId {
		self.component_ids.get(type_id).cloned().unwrap_or_else(|| {
			ComponentId::new(if type_id == MESH_TYPE {
				"mesh"
			} else {
				"collider"
			})
			.expect("fixed migrated component ID")
		})
	}
	pub fn component_records(&self) -> Vec<ComponentRecord> {
		let mut records = Vec::with_capacity(2 + self.native_behaviors.len());
		if let Some(component) = &self.character_controller {
			records.push(ComponentRecord {
				id: component.id.clone(),
				schema_version: 1,
				enabled: component.enabled,
				data: ComponentData::CharacterController {
					settings: component.settings.clone(),
				},
			});
		}
		if let Some(component) = &self.animation {
			records.push(ComponentRecord {
				id: component.id.clone(),
				schema_version: 1,
				enabled: component.enabled,
				data: ComponentData::Animation {
					settings: component.settings.clone(),
				},
			});
		}
		if self.has_mesh_component() {
			records.push(ComponentRecord {
				id: self.component_id(MESH_TYPE),
				schema_version: 1,
				enabled: self.renderable,
				data: ComponentData::MeshRenderer {
					model_path: self.model_path.clone(),
					asset_id: self.asset_id.clone(),
					mesh_index: self.mesh_index,
					material: self.material.clone(),
				},
			});
		}
		if let Some(collider) = &self.collider {
			records.push(ComponentRecord {
				id: self.component_id(COLLIDER_TYPE),
				schema_version: 1,
				enabled: self.collision_enabled,
				data: ComponentData::Collider {
					collider: collider.clone(),
				},
			});
		}
		records.extend(
			self.native_behaviors
				.iter()
				.map(|component| ComponentRecord {
					id: component.id.clone(),
					schema_version: 1,
					enabled: component.enabled,
					data: ComponentData::NativeBehavior {
						behavior: component.behavior.clone(),
					},
				}),
		);
		records
	}
	pub fn validate_components(&self) -> Result<(), String> {
		if self.native_behaviors.len() > MAX_NATIVE_BEHAVIORS_PER_OBJECT
			|| self.component_records().len() > MAX_COMPONENTS_PER_OBJECT
		{
			return Err("Object component count exceeds its limit".into());
		}
		for (type_id, id) in &self.component_ids {
			if (type_id != MESH_TYPE && type_id != COLLIDER_TYPE)
				|| (type_id == MESH_TYPE && !self.has_mesh_component())
				|| (type_id == COLLIDER_TYPE && self.collider.is_none())
			{
				return Err(
					"Component identity metadata refers to an absent or unsupported singleton"
						.into(),
				);
			}
			ComponentId::new(id.as_str())?;
		}
		let mut ids = HashSet::new();
		for component in self.component_records() {
			component.validate()?;
			if !ids.insert(component.id) {
				return Err("Component IDs must be unique within an object".into());
			}
		}
		Ok(())
	}
	pub fn put_component(
		&mut self,
		component: ComponentRecord,
		replace: bool,
	) -> Result<(), String> {
		component.validate()?;
		let records = self.component_records();
		let existing = records.iter().find(|old| old.id == component.id);
		if replace {
			if existing.is_none_or(|old| old.type_id() != component.type_id()) {
				return Err(
					"Replace requires an existing component of the same type and ID".into(),
				);
			}
		} else if existing.is_some() {
			return Err("Component ID is already attached".into());
		}
		let type_id = component.type_id();
		if type_id != NATIVE_TYPE && !replace && records.iter().any(|old| old.type_id() == type_id)
		{
			return Err("This component type permits one attachment per object".into());
		}
		match component.data {
			ComponentData::CharacterController { settings } => {
				self.character_controller =
					Some(crate::character_controller::CharacterControllerComponent {
						id: component.id,
						enabled: component.enabled,
						settings,
					})
			}
			ComponentData::Animation { settings } => {
				self.animation = Some(crate::animation::AnimationComponent {
					id: component.id,
					enabled: component.enabled,
					settings,
				})
			}
			ComponentData::MeshRenderer {
				model_path,
				asset_id,
				mesh_index,
				material,
			} => {
				self.model_path = model_path;
				self.asset_id = asset_id;
				self.mesh_index = mesh_index;
				self.material = material;
				self.renderable = component.enabled;
				self.component_ids.insert(MESH_TYPE.into(), component.id);
			}
			ComponentData::Collider { collider } => {
				self.collider = Some(collider);
				self.collision_enabled = component.enabled;
				self.component_ids
					.insert(COLLIDER_TYPE.into(), component.id);
			}
			ComponentData::NativeBehavior { behavior } => {
				let attachment = NativeBehaviorComponent {
					id: component.id,
					enabled: component.enabled,
					behavior,
				};
				if let Some(old) = self
					.native_behaviors
					.iter_mut()
					.find(|old| old.id == attachment.id)
				{
					*old = attachment;
				} else {
					self.native_behaviors.push(attachment);
				}
			}
		}
		self.validate_components()
	}
	pub fn remove_component(&mut self, id: &ComponentId) -> Result<(), String> {
		let record = self
			.component_records()
			.into_iter()
			.find(|record| &record.id == id)
			.ok_or("Component does not exist")?;
		match record.data {
			ComponentData::CharacterController { .. } => self.character_controller = None,
			ComponentData::Animation { .. } => self.animation = None,
			ComponentData::MeshRenderer { .. } => {
				self.model_path = NO_MESH_PATH.into();
				self.asset_id = None;
				self.mesh_index = None;
				self.material = None;
				self.renderable = false;
				self.component_ids.remove(MESH_TYPE);
			}
			ComponentData::Collider { .. } => {
				self.collider = None;
				self.collision_enabled = true;
				self.component_ids.remove(COLLIDER_TYPE);
			}
			ComponentData::NativeBehavior { .. } => self
				.native_behaviors
				.retain(|component| &component.id != id),
		}
		Ok(())
	}
	pub fn set_component_enabled(&mut self, id: &ComponentId, enabled: bool) -> Result<(), String> {
		let mut record = self
			.component_records()
			.into_iter()
			.find(|record| &record.id == id)
			.ok_or("Component does not exist")?;
		record.enabled = enabled;
		self.put_component(record, true)
	}
	pub fn remap_component_references(
		&mut self,
		remap: &HashMap<EntityKey, EntityKey>,
	) -> Result<(), String> {
		for component in &mut self.native_behaviors {
			component.behavior.remap_entity_references(remap);
		}
		self.validate_components()
	}
}
pub fn validate_scene_components(scene: &SceneData) -> Result<(), String> {
	validate_scene_components_with_slots(scene, &HashSet::new())
}
/// Prefab validation may defer capabilities of explicitly declared external
/// reference slots. Installed scenes always use the strict entry point above.
pub fn validate_scene_components_with_slots(
	scene: &SceneData,
	slots: &HashSet<EntityKey>,
) -> Result<(), String> {
	let keys: HashSet<_> = scene.entities.iter().map(|entity| &entity.key).collect();
	let entities: HashMap<_, _> = scene
		.entities
		.iter()
		.map(|entity| (entity.key.as_str(), entity))
		.collect();
	let mut active = 0usize;
	for entity in &scene.entities {
		entity.validate_components()?;
		for component in &entity.native_behaviors {
			for target in component.behavior.entity_references() {
				if !keys.contains(target) {
					return Err(format!(
						"Native component {} on {} targets missing entity {}",
						component.id.as_str(),
						entity.key.as_str(),
						target.as_str()
					));
				}
			}
			if let NativeBehaviorSpec::StateMachine { definition } = &component.behavior {
				definition.validate_scene(&entities, slots)?;
			}
			if !component.enabled {
				continue;
			}
			active += 1;
			if active > MAX_ACTIVE_NATIVE_BEHAVIORS {
				return Err("Scene exceeds 4096 active native behavior attachments".into());
			}
			let result = (|| {
				match &component.behavior {
					NativeBehaviorSpec::KeyDoor {
						actor,
						key_trigger,
						door,
						key_visual,
					} => {
						let actor = required_collider(&entities, slots, actor, "Key/door actor")?;
						let sensor =
							required_collider(&entities, slots, key_trigger, "Key/door trigger")?;
						if sensor.is_some_and(|collider| !collider.interaction().sensor) {
							return Err("Key/door trigger requires a Sensor collider".into());
						}
						if required_collider(&entities, slots, door, "Door")?
							.is_some_and(|collider| collider.interaction().sensor)
						{
							return Err("Door requires a blocking collider".into());
						}
						if let (Some(actor), Some(sensor)) = (actor, sensor)
							&& !actor.interaction().detects(sensor.interaction())
						{
							return Err("Key/door actor and trigger collision filters must accept each other".into());
						}
						if let Some(visual) = key_visual {
							required_visual(&entities, slots, visual)?;
						}
					}
					NativeBehaviorSpec::Collectible {
						actor,
						trigger,
						visual,
						..
					} => {
						let actor =
							required_collider(&entities, slots, actor, "Collectible actor")?;
						let sensor =
							required_collider(&entities, slots, trigger, "Collectible trigger")?;
						if sensor.is_some_and(|collider| !collider.interaction().sensor) {
							return Err("Collectible trigger requires a Sensor collider".into());
						}
						if let (Some(actor), Some(sensor)) = (actor, sensor)
							&& !actor.interaction().detects(sensor.interaction())
						{
							return Err("Collectible actor and trigger collision filters must accept each other".into());
						}
						if let Some(visual) = visual {
							required_visual(&entities, slots, visual)?;
						}
					}
					NativeBehaviorSpec::TimedSpawner { .. }
					| NativeBehaviorSpec::StateMachine { .. } => {}
				}
				Ok::<(), String>(())
			})();
			result.map_err(|error| {
				format!(
					"Native component {} on {}: {error}",
					component.id.as_str(),
					entity.key.as_str()
				)
			})?;
		}
	}
	Ok(())
}
fn required_collider<'a>(
	entities: &HashMap<&str, &'a EntityData>,
	slots: &HashSet<EntityKey>,
	key: &EntityKey,
	label: &str,
) -> Result<Option<&'a ColliderData>, String> {
	if slots.contains(key) {
		return Ok(None);
	}
	let entity = entities
		.get(key.as_str())
		.ok_or("Native behavior entity reference is missing")?;
	entity
		.collider
		.as_ref()
		.map(Some)
		.ok_or_else(|| format!("{label} requires a collider component"))
}
fn required_visual(
	entities: &HashMap<&str, &EntityData>,
	slots: &HashSet<EntityKey>,
	key: &EntityKey,
) -> Result<(), String> {
	if slots.contains(key) {
		return Ok(());
	}
	if entities
		.get(key.as_str())
		.is_some_and(|entity| entity.has_mesh_component())
	{
		Ok(())
	} else {
		Err("Native behavior visual requires a mesh renderer component".into())
	}
}

/// Singleton metadata is stored only when it differs from deterministic legacy IDs.
pub fn singleton_ids(records: &[ComponentRecord]) -> BTreeMap<String, ComponentId> {
	records
		.iter()
		.filter(|record| matches!(record.type_id(), MESH_TYPE | COLLIDER_TYPE))
		.filter(|record| {
			record.id.as_str()
				!= if record.type_id() == MESH_TYPE {
					"mesh"
				} else {
					"collider"
				}
		})
		.map(|record| (record.type_id().into(), record.id.clone()))
		.collect()
}
