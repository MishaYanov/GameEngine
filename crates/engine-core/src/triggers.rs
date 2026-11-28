//! Directed, deterministic sensor edges. Tracker state belongs to one runtime scene.
use crate::{EntityKey, collision::Contact, scene_catalog::SceneId, world::RuntimeWorld};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const MAX_TRIGGER_PAIRS: usize = 8192;
pub const MAX_TRIGGER_EVENTS_PER_TICK: usize = 16_384;
pub const MAX_TRIGGER_KEY_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerPhase {
	Exit,
	Enter,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TriggerEvent {
	pub scene_id: SceneId,
	pub step: u64,
	pub phase: TriggerPhase,
	pub sensor: EntityKey,
	pub other: EntityKey,
}

/// Read-only accepted overlaps for this step; no duplicate collision query is needed.
#[derive(Clone, Copy)]
pub struct TriggerView<'a> {
	pairs: &'a HashSet<(EntityKey, EntityKey)>,
}
impl TriggerView<'_> {
	pub fn contains(&self, sensor: &EntityKey, other: &EntityKey) -> bool {
		self.pairs.contains(&(sensor.clone(), other.clone()))
	}
}

#[derive(Clone)]
pub(crate) struct TriggerTracker {
	pairs: HashSet<(EntityKey, EntityKey)>,
	has_sensors: bool,
	pub(crate) sample_needed: bool,
}

impl TriggerTracker {
	pub(crate) fn view(&self) -> TriggerView<'_> {
		TriggerView { pairs: &self.pairs }
	}
	pub(crate) fn new(world: &RuntimeWorld) -> Self {
		// Include disabled sensors: a later enable action must wake sampling.
		let has_sensors = world.data().entities.iter().any(|entity| {
			entity
				.collider
				.as_ref()
				.is_some_and(|collider| collider.interaction().sensor)
		});
		Self {
			pairs: HashSet::new(),
			has_sensors,
			sample_needed: has_sensors,
		}
	}

	pub(crate) fn remove_pairs_for(&mut self, keys: &HashSet<EntityKey>) {
		if !keys.is_empty() {
			self.pairs
				.retain(|(sensor, actor)| !keys.contains(sensor) && !keys.contains(actor));
		}
	}
	pub(crate) fn invalidate(&mut self) {
		self.sample_needed |= self.has_sensors || !self.pairs.is_empty();
	}
	pub(crate) fn structural_change(&mut self, world: &RuntimeWorld) {
		self.has_sensors = world.data().entities.iter().any(|entity| {
			entity
				.collider
				.as_ref()
				.is_some_and(|collider| collider.interaction().sensor)
		});
		self.sample_needed = true;
	}

	pub(crate) fn sample(
		&mut self,
		world: &RuntimeWorld,
		contacts: &[Contact],
		scene_id: &SceneId,
		step: u64,
		events: &mut Vec<TriggerEvent>,
		event_bytes: &mut usize,
	) -> Result<(), String> {
		let mut current = HashSet::new();
		let mut retained_bytes = 0usize;
		for contact in contacts {
			for (sensor, other) in [(&contact.a, &contact.b), (&contact.b, &contact.a)] {
				let entity = &world.data().entities[world
					.entity_index(sensor)
					.ok_or("Trigger participant is missing")?];
				if !entity.collision_enabled
					|| !entity
						.collider
						.as_ref()
						.is_some_and(|value| value.interaction().sensor)
				{
					continue;
				}
				if current.len() >= MAX_TRIGGER_PAIRS {
					return Err("Trigger pair budget exceeded".into());
				}
				retained_bytes = retained_bytes
					.saturating_add(sensor.as_str().len())
					.saturating_add(other.as_str().len());
				if retained_bytes > MAX_TRIGGER_KEY_BYTES {
					return Err("Trigger key byte budget exceeded".into());
				}
				current.insert((sensor.clone(), other.clone()));
			}
		}
		let start = events.len();
		for (phase, source, previous) in [
			(TriggerPhase::Exit, &self.pairs, &current),
			(TriggerPhase::Enter, &current, &self.pairs),
		] {
			for (sensor, other) in source.difference(previous) {
				if events.len() == MAX_TRIGGER_EVENTS_PER_TICK {
					return Err("Trigger event budget exceeded".into());
				}
				*event_bytes = event_bytes
					.saturating_add(sensor.as_str().len())
					.saturating_add(other.as_str().len())
					.saturating_add(scene_id.as_str().len());
				if *event_bytes > MAX_TRIGGER_KEY_BYTES {
					return Err("Trigger event byte budget exceeded".into());
				}
				events.push(TriggerEvent {
					scene_id: scene_id.clone(),
					step,
					phase,
					sensor: sensor.clone(),
					other: other.clone(),
				});
			}
		}
		events[start..].sort_unstable_by(|a, b| {
			(a.phase, a.sensor.as_str(), a.other.as_str()).cmp(&(
				b.phase,
				b.sensor.as_str(),
				b.other.as_str(),
			))
		});
		self.pairs = current;
		self.sample_needed = false;
		Ok(())
	}
}
