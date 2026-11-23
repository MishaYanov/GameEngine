//! Versioned nested Prop placements and the pinned source graph that they reference.

use crate::{
	EntityKey,
	asset_catalog::AssetId,
	components::{ComponentId, NO_MESH_PATH},
	editor::PrefabData,
	scene_data::{EntityData, TransformData, validate_model_path},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};

pub const MAX_PROP_NESTING: usize = 8;
pub const MAX_NESTED_PLACEMENTS: usize = 128;
pub const MAX_PINNED_PROP_VERSIONS: usize = 128;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PlacementId(ComponentId);

impl PlacementId {
	pub fn new(value: impl Into<String>) -> Result<Self, String> {
		ComponentId::new(value).map(Self)
	}

	pub fn generate() -> Self {
		Self(ComponentId::generate())
	}

	pub fn as_str(&self) -> &str {
		self.0.as_str()
	}
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PinnedAssetVersion {
	pub asset_id: AssetId,
	pub fingerprint: String,
}

impl PinnedAssetVersion {
	fn validate(&self) -> Result<(), String> {
		if self.fingerprint.len() != 64
			|| !self
				.fingerprint
				.bytes()
				.all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
		{
			return Err("A pinned Prop version requires a lowercase SHA-256 fingerprint".into());
		}
		Ok(())
	}
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NestedBinding {
	Object { key: EntityKey },
	Slot { name: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NestedPlacement {
	pub id: PlacementId,
	pub name: String,
	pub target: PinnedAssetVersion,
	pub parent: EntityKey,
	pub local: TransformData,
	#[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
	pub bindings: BTreeMap<String, NestedBinding>,
}

impl NestedPlacement {
	fn validate(&self) -> Result<(), String> {
		if self.name.trim().is_empty()
			|| self.name.len() > 256
			|| self.name.chars().any(char::is_control)
		{
			return Err(
				"A nested Prop placement requires a name of 1–256 bytes with no controls".into(),
			);
		}
		if !self
			.local
			.position
			.iter()
			.chain(self.local.rotation.iter())
			.chain(self.local.scale.iter())
			.all(|component| component.is_finite())
		{
			return Err("Nested Prop placement transforms must be finite".into());
		}
		Ok(())
	}
}

/// A captured source frame. Its nested placements refer to siblings in the same
/// pin table, so multiple placements can share one captured version.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PinnedPropSource {
	pub version: PinnedAssetVersion,
	pub path: String,
	#[serde(
		serialize_with = "crate::scene_format::serialize_entities",
		deserialize_with = "crate::scene_format::deserialize_entities"
	)]
	pub entities: Vec<EntityData>,
	#[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
	pub reference_slots: BTreeMap<String, EntityKey>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub placements: Vec<NestedPlacement>,
}

impl PinnedPropSource {
	fn frame(&self) -> PropFrame<'_> {
		PropFrame {
			version: Some(&self.version),
			entities: &self.entities,
			slots: &self.reference_slots,
			placements: &self.placements,
		}
	}
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PropComposition {
	pub placements: Vec<NestedPlacement>,
	pub pins: Vec<PinnedPropSource>,
}

impl PropComposition {
	pub fn is_empty(&self) -> bool {
		self.placements.is_empty() && self.pins.is_empty()
	}

	pub(crate) fn retain_reachable_pins(&mut self) {
		let pins: HashMap<_, _> = self.pins.iter().map(|pin| (&pin.version, pin)).collect();
		let mut pending: Vec<_> = self.placements.iter().map(|entry| &entry.target).collect();
		let mut used = HashSet::new();
		while let Some(version) = pending.pop() {
			if used.insert(version)
				&& let Some(pin) = pins.get(version)
			{
				pending.extend(pin.placements.iter().map(|entry| &entry.target));
			}
		}
		let used: HashSet<_> = used.into_iter().cloned().collect();
		self.pins.retain(|pin| used.contains(&pin.version));
	}

	/// Validate the reachable pinned graph and return the flattened entity count.
	/// The caller validates its own and pinned local entity/component documents.
	pub fn validate(
		&self,
		owner: Option<&AssetId>,
		entities: &[EntityData],
		reference_slots: &BTreeMap<String, EntityKey>,
	) -> Result<usize, String> {
		let graph = PropGraph::new(&self.pins)?;
		graph.validate(owner, self.root_frame(entities, reference_slots))
	}

	/// Expand one source with stable caller-provided keys for nested addresses.
	/// Root source entity keys stay unchanged. Source-local and external slot
	/// references are mapped while traversing each placement's address.
	pub fn flatten(
		&self,
		owner: Option<&AssetId>,
		entities: &[EntityData],
		reference_slots: &BTreeMap<String, EntityKey>,
		key_for: impl Fn(&PropAddress) -> Result<EntityKey, String>,
	) -> Result<ComposedProp, String> {
		let graph = PropGraph::new(&self.pins)?;
		let frame = self.root_frame(entities, reference_slots);
		let count = graph.validate(owner, frame)?;
		let mut expansion = PropExpansion {
			keys: reference_slots.values().cloned().collect(),
			key_for,
			result: ComposedProp {
				entities: Vec::with_capacity(count),
				addresses: BTreeMap::new(),
			},
		};
		let mut path = Vec::new();
		expansion.append_frame(&graph, frame, reference_slots, &mut path, None)?;
		Ok(expansion.result)
	}

	fn root_frame<'a>(
		&'a self,
		entities: &'a [EntityData],
		slots: &'a BTreeMap<String, EntityKey>,
	) -> PropFrame<'a> {
		PropFrame {
			version: None,
			entities,
			slots,
			placements: &self.placements,
		}
	}
}

/// A semantic address stays stable when source labels or sibling order change.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PropAddress {
	Object {
		placement_path: Vec<PlacementId>,
		source_entity: EntityKey,
	},
	Placement {
		placement_path: Vec<PlacementId>,
	},
}

impl PropAddress {
	/// A versioned, length-delimited identity for the flat source mapping. The
	/// stored scene map gives each placed instance its own unrelated scene keys.
	pub fn materialized_key(&self) -> Result<EntityKey, String> {
		let mut hash = Sha256::new();
		hash.update(b"game-engine.prop-address-v1\0");
		let path = match self {
			Self::Object { placement_path, .. } => {
				hash.update([0]);
				placement_path
			}
			Self::Placement { placement_path } => {
				hash.update([1]);
				placement_path
			}
		};
		if path.len() > MAX_PROP_NESTING {
			return Err("Prop address exceeds the nesting limit".into());
		}
		hash.update((path.len() as u32).to_le_bytes());
		for placement in path {
			let token = placement.as_str().as_bytes();
			hash.update((token.len() as u32).to_le_bytes());
			hash.update(token);
		}
		if let Self::Object { source_entity, .. } = self {
			let token = source_entity.as_str().as_bytes();
			if token.len() > 256 {
				return Err("Prop address uses an oversized source entity key".into());
			}
			hash.update((token.len() as u32).to_le_bytes());
			hash.update(token);
		}
		EntityKey::new(format!("prop-{:x}", hash.finalize()))
	}
}

#[derive(Debug)]
pub struct ComposedProp {
	pub entities: Vec<EntityData>,
	pub addresses: BTreeMap<EntityKey, PropAddress>,
}

#[derive(Clone, Copy)]
struct PropFrame<'a> {
	version: Option<&'a PinnedAssetVersion>,
	entities: &'a [EntityData],
	slots: &'a BTreeMap<String, EntityKey>,
	placements: &'a [NestedPlacement],
}

struct PropGraph<'a> {
	pins: HashMap<&'a PinnedAssetVersion, &'a PinnedPropSource>,
}

impl<'a> PropGraph<'a> {
	fn new(pins: &'a [PinnedPropSource]) -> Result<Self, String> {
		if pins.len() > MAX_PINNED_PROP_VERSIONS {
			return Err(format!(
				"A Prop supports at most {MAX_PINNED_PROP_VERSIONS} pinned source versions"
			));
		}
		let mut by_version = HashMap::with_capacity(pins.len());
		for pin in pins {
			pin.version.validate()?;
			validate_model_path(&pin.path)?;
			if !pin.path.to_ascii_lowercase().ends_with(".json") {
				return Err(
					"Pinned Prop source paths must name a project-relative JSON asset".into(),
				);
			}
			if by_version.insert(&pin.version, pin).is_some() {
				return Err("A Prop source version is pinned more than once".into());
			}
		}
		Ok(Self { pins: by_version })
	}

	fn child(&self, placement: &NestedPlacement) -> Result<&'a PinnedPropSource, String> {
		self.pins.get(&placement.target).copied().ok_or_else(|| {
			format!(
				"Nested Prop {:?} references a missing pinned source version",
				placement.name
			)
		})
	}

	fn validate(&self, owner: Option<&'a AssetId>, root: PropFrame<'a>) -> Result<usize, String> {
		let mut inspection = GraphInspection::default();
		let mut ancestry = Vec::new();
		if let Some(owner) = owner {
			ancestry.push(owner);
		}
		self.inspect_frame(root, &mut ancestry, 0, &mut inspection)?;
		if inspection.used.len() != self.pins.len() {
			return Err("The Prop pin table contains an unused source version".into());
		}
		Ok(inspection.entities)
	}

	fn inspect_frame(
		&self,
		frame: PropFrame<'a>,
		ancestry: &mut Vec<&'a AssetId>,
		depth: usize,
		inspection: &mut GraphInspection<'a>,
	) -> Result<(), String> {
		if frame.entities.is_empty()
			|| frame
				.entities
				.iter()
				.any(|entity| entity.linked_prefab.is_some())
		{
			return Err("Each Prop frame must contain local source objects and use explicit nested placements".into());
		}
		inspection.charge(frame)?;
		let object_keys: HashSet<_> = frame.entities.iter().map(|entity| &entity.key).collect();
		if object_keys.len() != frame.entities.len()
			|| frame.entities.iter().any(|entity| {
				entity
					.parent
					.as_ref()
					.is_some_and(|parent| !object_keys.contains(parent))
			}) {
			return Err(
				"A pinned Prop frame requires unique objects and local hierarchy parents".into(),
			);
		}
		let mut placement_ids = HashSet::new();
		for placement in frame.placements {
			placement.validate()?;
			if !placement_ids.insert(&placement.id) {
				return Err(format!(
					"Duplicate nested Prop placement ID {:?}",
					placement.id.as_str()
				));
			}
			if !object_keys.contains(&placement.parent) {
				return Err(format!(
					"Nested Prop {:?} requires a parent in its owning source",
					placement.name
				));
			}
			let child = self.child(placement)?;
			if ancestry.contains(&&child.version.asset_id) {
				return Err(format!(
					"Nested Prop {:?} creates a source cycle at {}",
					placement.name, child.path
				));
			}
			if depth >= MAX_PROP_NESTING {
				return Err(format!(
					"Nested Prop {:?} exceeds depth {MAX_PROP_NESTING}",
					placement.name
				));
			}
			if placement.bindings.len() != child.reference_slots.len()
				|| placement.bindings.iter().any(|(name, binding)| {
					!child.reference_slots.contains_key(name)
						|| !match binding {
							NestedBinding::Object { key } => object_keys.contains(key),
							NestedBinding::Slot { name } => frame.slots.contains_key(name),
						}
				}) {
				return Err(format!(
					"Nested Prop {:?} requires one valid local object or source slot for every child slot",
					placement.name
				));
			}
			inspection.used.insert(&child.version);
			ancestry.push(&child.version.asset_id);
			self.inspect_frame(child.frame(), ancestry, depth + 1, inspection)?;
			ancestry.pop();
		}
		Ok(())
	}
}

#[derive(Default)]
struct GraphInspection<'a> {
	entities: usize,
	entity_bytes: usize,
	frame_bytes: HashMap<Option<&'a PinnedAssetVersion>, usize>,
	placements: usize,
	used: HashSet<&'a PinnedAssetVersion>,
}

impl<'a> GraphInspection<'a> {
	fn charge(&mut self, frame: PropFrame<'a>) -> Result<(), String> {
		self.entities = self
			.entities
			.saturating_add(frame.entities.len())
			.saturating_add(frame.placements.len());
		self.placements = self.placements.saturating_add(frame.placements.len());
		if self.entities > PrefabData::MAX_ENTITIES || self.placements > MAX_NESTED_PLACEMENTS {
			return Err(format!(
				"Expanded Prop exceeds {} objects or {MAX_NESTED_PLACEMENTS} nested placements",
				PrefabData::MAX_ENTITIES
			));
		}
		// Charge each use of a frame before cloning entities into the expanded
		// document. Measure a shared pin only once.
		let bytes = match self.frame_bytes.entry(frame.version) {
			std::collections::hash_map::Entry::Occupied(entry) => *entry.get(),
			std::collections::hash_map::Entry::Vacant(entry) => {
				*entry.insert(PrefabData::serialized_entity_bytes(frame.entities)?)
			}
		};
		self.entity_bytes = self.entity_bytes.saturating_add(bytes);
		if self.entity_bytes > PrefabData::MAX_JSON_BYTES {
			return Err("Expanded Prop exceeds its 16 MiB serialized entity budget".into());
		}
		Ok(())
	}
}

struct PropExpansion<K> {
	keys: HashSet<EntityKey>,
	key_for: K,
	result: ComposedProp,
}

impl<K: Fn(&PropAddress) -> Result<EntityKey, String>> PropExpansion<K> {
	fn register(
		&mut self,
		address: PropAddress,
		root_key: Option<&EntityKey>,
	) -> Result<EntityKey, String> {
		let key = match root_key {
			Some(key) => key.clone(),
			None => (self.key_for)(&address)?,
		};
		if !self.keys.insert(key.clone()) {
			return Err("Nested Prop identities collide with an object or source slot".into());
		}
		self.result.addresses.insert(key.clone(), address);
		Ok(key)
	}

	fn append_frame(
		&mut self,
		graph: &PropGraph<'_>,
		frame: PropFrame<'_>,
		bindings: &BTreeMap<String, EntityKey>,
		path: &mut Vec<PlacementId>,
		anchor: Option<&EntityKey>,
	) -> Result<(), String> {
		let mut remap = HashMap::with_capacity(frame.entities.len() + frame.slots.len());
		for entity in frame.entities {
			let address = PropAddress::Object {
				placement_path: path.clone(),
				source_entity: entity.key.clone(),
			};
			let key = self.register(address, path.is_empty().then_some(&entity.key))?;
			remap.insert(entity.key.clone(), key);
		}
		for (name, placeholder) in frame.slots {
			let target = bindings
				.get(name)
				.ok_or("Expanded Prop frame has an unbound slot")?;
			if remap.insert(placeholder.clone(), target.clone()).is_some() {
				return Err("Prop slot placeholder collides with a local source object".into());
			}
		}
		for original in frame.entities {
			let mut entity = original.clone();
			entity.key = remap[&original.key].clone();
			entity.parent = match original.parent.as_ref() {
				Some(parent) => Some(
					remap
						.get(parent)
						.cloned()
						.ok_or("Prop object parent is missing")?,
				),
				None => anchor.cloned(),
			};
			entity.remap_component_references(&remap)?;
			for component in &mut entity.scripts {
				component.settings.bind_slots(bindings)?;
			}
			self.result.entities.push(entity);
		}
		for placement in frame.placements {
			let child = graph.child(placement)?;
			let mut nested_bindings = BTreeMap::new();
			for (name, binding) in &placement.bindings {
				let key = match binding {
					NestedBinding::Object { key } => remap.get(key),
					NestedBinding::Slot { name } => bindings.get(name),
				}
				.ok_or("Nested Prop source binding is unavailable")?;
				nested_bindings.insert(name.clone(), key.clone());
			}
			path.push(placement.id.clone());
			let anchor = self.register(
				PropAddress::Placement {
					placement_path: path.clone(),
				},
				None,
			)?;
			let parent = remap
				.get(&placement.parent)
				.cloned()
				.ok_or("Nested Prop parent is missing")?;
			self.result.entities.push(
				EntityData::new(anchor.clone(), &placement.name, NO_MESH_PATH)
					.with_parent(Some(parent))
					.with_transform(placement.local.clone())
					.with_renderable(false),
			);
			self.append_frame(graph, child.frame(), &nested_bindings, path, Some(&anchor))?;
			path.pop();
		}
		Ok(())
	}
}
