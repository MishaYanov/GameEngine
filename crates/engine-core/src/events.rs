//! Bounded pull-based change journal. Slow consumers receive a resync marker.
use crate::world::WorldChange;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BackendEventKind {
	DocumentChanged { changes: WorldChange },
	SimulationAdvanced { tick: u64, changes: WorldChange },
	StateChanged,
	ProjectChanged,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendEvent {
	pub sequence: u64,
	pub revision: u64,
	pub scene_revision: u64,
	pub event: BackendEventKind,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangePage {
	pub cursor: u64,
	pub resync_required: bool,
	pub events: Vec<BackendEvent>,
}

#[derive(Default)]
pub struct ChangeJournal {
	entries: VecDeque<(BackendEvent, usize)>,
	cursor: u64,
	bytes: usize,
	floor: u64,
}
impl ChangeJournal {
	pub const MAX_ENTRIES: usize = 256;
	pub const MAX_BYTES: usize = 1024 * 1024;
	pub fn cursor(&self) -> u64 {
		self.cursor
	}
	pub fn retained_bytes(&self) -> usize {
		self.bytes
	}
	pub fn publish(
		&mut self,
		revision: u64,
		scene_revision: u64,
		event: BackendEventKind,
	) -> Result<(), String> {
		let sequence = self
			.cursor
			.checked_add(1)
			.ok_or("Backend event sequence is exhausted")?;
		let event = BackendEvent {
			sequence,
			revision,
			scene_revision,
			event,
		};
		let bytes = serde_json::to_vec(&event).map_err(|e| e.to_string())?.len();
		self.cursor = sequence;
		if bytes > Self::MAX_BYTES {
			self.entries.clear();
			self.bytes = 0;
			self.floor = sequence;
			return Ok(());
		}
		while self.entries.len() >= Self::MAX_ENTRIES
			|| self.bytes.saturating_add(bytes) > Self::MAX_BYTES
		{
			if let Some((old, size)) = self.entries.pop_front() {
				self.bytes -= size;
				self.floor = old.sequence;
			}
		}
		self.bytes += bytes;
		self.entries.push_back((event, bytes));
		Ok(())
	}
	pub fn since(&self, cursor: u64) -> Result<ChangePage, String> {
		if cursor > self.cursor {
			return Err("Change cursor belongs to a newer backend state".into());
		}
		let resync_required = cursor < self.floor;
		Ok(ChangePage {
			cursor: self.cursor,
			resync_required,
			events: if resync_required {
				Vec::new()
			} else {
				self.entries
					.iter()
					.filter(|(e, _)| e.sequence > cursor)
					.map(|(e, _)| e.clone())
					.collect()
			},
		})
	}
}
