mod export;
mod instantiate;
use super::EditorSession;
use crate::{
	EntityKey,
	asset_catalog::AssetId,
	linked_prefab::{
		self, LinkedPrefabProperty, LinkedPrefabSource, LinkedPrefabValue, NestedPlacement,
		PinnedAssetVersion, PinnedPropSource, PlacementId, PropAddress, PropComposition,
	},
	scene_data::{CURRENT_SCHEMA_VERSION, CameraData, EntityData, SceneData, TransformData},
};
use serde::{Deserialize, Serialize};
use std::{
	borrow::Cow,
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
	#[serde(default, skip_serializing_if = "PropComposition::is_empty")]
	pub composition: PropComposition,
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
	pub fn validate_linked_player_components(&self) -> Result<(), String> {
		if self
			.entities
			.iter()
			.chain(self.composition.pins.iter().flat_map(|pin| &pin.entities))
			.any(|entity| entity.player_controller.is_some())
		{
			return Err("Linked Props cannot contain a Player Controller; keep the Player on a scene root and link its visual child".into());
		}
		Ok(())
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
			#[serde(skip_serializing_if = "PropComposition::is_empty")]
			composition: &'a PropComposition,
		}
		Stored {
			schema_version: if self.composition.is_empty() { 3 } else { 4 },
			entities: &self.entities,
			gameplay: &self.gameplay,
			reference_slots: &self.reference_slots,
			composition: &self.composition,
		}
		.serialize(serializer)
	}

	fn validated(self) -> Result<Self, String> {
		if self.schema_version == 4 {
			return self.validated_composition();
		}
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
						|| e.player_controller.is_some()
				})) {
			return Err(
				"A version-1/2/3 prefab requires 1–4096 entities; gameplay requires version 2 and reference slots version 3".into(),
			);
		}
		if !self.composition.is_empty() {
			return Err("Nested Prop composition requires prefab schema 4".into());
		}
		if self
			.entities
			.iter()
			.any(|entity| entity.linked_prefab.is_some())
		{
			return Err("Prefab sources cannot contain linked instances; export a resolved copy or Unpack them first".into());
		}
		if self.entities.iter().any(EntityData::is_local_player) {
			return Err(
				"A Prop source cannot own scene-local input; assign a Player in the scene".into(),
			);
		}
		self.validate_size()?;
		let camera = crate::Camera::default();
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
		for entity in &entities[..original_count] {
			for component in &entity.scripts {
				use crate::components::ScriptReference;
				for reference in component.settings.references.values() {
					let valid = match reference {
						ScriptReference::Entity { entity } => keys.contains(entity),
						ScriptReference::Slot { slot } => self.reference_slots.contains_key(slot),
					};
					if !valid {
						return Err(format!(
							"Script {} on {} requires a local object or a declared Prop reference slot",
							component.id.as_str(),
							entity.key.as_str()
						));
					}
				}
			}
		}
		let mut validation = SceneData {
			schema_version: CURRENT_SCHEMA_VERSION,
			organization: Default::default(),
			default_game_camera: None,
			environment: Default::default(),
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
			composition: PropComposition::default(),
		})
	}

	fn validated_composition(mut self) -> Result<Self, String> {
		if self.gameplay.is_some() {
			return Err(
				"Nested Props support component sources with no project gameplay payload".into(),
			);
		}
		if self.composition.is_empty() {
			self.schema_version = 3;
			return self.validated();
		}
		self.composition
			.validate(None, &self.entities, &self.reference_slots)?;
		self.validate_size()?;
		self.entities = Self::validated_frame(self.entities, &self.reference_slots)?;
		for pin in &mut self.composition.pins {
			pin.entities =
				Self::validated_frame(std::mem::take(&mut pin.entities), &pin.reference_slots)
					.map_err(|error| format!("Pinned Prop {:?}: {error}", pin.path))?;
		}
		let expanded = self.expanded_entities()?.into_owned();
		Self::validated_frame(expanded, &self.reference_slots)?;
		Ok(self)
	}

	fn validated_frame(
		entities: Vec<EntityData>,
		slots: &BTreeMap<String, EntityKey>,
	) -> Result<Vec<EntityData>, String> {
		Self {
			schema_version: 3,
			entities,
			gameplay: None,
			reference_slots: slots.clone(),
			composition: PropComposition::default(),
		}
		.validated()
		.map(|frame| frame.entities)
	}

	fn validate_size(&self) -> Result<(), String> {
		// Check the owned DTO without allocating a second JSON document.
		let result = if self.composition.is_empty() {
			self.serialize_canonical(&mut serde_json::Serializer::new(SizeBudget(0)))
		} else {
			self.serialize_canonical(&mut serde_json::Serializer::pretty(SizeBudget(0)))
		};
		result.map_err(|error| format!("Prefab size validation failed: {error}"))
	}

	pub(crate) fn serialized_entity_bytes(entities: &[EntityData]) -> Result<usize, String> {
		let mut budget = SizeBudget(0);
		crate::scene_format::serialize_entities(
			entities,
			&mut serde_json::Serializer::new(&mut budget),
		)
		.map_err(|error| format!("Expanded Prop exceeds its serialized entity budget: {error}"))?;
		Ok(budget.0)
	}

	/// Adopt one reviewed component-source capture into an independent draft.
	/// Every reachable asset/version has one immutable pin in the resulting table.
	pub fn with_nested_placement(
		&self,
		owner: &AssetId,
		reviewed: &LinkedPrefabSource,
		placement: NestedPlacement,
	) -> Result<Self, String> {
		self.validate()?;
		reviewed.validate()?;
		if self.gameplay.is_some() {
			return Err("Nested placements require an owning component Prop source".into());
		}
		let version = Self::reviewed_version(reviewed);
		if placement.target != version {
			return Err("Nested placement does not match the reviewed source version".into());
		}
		let mut next = self.clone();
		next.composition.placements.push(placement);
		next.adopt_reviewed_pins(reviewed, version)?;
		next.validate_nested_draft(owner)
	}

	/// Refresh one root placement to reviewed bytes of the same child asset.
	/// Other placements keep the exact captured versions they already reference.
	pub fn with_refreshed_nested_placement(
		&self,
		owner: &AssetId,
		placement: &PlacementId,
		reviewed: &LinkedPrefabSource,
	) -> Result<Self, String> {
		self.validate()?;
		let index = self
			.composition
			.placements
			.iter()
			.position(|candidate| &candidate.id == placement)
			.ok_or("Nested Prop placement no longer exists in this source")?;
		if self.composition.placements[index].target.asset_id != reviewed.source_id {
			return Err("Refresh must use the same nested Prop asset identity".into());
		}
		reviewed.validate()?;
		let version = Self::reviewed_version(reviewed);
		let mut next = self.clone();
		next.composition.placements[index].target = version.clone();
		next.adopt_reviewed_pins(reviewed, version)?;
		next.composition.retain_reachable_pins();
		next.validate_nested_draft(owner)
	}

	/// Stage the reviewed value in the owning Prop draft. The caller saves
	/// that draft and reviews scene pin changes as separate explicit steps.
	pub fn with_applied_prop_override(
		&self,
		owner: &AssetId,
		address: &PropAddress,
		property: &LinkedPrefabProperty,
		expected: &LinkedPrefabValue,
		new_value: LinkedPrefabValue,
	) -> Result<Self, String> {
		self.validate()?;
		if !linked_prefab::supports_source_override(address, property) {
			return Err("The selected Prop draft does not own this editable property".into());
		}
		let mut next = self.clone();
		match address {
			PropAddress::Object { source_entity, .. } => {
				let object = next
					.entities
					.iter_mut()
					.find(|object| &object.key == source_entity)
					.ok_or("The override object no longer exists in the owning Prop draft")?;
				if &linked_prefab::value(object, property) != expected {
					return Err(
						"The owning Prop draft changed this property; review the override again"
							.into(),
					);
				}
				linked_prefab::apply_value(object, property, new_value)?;
			}
			PropAddress::Placement { placement_path } => {
				let [id] = placement_path.as_slice() else {
					return Err("Select the draft that directly owns this placement".into());
				};
				let placement = next
					.composition
					.placements
					.iter_mut()
					.find(|candidate| &candidate.id == id)
					.ok_or("The override placement no longer exists in the owning Prop draft")?;
				let position = match property {
					LinkedPrefabProperty::TransformPosition => &mut placement.local.position,
					LinkedPrefabProperty::TransformRotation => &mut placement.local.rotation,
					LinkedPrefabProperty::TransformScale => &mut placement.local.scale,
					_ => {
						return Err(
							"The owning Prop draft supports only placement transforms".into()
						);
					}
				};
				if LinkedPrefabValue::Vector(*position) != *expected {
					return Err(
						"The owning Prop draft changed this property; review the override again"
							.into(),
					);
				}
				let LinkedPrefabValue::Vector(vector) = new_value else {
					return Err("Placement transform overrides require a vector".into());
				};
				*position = vector;
			}
		}
		if next.composition.is_empty() {
			next.validated()
		} else {
			next.validate_nested_draft(owner)
		}
	}

	fn reviewed_version(source: &LinkedPrefabSource) -> PinnedAssetVersion {
		PinnedAssetVersion {
			asset_id: source.source_id.clone(),
			fingerprint: source.fingerprint.clone(),
		}
	}

	fn adopt_reviewed_pins(
		&mut self,
		reviewed: &LinkedPrefabSource,
		version: PinnedAssetVersion,
	) -> Result<(), String> {
		let child = PinnedPropSource {
			version,
			path: reviewed.path.clone(),
			entities: reviewed.prefab.entities.clone(),
			reference_slots: reviewed.prefab.reference_slots.clone(),
			placements: reviewed.prefab.composition.placements.clone(),
		};
		for pin in std::iter::once(child).chain(reviewed.prefab.composition.pins.iter().cloned()) {
			if let Some(saved) = self
				.composition
				.pins
				.iter()
				.find(|saved| saved.version == pin.version)
			{
				// A project-relative path can move while the identity and captured
				// authored bytes are unchanged. Retain the previously captured path.
				if saved.entities != pin.entities
					|| saved.reference_slots != pin.reference_slots
					|| saved.placements != pin.placements
				{
					return Err(format!(
						"Conflicting captured contents for pinned Prop {:?}",
						pin.path,
					));
				}
			} else {
				self.composition.pins.push(pin);
			}
		}
		Ok(())
	}

	fn validate_nested_draft(mut self, owner: &AssetId) -> Result<Self, String> {
		self.schema_version = 4;
		self.composition
			.validate(Some(owner), &self.entities, &self.reference_slots)?;
		self.validated()
	}

	/// Materialize one portable hierarchy. Nested members use stable source
	/// addresses and external slot placeholders remain available for placement.
	pub fn expanded_entities(&self) -> Result<Cow<'_, [EntityData]>, String> {
		if self.composition.is_empty() {
			return Ok(Cow::Borrowed(&self.entities));
		}
		self.expanded_composition(None)
			.map(|result| Cow::Owned(result.entities))
	}

	pub fn expanded_composition(
		&self,
		owner: Option<&AssetId>,
	) -> Result<crate::linked_prefab::ComposedProp, String> {
		self.composition.flatten(
			owner,
			&self.entities,
			&self.reference_slots,
			PropAddress::materialized_key,
		)
	}

	/// Pure ownership remapping for editor copies, linked instances and runtime
	/// spawn candidates. Component IDs remain stable within their remapped owner.
	pub fn remap_entities(
		&self,
		keys: &HashMap<EntityKey, EntityKey>,
		bindings: &BTreeMap<String, EntityKey>,
	) -> Result<Vec<EntityData>, String> {
		self.validate()?;
		let sources = self.expanded_entities()?;
		Self::remap_expanded(&sources, &self.reference_slots, keys, bindings)
	}

	pub(crate) fn remap_expanded(
		sources: &[EntityData],
		reference_slots: &BTreeMap<String, EntityKey>,
		keys: &HashMap<EntityKey, EntityKey>,
		bindings: &BTreeMap<String, EntityKey>,
	) -> Result<Vec<EntityData>, String> {
		if bindings.len() != reference_slots.len()
			|| bindings
				.keys()
				.any(|name| !reference_slots.contains_key(name))
		{
			return Err(
				"Supply exactly one entity binding for every declared prefab reference slot".into(),
			);
		}
		let mut remap = keys.clone();
		let mut unique = HashSet::new();
		for entity in sources {
			let key = keys
				.get(&entity.key)
				.ok_or("Prefab entity mapping is incomplete")?;
			key.validate()?;
			if !unique.insert(key.clone()) {
				return Err("Prefab mapped entity keys must be distinct".into());
			}
		}
		for (name, placeholder) in reference_slots {
			let target = bindings
				.get(name)
				.ok_or("A prefab reference slot is unbound")?;
			target.validate()?;
			if unique.contains(target) {
				return Err("Prefab reference slots must bind an existing external entity".into());
			}
			remap.insert(placeholder.clone(), target.clone());
		}
		sources
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
				for component in &mut entity.scripts {
					component.settings.bind_slots(bindings)?;
				}
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
		self.schema_version = if self.composition.is_empty() { 3 } else { 4 };
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
