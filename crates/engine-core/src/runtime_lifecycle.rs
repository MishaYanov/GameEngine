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
	collections::{BTreeMap, HashMap, HashSet},
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
	instances: BTreeMap<String, RuntimeInstance>,
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
		if available
			.iter()
			.any(|key| !self.entities.iter().any(|entity| &entity.key == key))
		{
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
			instances: BTreeMap::new(),
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
		!self.instances.is_empty()
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
		self.instances
			.iter()
			.find(|(_, instance)| instance.keys.contains(key))
			.map(|(id, _)| id.as_str())
	}
	pub fn alive_for(&self, owner: &EntityKey, component: &ComponentId) -> usize {
		self.instances
			.values()
			.filter(|instance| {
				instance
					.owner
					.as_ref()
					.is_some_and(|handle| &handle.key == owner)
					&& instance.component_id.as_ref() == Some(component)
			})
			.count()
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
		if self.instances.len() >= MAX_RUNTIME_INSTANCES || self.next_instance >= MAX_COUNTER {
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
		for key in &keys {
			Arc::make_mut(&mut self.generations).insert(key.clone(), prepared.instance_serial);
		}
		self.instances.insert(
			prepared.instance_id(),
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
		let mut keys: HashSet<_> = self.instances[id].keys.iter().cloned().collect();
		loop {
			let before = keys.len();
			for instance in self.instances.values() {
				if instance
					.owner
					.as_ref()
					.is_some_and(|owner| keys.contains(&owner.key))
				{
					keys.extend(instance.keys.iter().cloned());
				}
			}
			if keys.len() == before {
				break;
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
		self.instances
			.retain(|_, instance| instance.keys.iter().all(|key| !keys.contains(key)));
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
