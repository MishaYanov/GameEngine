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
mod tests {
	use super::*;
	fn envelope(signal: Signal) -> Envelope {
		let endpoint = ComponentEndpoint {
			owner: RuntimeEntityHandle {
				world_id: "test".into(),
				key: EntityKey::new("switch").unwrap(),
				generation: 1,
			},
			component: ComponentId::new("logic").unwrap(),
		};
		Envelope {
			source: endpoint.clone(),
			target: endpoint,
			signal,
		}
	}
	#[test]
	fn next_step_delivery_retains_emission_order_and_never_redrains_new_output() {
		let mut queue = EventQueue::default();
		let mut emitted = 0;
		let mut delivered = 0;
		for signal in [Signal::Reset, Signal::Activated, Signal::Collected] {
			queue.emit_next(9, &mut emitted, envelope(signal)).unwrap();
		}
		assert!(queue.take_due(9, &mut delivered).unwrap().is_empty());
		let frozen = queue.take_due(10, &mut delivered).unwrap();
		assert_eq!(
			frozen.iter().map(|e| e.signal).collect::<Vec<_>>(),
			[Signal::Reset, Signal::Activated, Signal::Collected]
		);
		queue
			.emit_next(10, &mut emitted, envelope(Signal::Released))
			.unwrap();
		assert!(queue.take_due(10, &mut delivered).unwrap().is_empty());
		assert_eq!(queue.take_due(11, &mut delivered).unwrap().len(), 1);
	}
	#[test]
	fn exact_storage_and_producer_caps_reject_without_mutation() {
		let mut queue = EventQueue::default();
		for step in 0..2 {
			let mut emitted = 0;
			for _ in 0..MAX_EVENTS_PER_STEP {
				queue
					.emit_next(step, &mut emitted, envelope(Signal::Activated))
					.unwrap();
			}
			let before = queue.clone();
			assert!(
				queue
					.emit_next(step, &mut emitted, envelope(Signal::Reset))
					.is_err()
			);
			assert_eq!(queue, before);
			assert_eq!(emitted, MAX_EVENTS_PER_STEP);
		}
		assert_eq!(queue.len(), MAX_PENDING_EVENTS);
		let before = queue.clone();
		assert!(queue.emit_next(2, &mut 0, envelope(Signal::Reset)).is_err());
		assert_eq!(queue, before);
	}
	#[test]
	fn delivery_budget_and_counter_overflow_do_not_consume_or_enqueue() {
		let mut queue = EventQueue::default();
		let mut emitted = 0;
		queue
			.emit_next(0, &mut emitted, envelope(Signal::Activated))
			.unwrap();
		let before = queue.clone();
		let mut delivered = MAX_DELIVERIES_PER_TICK;
		assert!(queue.take_due(1, &mut delivered).is_err());
		assert_eq!(queue, before);
		assert!(
			queue
				.emit_next(u64::MAX, &mut emitted, envelope(Signal::Reset))
				.is_err()
		);
		assert_eq!(queue, before);
		queue.next_sequence = MAX_SEQUENCE;
		let before = queue.clone();
		assert!(
			queue
				.emit_next(1, &mut emitted, envelope(Signal::Reset))
				.is_err()
		);
		assert_eq!(queue, before);
	}
	#[test]
	fn retiring_either_endpoint_cancels_only_its_queued_messages() {
		let mut queue = EventQueue::default();
		let mut first = envelope(Signal::Activated);
		first.source.owner.key = EntityKey::new("sender").unwrap();
		let mut second = envelope(Signal::Reset);
		second.target.owner.key = EntityKey::new("receiver").unwrap();
		queue.emit_next(1, &mut 0, first).unwrap();
		queue.emit_next(1, &mut 0, second).unwrap();
		queue.remove_owners(&HashSet::from([EntityKey::new("sender").unwrap()]));
		assert_eq!(queue.len(), 1);
		queue.remove_owners(&HashSet::from([EntityKey::new("receiver").unwrap()]));
		assert_eq!(queue.len(), 0);
	}
}
