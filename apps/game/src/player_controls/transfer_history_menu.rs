//! Read-only results of recent completed transfers for the selected save slot.

use super::{GameApp, Key, advance_row};
use crate::checkpoint_transfer_history::{Direction, HistoryEntry};
use courtyard_gameplay::checkpoint::SaveSlot;
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) struct TransferHistoryMenu {
	rows: Vec<String>,
	focus: usize,
	title: String,
}

impl TransferHistoryMenu {
	pub(super) fn new(app: &GameApp, slot: SaveSlot) -> Result<Self, String> {
		let now = SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.map_or(0, |time| time.as_secs());
		Ok(Self::from_entries(
			slot,
			app.checkpoint_transfer_history(slot)?,
			now,
		))
	}

	pub(super) fn rows(&self) -> (String, Vec<String>, usize) {
		(self.title.clone(), self.rows.clone(), self.focus)
	}

	/// True while the read-only page keeps ownership of navigation.
	pub(super) fn key(&mut self, key: Key) -> bool {
		match key {
			Key::Escape => return false,
			Key::Enter if self.focus + 1 == self.rows.len() => return false,
			Key::ArrowUp | Key::ArrowDown => {
				self.focus = advance_row(self.focus, self.rows.len(), key);
			}
			_ => {}
		}
		true
	}

	fn from_entries(slot: SaveSlot, entries: Vec<HistoryEntry>, now: u64) -> Self {
		let title = if entries.is_empty() {
			"No checkpoint transfers".into()
		} else {
			format!("{} transfer history", slot.label())
		};
		let mut rows: Vec<_> = entries
			.into_iter()
			.map(|entry| {
				let action = match entry.direction {
					Direction::Export => "Exported",
					Direction::Import => "Imported",
				};
				let age = now.saturating_sub(entry.completed_unix_seconds);
				let age = if age < 60 {
					"now".into()
				} else if age < 3600 {
					format!("{}m ago", age / 60)
				} else if age < 86400 {
					format!("{}h ago", age / 3600)
				} else {
					format!("{}d ago", age / 86400)
				};
				format!("{action} {} ({age})", entry.checkpoint_id)
			})
			.collect();
		rows.push("Back".into());
		Self {
			rows,
			title,
			focus: 0,
		}
	}
}
