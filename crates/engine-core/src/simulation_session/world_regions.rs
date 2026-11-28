//! Additive transactions preserve the live world and its existing owners.
mod movement;
pub(crate) use movement::RegionMovementGuard;

use super::SimulationSession;
use crate::{
	EntityKey, SceneData,
	collision::CollisionWorld,
	project::WorldRegion,
	scene_catalog::SceneId,
	scene_data::{EntityData, TransformData},
	world::{RuntimeWorld, WorldChange},
};
use glam::Vec3;
use serde::{Deserialize, Serialize};
use std::{
	collections::{BTreeMap, HashSet},
	sync::Arc,
};

const MAX_RESIDENT_REGIONS: usize = 8;
const MAX_CHANGED_OBJECTS: usize = 4_096;
const MAX_REGION_SAVE_BYTES: usize = 1024 * 1024;
const REGION_SAVE_VERSION: u32 = 1;

pub struct PreparedRegionChange {
	source: Arc<SceneData>,
	world_id: String,
	topology_revision: u64,
	state_revision: u64,
	tick: u64,
	scene_id: SceneId,
	document: Arc<SceneData>,
	world: RuntimeWorld,
	collisions: CollisionWorld,
	change: RegionChange,
	added_start: usize,
	removed: Vec<EntityKey>,
	visible: Option<HashSet<EntityKey>>,
}

impl PreparedRegionChange {
	pub fn scene_id(&self) -> &SceneId {
		&self.scene_id
	}

	pub fn document(&self) -> &Arc<SceneData> {
		&self.document
	}

	pub fn added_entities(&self) -> &[EntityData] {
		&self.document.entities[self.added_start..]
	}

	pub fn removed_entities(&self) -> &[EntityKey] {
		&self.removed
	}

	pub fn set_visibility_capabilities(
		&mut self,
		mut available: HashSet<EntityKey>,
	) -> Result<(), String> {
		if available
			.iter()
			.any(|key| self.world.entity_index(key).is_none())
		{
			return Err("Region visibility capabilities contain a foreign entity".into());
		}
		let added: HashSet<_> = self
			.added_entities()
			.iter()
			.map(|entity| &entity.key)
			.collect();
		available.retain(|key| added.contains(key));
		self.visible = Some(available);
		Ok(())
	}
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldRegionState {
	version: u32,
	world: SceneId,
	regions: Vec<RegionDelta>,
}

impl WorldRegionState {
	pub(crate) fn is_empty(&self) -> bool {
		self.regions.is_empty()
	}

	pub fn from_json(source: &str) -> Result<Self, String> {
		if source.len() > MAX_REGION_SAVE_BYTES {
			return Err("Region state exceeds its byte limit".into());
		}
		let state: Self = serde_json::from_str(source).map_err(|error| error.to_string())?;
		state.check_budget()?;
		Ok(state)
	}

	pub fn to_json(&self) -> Result<String, String> {
		self.check_budget()?;
		let json = serde_json::to_string(self).map_err(|error| error.to_string())?;
		if json.len() > MAX_REGION_SAVE_BYTES {
			return Err("Region state exceeds its byte limit".into());
		}
		Ok(json)
	}

	fn check_budget(&self) -> Result<(), String> {
		if self.version != REGION_SAVE_VERSION {
			return Err("Unsupported region state version".into());
		}
		if self.regions.len() > 256 {
			return Err("Region state exceeds its region limit".into());
		}
		let mut count = 0;
		let mut ids = HashSet::new();
		for delta in &self.regions {
			if !ids.insert(&delta.scene) {
				return Err("Region state contains a duplicate region".into());
			}
			count += delta.objects.len();
			if count > MAX_CHANGED_OBJECTS {
				return Err("Region state exceeds its changed object limit".into());
			}
			for (key, value) in &delta.objects {
				WorldRegion::validate_entity_key(key)?;
				if !value.transform.matrix().is_finite() {
					return Err("Region state transform must be finite".into());
				}
			}
		}
		Ok(())
	}
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegionDelta {
	scene: SceneId,
	objects: BTreeMap<EntityKey, RegionObjectState>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegionObjectState {
	transform: TransformData,
	renderable: bool,
	collision_enabled: bool,
}

impl RegionObjectState {
	fn from_entity(entity: &EntityData) -> Self {
		Self {
			transform: entity.transform.clone(),
			renderable: entity.renderable,
			collision_enabled: entity.collision_enabled,
		}
	}

	fn apply(&self, entity: &mut EntityData) {
		entity.transform = self.transform.clone();
		entity.renderable = self.renderable;
		entity.collision_enabled = self.collision_enabled;
	}
}

#[derive(Default)]
pub(super) struct WorldRegions {
	resident: BTreeMap<String, ResidentRegion>,
	saved: BTreeMap<String, RegionDelta>,
	state_revision: u64,
}

struct ResidentRegion {
	scene: SceneId,
	originals: BTreeMap<EntityKey, RegionObjectState>,
}

impl ResidentRegion {
	fn current_delta(&self, world: &RuntimeWorld) -> Result<RegionDelta, String> {
		let mut objects = BTreeMap::new();
		for (key, original) in &self.originals {
			let index = world
				.entity_index(key)
				.ok_or("Resident region entity is missing")?;
			let current = RegionObjectState::from_entity(&world.data().entities[index]);
			if &current != original {
				objects.insert(key.clone(), current);
			}
		}
		Ok(RegionDelta {
			scene: self.scene.clone(),
			objects,
		})
	}
}

enum RegionChange {
	Attach(ResidentRegion),
	Detach(RegionDelta),
}

impl SimulationSession {
	pub fn resident_regions(&self) -> Vec<SceneId> {
		self.regions
			.resident
			.values()
			.map(|region| region.scene.clone())
			.collect()
	}

	/// Prefer the possessed actor. Legacy controlled scenes use their first
	/// authored movement target until they configure local possession.
	pub fn streaming_focus(&self) -> Option<[f32; 3]> {
		let key = if self.possession.managed {
			&self.local_player()?.key
		} else {
			self.controlled_targets.first()?
		};
		self.region_entity_position(key)
	}

	pub fn prepare_region_attach(
		&self,
		scene_id: &SceneId,
		mut region: Arc<SceneData>,
	) -> Result<PreparedRegionChange, String> {
		self.region_spec(scene_id)?.validate_content(&region)?;
		if self.regions.resident.contains_key(scene_id.as_str()) {
			return Err("Region is already resident".into());
		}
		if self.regions.resident.len() >= MAX_RESIDENT_REGIONS {
			return Err("Resident region budget exceeded".into());
		}
		region.validate()?;
		if region.entities.iter().any(|entity| !entity.active) {
			Arc::make_mut(&mut region).project_runtime_activation();
		}

		let mut incoming = region.entities.clone();
		let mut originals = BTreeMap::new();
		for entity in &incoming {
			if self.world.entity_index(&entity.key).is_some() {
				return Err(format!(
					"Region entity {:?} already exists in the world",
					entity.key.as_str()
				));
			}
			originals.insert(entity.key.clone(), RegionObjectState::from_entity(entity));
		}
		if let Some(delta) = self.regions.saved.get(scene_id.as_str()) {
			if delta.objects.keys().any(|key| !originals.contains_key(key)) {
				return Err(
					"Saved region state refers to an entity missing from its authored region"
						.into(),
				);
			}
			for entity in &mut incoming {
				if let Some(value) = delta.objects.get(&entity.key) {
					value.apply(entity);
				}
			}
		}

		let mut document = self.world.data().clone();
		let added_start = document.entities.len();
		document.entities.extend(incoming);
		self.prepare_region_change(
			scene_id,
			document,
			added_start,
			Vec::new(),
			RegionChange::Attach(ResidentRegion {
				scene: scene_id.clone(),
				originals,
			}),
		)
	}

	/// Check whether any live actor or foreign child is keeping a resident region loaded.
	/// Preparation checks the same conditions before building the detach transaction.
	pub fn can_detach_region(&self, scene_id: &SceneId) -> bool {
		self.region_spec(scene_id)
			.is_ok_and(|spec| self.region_for_retirement(spec).is_ok())
	}

	pub fn prepare_region_detach(
		&self,
		scene_id: &SceneId,
	) -> Result<PreparedRegionChange, String> {
		let region = self.region_for_retirement(self.region_spec(scene_id)?)?;
		let keys: HashSet<_> = region.originals.keys().collect();
		let delta = region.current_delta(&self.world)?;
		let mut document = self.world.data().clone();
		document
			.entities
			.retain(|entity| !keys.contains(&entity.key));
		document
			.organization
			.root_membership
			.retain(|key, _| !keys.contains(key));
		let added_start = document.entities.len();
		let removed = region.originals.keys().cloned().collect();
		let remaining_changes = self
			.regions
			.saved
			.values()
			.filter(|saved| &saved.scene != scene_id)
			.map(|saved| saved.objects.len())
			.sum::<usize>();
		if remaining_changes + delta.objects.len() > MAX_CHANGED_OBJECTS {
			return Err("Region state exceeds its changed object limit".into());
		}
		self.prepare_region_change(
			scene_id,
			document,
			added_start,
			removed,
			RegionChange::Detach(delta),
		)
	}

	pub fn commit_region_change(
		&mut self,
		prepared: PreparedRegionChange,
	) -> Result<WorldChange, String> {
		if prepared.world_id != self.lifecycle.world_id()
			|| prepared.topology_revision != self.lifecycle.topology_revision
			|| prepared.state_revision != self.regions.state_revision
			|| prepared.tick != self.tick
			|| !Arc::ptr_eq(&prepared.source, &self.world.document())
		{
			return Err("Prepared region belongs to a stale world, tick, or topology".into());
		}
		let added = prepared.added_entities();
		if self.visibility_capabilities.is_some()
			&& added.iter().any(|entity| {
			entity.active
				&& entity.renderable
				&& entity.has_mesh_component()
				&& prepared
				.visible
				.as_ref()
				.is_none_or(|available| !available.contains(&entity.key))
		}) {
			return Err("Prepare native render resources before admitting a region".into());
		}
		let changed_entities = added.iter().map(|entity| entity.key.clone()).collect();
		let removed_keys: HashSet<_> = prepared.removed.iter().cloned().collect();
		let mut lifecycle = self.lifecycle.clone();
		match &prepared.change {
			RegionChange::Attach(_) => lifecycle.admit_region_entities(added)?,
			RegionChange::Detach(_) => lifecycle.removed(&removed_keys)?,
		}

		let id = prepared.scene_id.as_str().to_owned();
		match prepared.change {
			RegionChange::Attach(region) => {
				self.regions.resident.insert(id, region);
			}
			RegionChange::Detach(delta) => {
				self.regions.resident.remove(&id);
				if delta.objects.is_empty() {
					self.regions.saved.remove(&id);
				} else {
					self.regions.saved.insert(id, delta);
				}
			}
		}
		if let Some(available) = &mut self.visibility_capabilities {
			available.retain(|key| !removed_keys.contains(key));
			available.extend(prepared.visible.into_iter().flatten());
		}
		self.triggers.remove_pairs_for(&removed_keys);
		self.world = prepared.world;
		self.collisions = prepared.collisions;
		self.lifecycle = lifecycle;
		self.camera_exclusions = None;
		self.triggers.structural_change(&self.world);
		Ok(WorldChange {
			changed_entities,
			removed_entities: prepared.removed,
			topology_changed: true,
			full_resync: true,
			..WorldChange::default()
		})
	}

	pub fn capture_region_state(&self) -> Result<WorldRegionState, String> {
		let mut deltas = self.regions.saved.clone();
		for (id, resident) in &self.regions.resident {
			deltas.insert(id.clone(), resident.current_delta(&self.world)?);
		}
		let state = WorldRegionState {
			version: REGION_SAVE_VERSION,
			world: self.scene_id.clone(),
			regions: deltas
				.into_values()
				.filter(|delta| !delta.objects.is_empty())
				.collect(),
		};
		state.check_budget()?;
		Ok(state)
	}

	/// Restore saved changes before admitting any region in this world.
	pub fn restore_region_state(&mut self, state: WorldRegionState) -> Result<(), String> {
		if state.world != self.scene_id || !self.regions.resident.is_empty() {
			return Err(
				"Restore region state into its matching world before streaming starts".into(),
			);
		}
		state.check_budget()?;
		for delta in &state.regions {
			self.region_spec(&delta.scene)?;
		}
		let revision = self
			.regions
			.state_revision
			.checked_add(1)
			.ok_or("Region state counter exhausted")?;
		self.regions.saved = state
			.regions
			.into_iter()
			.map(|delta| (delta.scene.as_str().to_owned(), delta))
			.collect();
		self.regions.state_revision = revision;
		Ok(())
	}

	fn region_spec(&self, scene_id: &SceneId) -> Result<&WorldRegion, String> {
		self.project
			.regions_for(&self.scene_id)
			.find(|region| &region.scene == scene_id)
			.ok_or_else(|| {
				format!(
					"Region {:?} is not registered for this world",
					scene_id.as_str()
				)
			})
	}

	fn region_for_retirement(&self, spec: &WorldRegion) -> Result<&ResidentRegion, &'static str> {
		let region = self
			.regions
			.resident
			.get(spec.scene.as_str())
			.ok_or("Region is not resident")?;
		if self
			.streaming_focus()
			.is_some_and(|position| spec.bounds.distance_to(position) <= spec.release_distance)
			|| self
			.characters
			.keys()
			.filter_map(|key| self.region_entity_position(key))
			.any(|position| spec.bounds.distance_to(position) == 0.0)
		{
			return Err("Cannot retire a region while a gameplay actor needs it");
		}
		if self.world.data().entities.iter().any(|entity| {
			entity.parent.as_ref().is_some_and(|parent| {
				region.originals.contains_key(parent) && !region.originals.contains_key(&entity.key)
			})
		}) {
			return Err("Cannot retire a region that owns a foreign runtime child");
		}
		Ok(region)
	}

	fn region_entity_position(&self, key: &EntityKey) -> Option<[f32; 3]> {
		let index = self.world.entity_index(key)?;
		let position = self.world.world_matrices()[index].transform_point3(Vec3::ZERO);
		(self.world.data().entities[index].active && position.is_finite())
			.then(|| position.to_array())
	}

	fn prepare_region_change(
		&self,
		scene_id: &SceneId,
		document: SceneData,
		added_start: usize,
		removed: Vec<EntityKey>,
		change: RegionChange,
	) -> Result<PreparedRegionChange, String> {
		if self.project.physics_layers.is_some() {
			crate::project::validate_required_physics_contacts(
				&document,
				&self.collisions.matrix(),
			)?;
		}
		let world = self.world.rebuild_runtime_document(Arc::new(document))?;
		let collisions = CollisionWorld::with_matrix(&world, self.collisions.matrix())?;
		Ok(PreparedRegionChange {
			source: self.world.document(),
			world_id: self.lifecycle.world_id().to_owned(),
			topology_revision: self.lifecycle.topology_revision,
			state_revision: self.regions.state_revision,
			tick: self.tick,
			scene_id: scene_id.clone(),
			document: world.document(),
			world,
			collisions,
			change,
			added_start,
			removed,
			visible: None,
		})
	}
}
