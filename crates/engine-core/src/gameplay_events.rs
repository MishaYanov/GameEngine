//! Bounded, next-fixed-step gameplay delivery. Queue mutation stays inside the tick candidate.
use crate::{EntityKey, components::ComponentId, runtime_lifecycle::RuntimeEntityHandle};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

pub const MAX_PENDING_EVENTS: usize = 1024;
pub const MAX_EVENTS_PER_STEP: usize = 512;
pub const MAX_DELIVERIES_PER_TICK: usize = 4096;
const MAX_SEQUENCE: u64 = 9_007_199_254_740_991;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComponentRef {
	pub entity: EntityKey,
	pub component: ComponentId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Signal {
	Activated,
	Released,
	Collected,
	Reset,
	Jumped,
	Landed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ComponentEndpoint {
	pub owner: RuntimeEntityHandle,
	pub component: ComponentId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Envelope {
	pub source: ComponentEndpoint,
	pub target: ComponentEndpoint,
	pub signal: Signal,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct EventQueue {
	pending: BTreeMap<(u64, u64), Envelope>,
	next_sequence: u64,
}

impl EventQueue {
	pub fn emit_next(
		&mut self,
		step: u64,
		emitted: &mut usize,
		value: Envelope,
	) -> Result<(), String> {
		if self.pending.len() >= MAX_PENDING_EVENTS || *emitted >= MAX_EVENTS_PER_STEP {
			return Err("Gameplay event budget exceeded".into());
		}
		let due = step.checked_add(1).ok_or("Gameplay event step exhausted")?;
		let sequence = self.next_sequence;
		let next = sequence
			.checked_add(1)
			.filter(|v| *v <= MAX_SEQUENCE)
			.ok_or("Gameplay event sequence exhausted")?;
		self.pending.insert((due, sequence), value);
		self.next_sequence = next;
		*emitted += 1;
		Ok(())
	}
	pub fn take_due(&mut self, step: u64, delivered: &mut usize) -> Result<Vec<Envelope>, String> {
		let keys: Vec<_> = self
			.pending
			.range(..=(step, u64::MAX))
			.take(MAX_EVENTS_PER_STEP + 1)
			.map(|(key, _)| *key)
			.collect();
		let total = delivered
			.checked_add(keys.len())
			.filter(|count| *count <= MAX_DELIVERIES_PER_TICK)
			.ok_or("Gameplay tick delivery budget exceeded")?;
		if keys.len() > MAX_EVENTS_PER_STEP {
			return Err("Gameplay step delivery budget exceeded".into());
		}
		*delivered = total;
		Ok(keys
			.into_iter()
			.map(|key| self.pending.remove(&key).expect("queued key"))
			.collect())
	}

	pub fn remove_owners(&mut self, removed: &HashSet<EntityKey>) {
		self.pending.retain(|_, event| {
			!removed.contains(&event.source.owner.key) && !removed.contains(&event.target.owner.key)
		});
	}
	pub fn len(&self) -> usize {
		self.pending.len()
	}
	pub fn sequence(&self) -> u64 {
		self.next_sequence
	}
	pub fn replay_state(&self, world_id: &str) -> serde_json::Value {
		let endpoint = |value: &ComponentEndpoint| {
			serde_json::json!({
				"entity":value.owner.key,"generation":value.owner.generation,
				"same_world":value.owner.world_id == world_id,"component":value.component
			})
		};
		serde_json::json!({"sequence":self.next_sequence,"pending":self.pending.iter().map(|((due,sequence),event)|
			serde_json::json!({"due_tick":due,"sequence":sequence,"signal":event.signal,"source":endpoint(&event.source),"target":endpoint(&event.target)})
		).collect::<Vec<_>>()})
	}
}

#[cfg(test)]
#[path = "../tests/unit/gameplay_events.rs"]
mod tests;
