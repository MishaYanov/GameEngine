//! Flat, pinned authoring links. Runtime scenes consume their materialized
//! entities; source files are read only by explicit host authoring operations.
mod constraints;
mod overrides;

use crate::{
	EntityKey,
	asset_catalog::AssetId,
	components::{ComponentId, ComponentRecord},
	editor::PrefabData,
	scene_data::{EntityData, SceneData, validate_model_path},
};
pub use constraints::validate_scene_links;
pub(crate) use overrides::{apply_value, properties, value};
pub use overrides::{capture_overrides, override_status};
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
	CharacterController,
	Animation,
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
