//! Scene-local runtime ownership. Copies of this state are committed with a tick.
use crate::{
	EntityKey,
	components::{ComponentId, NativeBehaviorSpec},
	editor::PrefabData,
	scene_data::{EntityData, SceneData},
	world::RuntimeWorld,
};
use serde::{Deserialize, Serialize};
use std::{
	collections::{BTreeMap, BTreeSet, HashMap, HashSet},
	sync::Arc,
};

pub const MAX_RUNTIME_INSTANCES: usize = 1024;
pub const MAX_RUNTIME_TIMERS: usize = 4096;
pub const MAX_PENDING_SPAWNS: usize = 32;
const MAX_COUNTER: u64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeEntityHandle {
	pub world_id: String,
	pub key: EntityKey,
	pub generation: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct NativeBehaviorState {
	pub activated: bool,
	pub elapsed_ticks: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeBehaviorStateDto {
	pub component_id: ComponentId,
	pub kind: String,
	pub activated: bool,
	pub elapsed_ticks: u32,
}
#[derive(Debug, Clone, Serialize)]
pub struct RuntimeEntityDto {
	pub handle: RuntimeEntityHandle,
	pub name: String,
	pub spawned: bool,
	pub instance_id: Option<String>,
	pub behavior_states: Vec<RuntimeBehaviorStateDto>,
}
#[derive(Debug, Clone, Serialize)]
pub struct RuntimeInspectionDto {
	pub play_session_id: String,
	pub run_id: String,
	pub scene_id: crate::scene_catalog::SceneId,
	pub tick: u64,
	pub topology_revision: u64,
	pub total_entities: usize,
	pub entities: Vec<RuntimeEntityDto>,
	pub timer_count: usize,
	pub pending_spawns: usize,
	pub last_error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RuntimeSpawnRequest {
	pub id: u64,
	pub owner: RuntimeEntityHandle,
	pub component_id: ComponentId,
	pub prefab_path: String,
	pub bindings: BTreeMap<String, EntityKey>,
	pub position: [f32; 3],
	pub lifetime_ticks: u32,
}
#[derive(Clone)]
struct RuntimeInstance {
	keys: Vec<EntityKey>,
	owner: Option<RuntimeEntityHandle>,
	component_id: Option<ComponentId>,
}
/// Ownership changes only at structural commits. Share the topology across tick
/// transactions, and detach all of its indexes together before a mutation.
#[derive(Clone, Default)]
struct RuntimeInstances {
	by_id: BTreeMap<String, RuntimeInstance>,
	by_entity: HashMap<EntityKey, String>,
	by_owner: BTreeMap<EntityKey, BTreeMap<ComponentId, BTreeSet<String>>>,
}
#[derive(Clone)]
struct RuntimeTimer {
	target: RuntimeEntityHandle,
	due: u64,
}

/// Contains no mutable asset definitions or renderer resources.
#[derive(Clone)]
pub(crate) struct RuntimeLifecycle {
	world_id: String,
	pub topology_revision: u64,
	next_instance: u64,
	next_request: u64,
	generations: Arc<BTreeMap<EntityKey, u64>>,
	instances: Arc<RuntimeInstances>,
	pub states: BTreeMap<(EntityKey, ComponentId), NativeBehaviorState>,
	timers: Vec<RuntimeTimer>,
	requests: BTreeMap<u64, RuntimeSpawnRequest>,
	pub last_error: Option<String>,
}

/// Prepared immutable CPU content. Native hosts must prepare the matching render
/// candidate before calling commit. Tokens are tied to one runtime topology.
pub struct PreparedRuntimeSpawn {
	world_id: String,
	topology_revision: u64,
	instance_serial: u64,
	pub(crate) entities: Vec<EntityData>,
	pub(crate) request: Option<RuntimeSpawnRequest>,
	source_fingerprint: String,
	pub(crate) visible: Option<HashSet<EntityKey>>,
}
impl PreparedRuntimeSpawn {
	pub fn source_fingerprint(&self) -> &str {
		&self.source_fingerprint
	}
	pub fn entities(&self) -> &[EntityData] {
		&self.entities
	}
	pub fn instance_id(&self) -> String {
		format!("spawn-{}", self.instance_serial)
	}
	pub fn set_visibility_capabilities(
		&mut self,
		available: HashSet<EntityKey>,
	) -> Result<(), String> {
		let keys: HashSet<_> = self.entities.iter().map(|entity| &entity.key).collect();
		if available.iter().any(|key| !keys.contains(key)) {
			return Err("Spawn visibility capabilities contain a foreign entity".into());
		}
		self.visible = Some(available);
		Ok(())
	}
}

impl RuntimeLifecycle {
	pub fn new(world: &RuntimeWorld) -> Self {
		Self {
			world_id: uuid::Uuid::new_v4().to_string(),
			topology_revision: 0,
			next_instance: 1,
			next_request: 1,
			generations: Arc::new(
				world
					.data()
					.entities
					.iter()
					.map(|entity| (entity.key.clone(), 0))
					.collect(),
			),
			instances: Arc::new(RuntimeInstances::default()),
			states: BTreeMap::new(),
			timers: Vec::new(),
			requests: BTreeMap::new(),
			last_error: None,
		}
	}
	pub fn handle(&self, key: &EntityKey) -> Option<RuntimeEntityHandle> {
		self.generations
			.get(key)
			.map(|generation| RuntimeEntityHandle {
				world_id: self.world_id.clone(),
				key: key.clone(),
				generation: *generation,
			})
	}
	pub fn validate_handle(&self, handle: &RuntimeEntityHandle) -> Result<(), String> {
		if handle.world_id != self.world_id
			|| self.generations.get(&handle.key) != Some(&handle.generation)
		{
			return Err("Runtime handle is stale or belongs to another world".into());
		}
		Ok(())
	}
	pub fn has_instances(&self) -> bool {
		!self.instances.by_id.is_empty()
	}
	pub fn prune_states(&mut self, world: &RuntimeWorld) {
		self.states.retain(|(key, id), _| {
			world.entity_index(key).is_some_and(|index| {
				world.data().entities[index]
					.native_behaviors
					.iter()
					.any(|component| &component.id == id)
			})
		});
	}
	pub fn needs_tick(&self) -> bool {
		!self.timers.is_empty()
	}
	pub fn requests(&self) -> Vec<RuntimeSpawnRequest> {
		self.requests.values().cloned().collect()
	}
	pub fn reject_request(&mut self, id: u64, error: &str) {
		if self.requests.remove(&id).is_some() {
			self.last_error = Some(error.chars().take(512).collect());
		}
	}
	pub fn instance_for(&self, key: &EntityKey) -> Option<&str> {
		self.instances.by_entity.get(key).map(String::as_str)
	}
	pub fn alive_for(&self, owner: &EntityKey, component: &ComponentId) -> usize {
		self.instances
			.by_owner
			.get(owner)
			.and_then(|components| components.get(component))
			.map_or(0, BTreeSet::len)
	}
	pub fn has_request(&self, owner: &EntityKey, component: &ComponentId) -> bool {
		self.requests
			.values()
			.any(|request| &request.owner.key == owner && &request.component_id == component)
	}
	pub fn enqueue(
		&mut self,
		owner: &EntityKey,
		component_id: &ComponentId,
		prefab_path: &str,
		bindings: &BTreeMap<String, EntityKey>,
		position: [f32; 3],
		lifetime_ticks: u32,
	) -> Result<(), String> {
		if self.requests.len() >= MAX_PENDING_SPAWNS {
			return Err("Runtime pending spawn budget exceeded".into());
		}
		let handle = self.handle(owner).ok_or("Spawner owner no longer exists")?;
		if self.next_request >= MAX_COUNTER {
			return Err("Runtime request counter exhausted".into());
		}
		let id = self.next_request;
		self.next_request += 1;
		self.requests.insert(
			id,
			RuntimeSpawnRequest {
				id,
				owner: handle,
				component_id: component_id.clone(),
				prefab_path: prefab_path.into(),
				bindings: bindings.clone(),
				position,
				lifetime_ticks,
			},
		);
		Ok(())
	}
	pub fn prepare(
		&self,
		world: &RuntimeWorld,
		prefab: &PrefabData,
		fingerprint: &str,
		position: [f32; 3],
		request_id: Option<u64>,
	) -> Result<PreparedRuntimeSpawn, String> {
		prefab.validate()?;
		if prefab.gameplay.is_some() {
			return Err("Runtime spawning requires component prefabs; convert legacy scene-role gameplay first".into());
		}
		if !position.iter().all(|value| value.is_finite())
			|| fingerprint.len() != 64
			|| !fingerprint.bytes().all(|value| value.is_ascii_hexdigit())
		{
			return Err("Spawn needs a finite position and a captured SHA-256 fingerprint".into());
		}
		if self.instances.by_id.len() >= MAX_RUNTIME_INSTANCES || self.next_instance >= MAX_COUNTER
		{
			return Err("Runtime instance budget exceeded".into());
		}
		let request = request_id
			.map(|id| {
				self.requests
					.get(&id)
					.cloned()
					.ok_or("Spawn request expired")
			})
			.transpose()?;
		if let Some(request) = &request {
			self.validate_handle(&request.owner)?;
		}
		let mut serial = self.next_instance;
		let keys = loop {
			let keys = (0..prefab.entities.len())
				.map(|index| EntityKey::new(format!("runtime-{serial}-{index}")))
				.collect::<Result<Vec<_>, _>>()?;
			if keys.iter().all(|key| world.entity_index(key).is_none()) {
				break keys;
			}
			serial = serial
				.checked_add(1)
				.filter(|serial| *serial < MAX_COUNTER)
				.ok_or("Runtime identity counter exhausted")?;
		};
		let mapping: HashMap<_, _> = prefab
			.entities
			.iter()
			.zip(&keys)
			.map(|(entity, key)| (entity.key.clone(), key.clone()))
			.collect();
		let bindings = request
			.as_ref()
			.map(|request| request.bindings.clone())
			.unwrap_or_default();
		let mut entities = prefab.remap_entities(&mapping, &bindings)?;
		for entity in &mut entities {
			if entity.parent.is_none() {
				for (value, offset) in entity.transform.position.iter_mut().zip(position) {
					*value += offset;
				}
			}
		}
		let prepared = PreparedRuntimeSpawn {
			world_id: self.world_id.clone(),
			topology_revision: self.topology_revision,
			instance_serial: serial,
			entities,
			request,
			source_fingerprint: fingerprint.into(),
			visible: None,
		};
		self.candidate(world, &prepared)?;
		Ok(prepared)
	}
	pub fn candidate(
		&self,
		world: &RuntimeWorld,
		prepared: &PreparedRuntimeSpawn,
	) -> Result<SceneData, String> {
		if prepared.world_id != self.world_id
			|| prepared.topology_revision != self.topology_revision
		{
			return Err("Prepared spawn belongs to a stale runtime topology".into());
		}
		if let Some(request) = &prepared.request {
			self.validate_handle(&request.owner)?;
			if !self.requests.contains_key(&request.id) {
				return Err("Prepared spawn request was cancelled".into());
			}
		}
		let mut data = world.data().clone();
		data.entities.extend(prepared.entities.iter().cloned());
		data.validate()?;
		Ok(data)
	}
	pub fn admit(&mut self, prepared: &PreparedRuntimeSpawn, tick: u64) -> Result<(), String> {
		let next_revision = self
			.topology_revision
			.checked_add(1)
			.filter(|value| *value <= MAX_COUNTER)
			.ok_or("Runtime topology counter exhausted")?;
		let timer = prepared
			.request
			.as_ref()
			.filter(|request| request.lifetime_ticks > 0)
			.map(|request| -> Result<RuntimeTimer, String> {
				if self.timers.len() >= MAX_RUNTIME_TIMERS {
					return Err("Runtime timer budget exceeded".into());
				}
				let due = tick
					.checked_add(u64::from(request.lifetime_ticks))
					.ok_or("Runtime timer deadline overflow")?;
				Ok(RuntimeTimer {
					target: RuntimeEntityHandle {
						world_id: self.world_id.clone(),
						key: prepared.entities[0].key.clone(),
						generation: prepared.instance_serial,
					},
					due,
				})
			})
			.transpose()?;
		self.topology_revision = next_revision;
		self.next_instance = prepared.instance_serial + 1;
		let keys: Vec<_> = prepared
			.entities
			.iter()
			.map(|entity| entity.key.clone())
			.collect();
		let id = prepared.instance_id();
		let generations = Arc::make_mut(&mut self.generations);
		let instances = Arc::make_mut(&mut self.instances);
		for key in &keys {
			generations.insert(key.clone(), prepared.instance_serial);
			instances.by_entity.insert(key.clone(), id.clone());
		}
		if let Some(request) = &prepared.request {
			instances
				.by_owner
				.entry(request.owner.key.clone())
				.or_default()
				.entry(request.component_id.clone())
				.or_default()
				.insert(id.clone());
		}
		instances.by_id.insert(
			id,
			RuntimeInstance {
				keys,
				owner: prepared
					.request
					.as_ref()
					.map(|request| request.owner.clone()),
				component_id: prepared
					.request
					.as_ref()
					.map(|request| request.component_id.clone()),
			},
		);
		if let Some(timer) = timer {
			self.timers.push(timer);
		}
		if let Some(request) = &prepared.request {
			self.requests.remove(&request.id);
		}
		self.last_error = None;
		Ok(())
	}
	pub fn due(&self, tick: u64) -> Vec<RuntimeEntityHandle> {
		self.timers
			.iter()
			.filter(|timer| timer.due <= tick)
			.map(|timer| timer.target.clone())
			.collect()
	}
	pub fn removal(&self, handle: &RuntimeEntityHandle) -> Result<HashSet<EntityKey>, String> {
		self.validate_handle(handle)?;
		let id = self
			.instance_for(&handle.key)
			.ok_or("Only spawned runtime instances can be despawned")?;
		let mut keys = HashSet::new();
		let mut pending = vec![id];
		// Each instance has one owner, admitted before the instance itself, so
		// ownership is a forest. Visit only the removed subtree, once per key.
		while let Some(id) = pending.pop() {
			for key in &self.instances.by_id[id].keys {
				keys.insert(key.clone());
				if let Some(components) = self.instances.by_owner.get(key) {
					pending.extend(
						components
							.values()
							.flat_map(|ids| ids.iter().map(String::as_str)),
					);
				}
			}
		}
		Ok(keys)
	}
	pub fn removed(&mut self, keys: &HashSet<EntityKey>) -> Result<(), String> {
		self.topology_revision = self
			.topology_revision
			.checked_add(1)
			.filter(|value| *value <= MAX_COUNTER)
			.ok_or("Runtime topology counter exhausted")?;
		Arc::make_mut(&mut self.generations).retain(|key, _| !keys.contains(key));
		let removed: BTreeSet<_> = keys
			.iter()
			.filter_map(|key| self.instances.by_entity.get(key).cloned())
			.collect();
		let instances = Arc::make_mut(&mut self.instances);
		for id in removed {
			let instance = instances
				.by_id
				.remove(&id)
				.expect("indexed runtime instance");
			for key in &instance.keys {
				instances.by_entity.remove(key);
			}
			if let (Some(owner), Some(component)) = (&instance.owner, &instance.component_id) {
				let components = instances
					.by_owner
					.get_mut(&owner.key)
					.expect("indexed owner");
				let ids = components.get_mut(component).expect("indexed component");
				ids.remove(&id);
				if ids.is_empty() {
					components.remove(component);
				}
				if components.is_empty() {
					instances.by_owner.remove(&owner.key);
				}
			}
		}
		self.states.retain(|(key, _), _| !keys.contains(key));
		self.timers
			.retain(|timer| !keys.contains(&timer.target.key));
		self.requests
			.retain(|_, request| !keys.contains(&request.owner.key));
		Ok(())
	}
	pub fn entities(
		&self,
		world: &RuntimeWorld,
		offset: usize,
		limit: usize,
	) -> Vec<RuntimeEntityDto> {
		world
			.data()
			.entities
			.iter()
			.skip(offset)
			.take(limit.min(128))
			.map(|entity| {
				let instance_id = self.instance_for(&entity.key).map(str::to_owned);
				RuntimeEntityDto {
					handle: self.handle(&entity.key).expect("live world has handles"),
					name: entity.name.clone(),
					spawned: instance_id.is_some(),
					instance_id,
					behavior_states: entity
						.native_behaviors
						.iter()
						.map(|component| {
							let state = self
								.states
								.get(&(entity.key.clone(), component.id.clone()))
								.cloned()
								.unwrap_or_default();
							RuntimeBehaviorStateDto {
								component_id: component.id.clone(),
								kind: match &component.behavior {
									NativeBehaviorSpec::KeyDoor { .. } => "key_door",
									NativeBehaviorSpec::Collectible { .. } => "collectible",
									NativeBehaviorSpec::TimedSpawner { .. } => "timed_spawner",
								}
								.into(),
								activated: state.activated,
								elapsed_ticks: state.elapsed_ticks,
							}
						})
						.collect(),
				}
			})
			.collect()
	}
	pub fn timer_count(&self) -> usize {
		self.timers.len()
	}
	pub fn request_count(&self) -> usize {
		self.requests.len()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::scene_data::{CURRENT_SCHEMA_VERSION, CameraData};

	fn key(value: &str) -> EntityKey {
		EntityKey::new(value).unwrap()
	}
	fn component(value: &str) -> ComponentId {
		ComponentId::new(value).unwrap()
	}
	fn lifecycle() -> RuntimeLifecycle {
		let camera = crate::Camera::default();
		let world = RuntimeWorld::new(Arc::new(SceneData {
			schema_version: CURRENT_SCHEMA_VERSION,
			bindings: BTreeMap::new(),
			entities: vec![EntityData::new(key("owner"), "Owner", "models/item.gltf")],
			camera: CameraData {
				follow: None,
				position: camera.position,
				yaw: camera.yaw,
				pitch: camera.pitch,
				vertical_fov: camera.vertical_fov,
				near_plane: camera.near,
				far_plane: camera.far,
			},
			lights: vec![],
		}))
		.unwrap();
		RuntimeLifecycle::new(&world)
	}
	// Exercise private admission bookkeeping directly; public API tests cover
	// prefab/world validation and admission of the matching world candidate.
	fn prepared(
		state: &mut RuntimeLifecycle,
		owner: Option<(&EntityKey, &ComponentId)>,
		count: usize,
	) -> PreparedRuntimeSpawn {
		let request = owner.map(|(owner, component)| {
			let id = state.next_request;
			state
				.enqueue(
					owner,
					component,
					"prefabs/item.json",
					&BTreeMap::new(),
					[0.; 3],
					10,
				)
				.unwrap();
			state.requests[&id].clone()
		});
		PreparedRuntimeSpawn {
			world_id: state.world_id.clone(),
			topology_revision: state.topology_revision,
			instance_serial: state.next_instance,
			entities: (0..count)
				.map(|index| {
					EntityData::new(
						key(&format!("runtime-{}-{index}", state.next_instance)),
						"Item",
						"models/item.gltf",
					)
				})
				.collect(),
			request,
			source_fingerprint: "a".repeat(64),
			visible: None,
		}
	}
	fn admit(
		state: &mut RuntimeLifecycle,
		owner: Option<(&EntityKey, &ComponentId)>,
		count: usize,
	) -> Vec<EntityKey> {
		let prepared = prepared(state, owner, count);
		state.admit(&prepared, 0).unwrap();
		prepared
			.entities
			.into_iter()
			.map(|entity| entity.key)
			.collect()
	}

	#[test]
	fn tick_clones_share_ownership_and_structural_changes_detach_all_indexes() {
		let mut original = lifecycle();
		let owner = key("owner");
		let component = component("emitter");
		let first = admit(&mut original, Some((&owner, &component)), 16);
		let mut transaction = original.clone();
		assert!(Arc::ptr_eq(&original.instances, &transaction.instances));
		assert!(Arc::ptr_eq(&original.generations, &transaction.generations));
		let second = admit(&mut transaction, Some((&owner, &component)), 16);
		assert!(!Arc::ptr_eq(&original.instances, &transaction.instances));
		assert_eq!(original.alive_for(&owner, &component), 1);
		assert_eq!(transaction.alive_for(&owner, &component), 2);
		assert!(original.instance_for(&second[0]).is_none());
		assert!(original.handle(&second[0]).is_none());
		let handle = transaction.handle(&first[0]).unwrap();
		let removed = transaction.removal(&handle).unwrap();
		transaction.removed(&removed).unwrap();
		assert_eq!(transaction.alive_for(&owner, &component), 1);
		assert_eq!(original.instance_for(&first[15]), Some("spawn-1"));
		assert_eq!(transaction.instance_for(&second[15]), Some("spawn-2"));
		assert!(transaction.validate_handle(&handle).is_err());
		assert!(original.validate_handle(&handle).is_ok());
	}

	#[test]
	fn removal_visits_child_owners_and_cleans_counts_timers_requests_and_states() {
		let mut state = lifecycle();
		let owner = key("owner");
		let emitter = component("emitter");
		let alternate = component("alternate");
		let parent = admit(&mut state, Some((&owner, &emitter)), 2);
		let child = admit(&mut state, Some((&parent[1], &emitter)), 2);
		let grandchild = admit(&mut state, Some((&child[1], &alternate)), 1);
		let sibling = admit(&mut state, Some((&owner, &alternate)), 1);
		let unowned = admit(&mut state, None, 1);
		state
			.enqueue(
				&parent[1],
				&alternate,
				"prefabs/item.json",
				&BTreeMap::new(),
				[0.; 3],
				1,
			)
			.unwrap();
		state.states.insert(
			(child[1].clone(), emitter.clone()),
			NativeBehaviorState::default(),
		);
		let snapshot = state.clone();
		let removed = state.removal(&state.handle(&parent[1]).unwrap()).unwrap();
		assert_eq!(removed.len(), 5);
		assert!(removed.contains(&grandchild[0]));
		state.removed(&removed).unwrap();
		assert_eq!(state.alive_for(&owner, &emitter), 0);
		assert_eq!(state.alive_for(&owner, &alternate), 1);
		assert_eq!(state.alive_for(&parent[1], &emitter), 0);
		assert_eq!(state.timer_count(), 1);
		assert_eq!(state.request_count(), 0);
		assert!(state.states.is_empty());
		assert_eq!(state.instances.by_id.len(), 2);
		assert_eq!(state.instances.by_entity.len(), 2);
		assert_eq!(state.instances.by_owner.len(), 1);
		assert!(state.instance_for(&sibling[0]).is_some());
		assert!(state.instance_for(&unowned[0]).is_some());
		assert_eq!(snapshot.alive_for(&owner, &emitter), 1);
		assert_eq!(snapshot.timer_count(), 4);
		assert_eq!(snapshot.request_count(), 1);
	}

	#[test]
	fn ownership_traversal_handles_the_instance_budget_without_recursive_scans() {
		let mut state = lifecycle();
		let emitter = component("emitter");
		let mut owner = key("owner");
		let mut root = None;
		for _ in 0..MAX_RUNTIME_INSTANCES {
			owner = admit(&mut state, Some((&owner, &emitter)), 1).remove(0);
			root.get_or_insert_with(|| owner.clone());
		}
		let removed = state
			.removal(&state.handle(&root.unwrap()).unwrap())
			.unwrap();
		assert_eq!(removed.len(), MAX_RUNTIME_INSTANCES);
		state.removed(&removed).unwrap();
		assert!(!state.has_instances());
		assert!(state.instances.by_entity.is_empty());
		assert!(state.instances.by_owner.is_empty());
		assert_eq!(state.timer_count(), 0);
	}

	#[test]
	fn visibility_membership_accepts_large_valid_subsets_and_preserves_on_rejection() {
		let mut state = lifecycle();
		let mut prepared = prepared(&mut state, None, PrefabData::MAX_ENTITIES);
		let available: HashSet<_> = prepared
			.entities
			.iter()
			.map(|entity| entity.key.clone())
			.collect();
		prepared
			.set_visibility_capabilities(available.clone())
			.unwrap();
		let mut invalid = available.clone();
		invalid.insert(key("foreign"));
		assert!(prepared.set_visibility_capabilities(invalid).is_err());
		assert_eq!(prepared.visible, Some(available));
		prepared
			.set_visibility_capabilities(HashSet::new())
			.unwrap();
		assert_eq!(prepared.visible, Some(HashSet::new()));
	}
}
