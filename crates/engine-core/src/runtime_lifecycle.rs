//! Scene-local runtime ownership. Copies of this state are committed with a tick.
mod inspection;
mod spawn;
pub use inspection::{
	ControlGraphStepDto, RuntimeBehaviorStateDto, RuntimeEntityDto, RuntimeInspectionDto,
};
pub use spawn::PreparedRuntimeSpawn;

use crate::{
	EntityKey,
	components::{ComponentId, NativeBehaviorSpec},
	scene_data::EntityData,
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
	pub machines:
		Arc<BTreeMap<(EntityKey, ComponentId), Arc<crate::state_machine::CompiledMachine>>>,
	pub machine_states: BTreeMap<(EntityKey, ComponentId), crate::state_machine::MachineInstance>,
	pub events: crate::gameplay_events::EventQueue,
	pub stale_events_discarded: u64,
	timers: Vec<RuntimeTimer>,
	requests: BTreeMap<u64, RuntimeSpawnRequest>,
	pub last_error: Option<String>,
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
			machines: Arc::new(
				Self::compile_machines(&world.data().entities)
					.expect("runtime world has validated machine definitions"),
			),
			machine_states: BTreeMap::new(),
			events: crate::gameplay_events::EventQueue::default(),
			stale_events_discarded: 0,
			timers: Vec::new(),
			requests: BTreeMap::new(),
			last_error: None,
		}
	}
	pub(crate) fn world_id(&self) -> &str {
		&self.world_id
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
		let live = |(key, id): &(EntityKey, ComponentId)| {
			world.entity_index(key).is_some_and(|index| {
				world.data().entities[index]
					.native_behaviors
					.iter()
					.any(|component| {
						&component.id == id
							&& component.enabled && matches!(
							component.behavior,
							NativeBehaviorSpec::StateMachine { .. }
						)
					})
			})
		};
		if self.machines.keys().any(|address| !live(address)) {
			Arc::make_mut(&mut self.machines).retain(|address, _| live(address));
		}
		self.machine_states.retain(|address, _| live(address));
	}
	pub fn needs_tick(&self) -> bool {
		!self.timers.is_empty()
			|| self.events.len() > 0
			|| self.machines.iter().any(|(address, definition)| {
				self.machine_states.get(address).is_none_or(|state| {
					!state.initialized || definition.states[state.state].timer.is_some()
				})
			})
	}
	pub fn machine_deadlines(&self) -> usize {
		self.machine_states
			.iter()
			.filter(|(address, state)| {
				state.initialized
					&& self
						.machines
						.get(*address)
						.is_some_and(|definition| definition.states[state.state].timer.is_some())
			})
			.count()
	}
	fn compile_machines(
		entities: &[EntityData],
	) -> Result<
		BTreeMap<(EntityKey, ComponentId), Arc<crate::state_machine::CompiledMachine>>,
		String,
	> {
		let mut definitions = BTreeMap::new();
		for entity in entities {
			for component in &entity.native_behaviors {
				if component.enabled
					&& let NativeBehaviorSpec::StateMachine { definition } = &component.behavior
				{
					definitions.insert(
						(entity.key.clone(), component.id.clone()),
						Arc::new(crate::state_machine::compile(definition)?),
					);
				}
			}
		}
		Ok(definitions)
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
	pub fn admit(&mut self, prepared: &PreparedRuntimeSpawn, tick: u64) -> Result<(), String> {
		let machines = Self::compile_machines(&prepared.entities)?;
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
		if !machines.is_empty() {
			Arc::make_mut(&mut self.machines).extend(machines);
		}
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
	pub(crate) fn validate_reparent(
		&self,
		target: &RuntimeEntityHandle,
		parent: Option<&RuntimeEntityHandle>,
	) -> Result<(), String> {
		self.validate_handle(target)?;
		if let Some(parent) = parent {
			self.validate_handle(parent)?;
			let owner = self.instance_for(&parent.key);
			if owner.is_some() && owner != self.instance_for(&target.key) {
				return Err("Runtime parent must share the child's spawned lifetime".into());
			}
		}
		Ok(())
	}

	pub(crate) fn advance_topology(&mut self) -> Result<(), String> {
		self.topology_revision = self
			.topology_revision
			.checked_add(1)
			.filter(|value| *value <= MAX_COUNTER)
			.ok_or("Runtime topology counter exhausted")?;
		Ok(())
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
		self.advance_topology()?;
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
		if self.machines.keys().any(|(key, _)| keys.contains(key)) {
			Arc::make_mut(&mut self.machines).retain(|(key, _), _| !keys.contains(key));
		}
		self.machine_states
			.retain(|(key, _), _| !keys.contains(key));
		self.events.remove_owners(keys);
		self.timers
			.retain(|timer| !keys.contains(&timer.target.key));
		self.requests
			.retain(|_, request| !keys.contains(&request.owner.key));
		Ok(())
	}
}

#[cfg(test)]
#[path = "../tests/unit/runtime_lifecycle.rs"]
mod tests;
