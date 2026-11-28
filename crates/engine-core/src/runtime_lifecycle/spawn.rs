use super::{MAX_COUNTER, MAX_RUNTIME_INSTANCES, RuntimeLifecycle, RuntimeSpawnRequest};
use crate::{
	EntityKey,
	editor::PrefabData,
	scene_data::{EntityData, SceneData},
	world::RuntimeWorld,
};
use std::collections::{HashMap, HashSet};

/// Prepared immutable CPU content. Native hosts must prepare the matching render
/// candidate before calling commit. Tokens are tied to one runtime topology.
pub struct PreparedRuntimeSpawn {
	pub(super) world_id: String,
	pub(super) topology_revision: u64,
	pub(super) instance_serial: u64,
	pub(crate) entities: Vec<EntityData>,
	pub(crate) request: Option<RuntimeSpawnRequest>,
	pub(super) source_fingerprint: String,
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
	/// Region objects retain authored keys, but receive a new handle generation
	/// on every attachment. They are removed by their region owner.
	pub(crate) fn admit_region_entities(&mut self, entities: &[EntityData]) -> Result<(), String> {
		let generation = self.next_instance;
		let next = generation
			.checked_add(1)
			.filter(|value| *value <= MAX_COUNTER)
			.ok_or("Runtime entity generation exhausted")?;
		if entities
			.iter()
			.any(|entity| self.generations.contains_key(&entity.key))
		{
			return Err("A region entity is already admitted to the world".into());
		}
		self.advance_topology()?;
		let generations = std::sync::Arc::make_mut(&mut self.generations);
		generations.extend(
			entities
				.iter()
				.map(|entity| (entity.key.clone(), generation)),
		);
		self.next_instance = next;
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
			if request.projectile_direction.is_some() {
				prefab.validate_projectile_launch()?;
			}
		}
		let sources = prefab.expanded_entities()?;
		let mut serial = self.next_instance;
		let keys = loop {
			let keys = (0..sources.len())
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
		let mapping: HashMap<_, _> = sources
			.iter()
			.zip(&keys)
			.map(|(entity, key)| (entity.key.clone(), key.clone()))
			.collect();
		let bindings = request
			.as_ref()
			.map(|request| request.bindings.clone())
			.unwrap_or_default();
		let mut entities =
			PrefabData::remap_expanded(&sources, &prefab.reference_slots, &mapping, &bindings)?;
		if let Some(direction) = request
			.as_ref()
			.and_then(|request| request.projectile_direction)
		{
			crate::projectile::orient_launch(&mut entities, direction)?;
		}
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
}
