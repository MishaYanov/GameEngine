//! Profile-local receipt of completed checkpoint transfer operations.

use crate::local_player_files::{lock, ordinary_file, profile_directory, read_bounded};
use courtyard_gameplay::checkpoint::{CheckpointRegistry, SaveSlot};
use runtime::scene_io::replace_bytes;
use serde::{Deserialize, Serialize};
use std::{
	path::{Path, PathBuf},
	time::{SystemTime, UNIX_EPOCH},
};

const VERSION: u32 = 1;
const MAX_ENTRIES: usize = 64;
const MAX_BYTES: usize = 32 * 1024;
const FILE: &str = "transfer-history.json";
const LOCK_FILE: &str = "transfer-history.lock";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Direction {
	Export,
	Import,
}

#[derive(Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HistoryEntry {
	slot: u8,
	pub(crate) direction: Direction,
	pub(crate) checkpoint_id: String,
	pub(crate) completed_unix_seconds: u64,
}

impl HistoryEntry {
	fn validate(&self) -> Result<(), String> {
		if !(1..=3).contains(&self.slot)
			|| self.checkpoint_id.is_empty()
			|| self.checkpoint_id.len() > 64
			|| !self.checkpoint_id.bytes().all(|byte| {
				byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
			}) {
			return Err("Invalid checkpoint history entry".into());
		}
		Ok(())
	}
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct HistoryRecord {
	schema_version: u32,
	entries: Vec<HistoryEntry>,
}

impl HistoryRecord {
	fn load(path: &Path) -> Result<Self, String> {
		let Some(bytes) = read_bounded(path, MAX_BYTES)? else {
			return Ok(Self {
				schema_version: VERSION,
				entries: Vec::new(),
			});
		};
		let record: Self = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
		if record.schema_version != VERSION || record.entries.len() > MAX_ENTRIES {
			return Err("Unsupported checkpoint transfer history".into());
		}
		for entry in &record.entries {
			entry.validate()?;
		}
		Ok(record)
	}
}

pub(crate) struct TransferHistory<'a> {
	profile: &'a Path,
	// Supplied only by the registry, which validates it as a canonical UUID.
	project_save_id: &'a str,
}

impl<'a> TransferHistory<'a> {
	pub(crate) fn new(profile: &'a Path, registry: &'a CheckpointRegistry) -> Self {
		Self {
			profile,
			project_save_id: registry.project_save_id(),
		}
	}

	pub(crate) fn slot(&self, slot: SaveSlot) -> Result<Vec<HistoryEntry>, String> {
		let Some(folder) = self.folder(false)? else {
			return Ok(Vec::new());
		};
		Ok(HistoryRecord::load(&folder.join(FILE))?
			.entries
			.into_iter()
			.rev()
			.filter(|entry| entry.slot == slot_number(slot))
			.collect())
	}

	pub(crate) fn append(
		&self,
		slot: SaveSlot,
		direction: Direction,
		checkpoint_id: String,
	) -> Result<(), String> {
		let completed_unix_seconds = SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.map_err(|error| error.to_string())?
			.as_secs();
		let entry = HistoryEntry {
			slot: slot_number(slot),
			direction,
			checkpoint_id,
			completed_unix_seconds,
		};
		entry.validate()?;
		let folder = self
			.folder(true)?
			.ok_or("Transfer history storage is unavailable")?;
		let path = folder.join(FILE);
		let _lock = lock(&folder.join(LOCK_FILE))?;
		let mut record = HistoryRecord::load(&path)?;
		record.entries.push(entry);
		if record.entries.len() > MAX_ENTRIES {
			record.entries.remove(0);
		}
		let bytes = serde_json::to_vec(&record).map_err(|error| error.to_string())?;
		if bytes.len() > MAX_BYTES {
			return Err("Checkpoint transfer history exceeds 32 KiB".into());
		}
		ordinary_file(&path)?;
		replace_bytes(&path, &bytes)
	}

	fn folder(&self, create: bool) -> Result<Option<PathBuf>, String> {
		profile_directory(
			self.profile,
			&["GameEngine", "checkpoints", self.project_save_id],
			create,
		)
	}
}

fn slot_number(slot: SaveSlot) -> u8 {
	match slot {
		SaveSlot::One => 1,
		SaveSlot::Two => 2,
		SaveSlot::Three => 3,
	}
}

#[cfg(test)]
#[path = "../tests/unit/checkpoint_transfer_history.rs"]
mod tests;
