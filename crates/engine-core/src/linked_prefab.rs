//! Flat, pinned authoring links. Runtime scenes consume their materialized
//! entities; source files are read only by explicit host authoring operations.
use crate::{
	EntityKey,
	asset_catalog::AssetId,
	components::{ComponentId, ComponentRecord},
	editor::PrefabData,
	scene_data::{EntityData, SceneData, validate_model_path},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkedPrefabSource {
	pub source_id: AssetId,
	pub path: String,
	pub fingerprint: String,
	#[serde(serialize_with = "crate::editor::PrefabData::serialize_canonical")]
	pub prefab: PrefabData,
}

/// Semantic addresses never depend on array indices or Inspector labels.
/// Component payloads are initially one typed override, including their enabled
/// state; transforms retain independent position, rotation and scale overrides.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LinkedPrefabProperty {
	Name,
	TransformPosition,
	TransformRotation,
	TransformScale,
	MeshRenderer,
	Collider,
	NativeBehavior { component_id: ComponentId },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkedPrefabOverrideTarget {
	pub source_entity: EntityKey,
	pub property: LinkedPrefabProperty,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkedPrefabInstance {
	pub source: LinkedPrefabSource,
	pub entity_map: BTreeMap<EntityKey, EntityKey>,
	#[serde(default)]
	pub bindings: BTreeMap<String, EntityKey>,
	#[serde(default)]
	pub overrides: BTreeSet<LinkedPrefabOverrideTarget>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrefabConflictResolution {
	#[default]
	Reject,
	KeepLocal,
	UseSource,
}

impl LinkedPrefabSource {
	pub fn validate(&self) -> Result<(), String> {
		validate_model_path(&self.path)?;
		if !self.path.to_ascii_lowercase().ends_with(".json")
			|| self.fingerprint.len() != 64
			|| !self
				.fingerprint
				.bytes()
				.all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
		{
			return Err(
				"Linked source requires a relative JSON path and captured SHA256 fingerprint"
					.into(),
			);
		}
		if self
			.prefab
			.entities
			.iter()
			.any(|entity| entity.linked_prefab.is_some())
		{
			return Err(
				"Linked prefab sources cannot contain nested linked instances; Unpack them first"
					.into(),
			);
		}
		if self.prefab.gameplay.is_some() {
			return Err("Linked sources support entity/component assemblies. Use Instantiate Copy for legacy prefab gameplay, or explicitly create a component source after converting its gameplay".into());
		}
		self.prefab.validate()
	}
}

impl LinkedPrefabInstance {
	pub fn remap_instance(&mut self, remap: &HashMap<EntityKey, EntityKey>) -> Result<(), String> {
		for key in self.entity_map.values_mut() {
			*key = remap
				.get(key)
				.cloned()
				.ok_or("Duplicate the whole linked instance or Unpack it first")?;
		}
		for target in self.bindings.values_mut() {
			if let Some(key) = remap.get(target) {
				*target = key.clone();
			}
		}
		Ok(())
	}
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum LinkedPrefabValue {
	Name(String),
	Vector([f32; 3]),
	Component(Option<ComponentRecord>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinkedPrefabOverrideDto {
	pub target: LinkedPrefabOverrideTarget,
	pub base: LinkedPrefabValue,
	pub value: LinkedPrefabValue,
}

pub fn validate_scene_links(scene: &SceneData) -> Result<(), String> {
	let entities: HashMap<_, _> = scene
		.entities
		.iter()
		.map(|entity| (&entity.key, entity))
		.collect();
	let mut children: HashMap<&EntityKey, Vec<&EntityKey>> = HashMap::new();
	for entity in &scene.entities {
		if let Some(parent) = &entity.parent {
			children.entry(parent).or_default().push(&entity.key);
		}
	}
	let mut members = HashSet::new();
	let mut count = 0usize;
	for anchor in scene
		.entities
		.iter()
		.filter(|entity| entity.linked_prefab.is_some())
	{
		count += 1;
		if count > 128 {
			return Err("Scene exceeds 128 linked prefab instances".into());
		}
		let link = anchor.linked_prefab.as_ref().expect("filtered link");
		link.source.validate()?;
		if anchor.has_mesh_component()
			|| anchor.collider.is_some()
			|| !anchor.native_behaviors.is_empty()
		{
			return Err(
				"Linked instance anchors are transform-only objects; edit a member or Unpack first"
					.into(),
			);
		}
		let sources: HashMap<_, _> = link
			.source
			.prefab
			.entities
			.iter()
			.map(|entity| (&entity.key, entity))
			.collect();
		let source_keys: HashSet<_> = sources.keys().copied().collect();
		if source_keys.len() != link.entity_map.len()
			|| link.entity_map.keys().any(|key| !source_keys.contains(key))
		{
			return Err("Linked instance mapping differs from its pinned source".into());
		}
		let owned: HashSet<_> = link.entity_map.values().collect();
		let mut pending = children.get(&anchor.key).cloned().unwrap_or_default();
		while let Some(key) = pending.pop() {
			if !owned.contains(key) {
				return Err("Linked instance structure is pinned; Unpack before adding or duplicating individual members".into());
			}
			if let Some(nested) = children.get(key) {
				pending.extend(nested);
			}
		}
		for (source, key) in &link.entity_map {
			if key == &anchor.key || !members.insert(key.clone()) {
				return Err(
					"Linked instance members must be distinct and belong to one instance".into(),
				);
			}
			let member = entities.get(key).ok_or(
				"A linked member was removed; delete the whole instance or Unpack it first",
			)?;
			let base = sources.get(source).expect("source mapping checked");
			let parent = base
				.parent
				.as_ref()
				.and_then(|key| link.entity_map.get(key))
				.unwrap_or(&anchor.key);
			if member.parent.as_ref() != Some(parent) || member.linked_prefab.is_some() {
				return Err(
					"Linked member hierarchy is pinned; Unpack before reparenting or nesting it"
						.into(),
				);
			}
		}
		for target in &link.overrides {
			if !source_keys.contains(&target.source_entity) {
				return Err("Override targets an entity outside its pinned source".into());
			}
		}
		for target in link.bindings.values() {
			if !entities.contains_key(target) {
				return Err(
					"Linked prefab reference binding targets a missing scene entity".into(),
				);
			}
		}
		let base = materialized(link, &anchor.key)?;
		for (source, expected) in &base {
			let actual = entities[&link.entity_map[source]];
			for property in properties(&[expected, actual]) {
				let target = LinkedPrefabOverrideTarget {
					source_entity: source.clone(),
					property: property.clone(),
				};
				if value(expected, &property) != value(actual, &property)
					&& !link.overrides.contains(&target)
				{
					return Err(
						"Linked property differs from its pin without an explicit override".into(),
					);
				}
			}
		}
	}
	Ok(())
}

pub fn resolve_scene(scene: &SceneData) -> SceneData {
	let mut scene = scene.clone();
	for entity in &mut scene.entities {
		entity.linked_prefab = None;
	}
	scene
}

pub(crate) fn materialized(
	link: &LinkedPrefabInstance,
	anchor: &EntityKey,
) -> Result<BTreeMap<EntityKey, EntityData>, String> {
	let keys = link
		.entity_map
		.iter()
		.map(|(source, target)| (source.clone(), target.clone()))
		.collect();
	let entities = link.source.prefab.remap_entities(&keys, &link.bindings)?;
	Ok(link
		.source
		.prefab
		.entities
		.iter()
		.zip(entities)
		.map(|(source, mut entity)| {
			if entity.parent.is_none() {
				entity.parent = Some(anchor.clone());
			}
			(source.key.clone(), entity)
		})
		.collect())
}

pub(crate) fn properties(entities: &[&EntityData]) -> BTreeSet<LinkedPrefabProperty> {
	use LinkedPrefabProperty::*;
	let mut properties = BTreeSet::from([
		Name,
		TransformPosition,
		TransformRotation,
		TransformScale,
		MeshRenderer,
		Collider,
	]);
	for entity in entities {
		for component in &entity.native_behaviors {
			properties.insert(NativeBehavior {
				component_id: component.id.clone(),
			});
		}
	}
	properties
}

pub(crate) fn value(entity: &EntityData, property: &LinkedPrefabProperty) -> LinkedPrefabValue {
	use LinkedPrefabProperty::*;
	match property {
		Name => LinkedPrefabValue::Name(entity.name.clone()),
		TransformPosition => LinkedPrefabValue::Vector(entity.transform.position),
		TransformRotation => LinkedPrefabValue::Vector(entity.transform.rotation),
		TransformScale => LinkedPrefabValue::Vector(entity.transform.scale),
		MeshRenderer => LinkedPrefabValue::Component(
			entity
				.component_records()
				.into_iter()
				.find(|record| record.type_id() == crate::components::MESH_TYPE),
		),
		Collider => LinkedPrefabValue::Component(
			entity
				.component_records()
				.into_iter()
				.find(|record| record.type_id() == crate::components::COLLIDER_TYPE),
		),
		NativeBehavior { component_id } => {
			LinkedPrefabValue::Component(entity.component_records().into_iter().find(|record| {
				record.type_id() == crate::components::NATIVE_TYPE && &record.id == component_id
			}))
		}
	}
}

pub(crate) fn apply_value(
	entity: &mut EntityData,
	property: &LinkedPrefabProperty,
	new: LinkedPrefabValue,
) -> Result<(), String> {
	use LinkedPrefabProperty::*;
	match (property, new) {
		(Name, LinkedPrefabValue::Name(name)) => entity.name = name,
		(TransformPosition, LinkedPrefabValue::Vector(vector)) => {
			entity.transform.position = vector
		}
		(TransformRotation, LinkedPrefabValue::Vector(vector)) => {
			entity.transform.rotation = vector
		}
		(TransformScale, LinkedPrefabValue::Vector(vector)) => entity.transform.scale = vector,
		(property, LinkedPrefabValue::Component(component)) => {
			let current = match value(entity, property) {
				LinkedPrefabValue::Component(component) => component,
				_ => return Err("Override property/value types differ".into()),
			};
			if let Some(current) = current {
				entity.remove_component(&current.id)?;
			}
			if let Some(component) = component {
				let correct = match property {
					MeshRenderer => component.type_id() == crate::components::MESH_TYPE,
					Collider => component.type_id() == crate::components::COLLIDER_TYPE,
					NativeBehavior { component_id } => {
						component.type_id() == crate::components::NATIVE_TYPE
							&& &component.id == component_id
					}
					_ => false,
				};
				if !correct {
					return Err(
						"Override component type or identity differs from its property address"
							.into(),
					);
				}
				entity.put_component(component, false)?;
			}
		}
		_ => return Err("Override property/value types differ".into()),
	}
	Ok(())
}

/// Normal Inspector commands record user intent even when its new value equals
/// a future source value. Only explicit Revert clears that persisted intention.
pub fn capture_overrides(before: &SceneData, after: &mut SceneData) -> Result<(), String> {
	let previous: HashMap<_, _> = before
		.entities
		.iter()
		.map(|entity| (&entity.key, entity))
		.collect();
	let indices: HashMap<_, _> = after
		.entities
		.iter()
		.enumerate()
		.map(|(index, entity)| (entity.key.clone(), index))
		.collect();
	let anchors: Vec<_> = after
		.entities
		.iter()
		.enumerate()
		.filter_map(|(index, entity)| {
			entity
				.linked_prefab
				.as_ref()
				.map(|link| (index, entity.key.clone(), link.clone()))
		})
		.collect();
	for (anchor_index, anchor, mut link) in anchors {
		let Some(old) = previous
			.get(&anchor)
			.and_then(|entity| entity.linked_prefab.as_ref())
		else {
			continue;
		};
		// Explicit link commands own changes to their pin/masks as one history edit.
		if old != &link {
			continue;
		}
		for (source, target) in &link.entity_map {
			let (Some(before), Some(&index)) = (previous.get(target), indices.get(target)) else {
				continue;
			};
			let current = &after.entities[index];
			for property in properties(&[before, current]) {
				if value(before, &property) != value(current, &property) {
					link.overrides.insert(LinkedPrefabOverrideTarget {
						source_entity: source.clone(),
						property,
					});
				}
			}
		}
		after.entities[anchor_index].linked_prefab = Some(link);
	}
	Ok(())
}

pub fn override_status(
	scene: &SceneData,
	anchor: &EntityKey,
) -> Result<Vec<LinkedPrefabOverrideDto>, String> {
	let link = scene
		.entities
		.iter()
		.find(|entity| &entity.key == anchor)
		.and_then(|entity| entity.linked_prefab.as_ref())
		.ok_or("Select a linked instance")?;
	let base = materialized(link, anchor)?;
	let entities: HashMap<_, _> = scene
		.entities
		.iter()
		.map(|entity| (&entity.key, entity))
		.collect();
	link.overrides
		.iter()
		.map(|target| {
			let expected = base
				.get(&target.source_entity)
				.ok_or("Override source entity is missing")?;
			let actual = entities
				.get(&link.entity_map[&target.source_entity])
				.ok_or("Linked member is missing")?;
			Ok(LinkedPrefabOverrideDto {
				target: target.clone(),
				base: value(expected, &target.property),
				value: value(actual, &target.property),
			})
		})
		.collect()
}
