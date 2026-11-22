//! Profile-local receipt of completed checkpoint transfer operations.

use crate::local_player_files::{
	lock, ordinary_directory, ordinary_file, profile_directory, read_bounded,
};
use courtyard_gameplay::checkpoint::{CheckpointRegistry, SaveSlot};
use runtime::scene_io::{replace_bytes, save_new_bytes};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
	path::{Path, PathBuf},
	time::{SystemTime, UNIX_EPOCH},
};

const VERSION: u32 = 2;
const AUDIT_VERSION: u32 = 1;
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

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "usize", into = "usize")]
pub(crate) enum HistoryRetention {
	Entries16,
	Entries32,
	#[default]
	Entries64,
}

impl HistoryRetention {
	pub(crate) fn limit(self) -> usize {
		match self {
			Self::Entries16 => 16,
			Self::Entries32 => 32,
			Self::Entries64 => MAX_ENTRIES,
		}
	}
}

impl TryFrom<usize> for HistoryRetention {
	type Error = &'static str;

	fn try_from(limit: usize) -> Result<Self, Self::Error> {
		match limit {
			16 => Ok(Self::Entries16),
			32 => Ok(Self::Entries32),
			MAX_ENTRIES => Ok(Self::Entries64),
			_ => Err("Checkpoint transfer history retention must be 16, 32, or 64"),
		}
	}
}

impl From<HistoryRetention> for usize {
	fn from(retention: HistoryRetention) -> Self {
		retention.limit()
	}
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct HistoryRecord {
	schema_version: u32,
	revision: u64,
	retention: HistoryRetention,
	entries: Vec<HistoryEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyHistoryRecord {
	schema_version: u32,
	entries: Vec<HistoryEntry>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum DiskHistoryRecord {
	Current(HistoryRecord),
	Legacy(LegacyHistoryRecord),
}

impl HistoryRecord {
	fn empty() -> Self {
		Self {
			schema_version: VERSION,
			revision: 0,
			retention: HistoryRetention::default(),
			entries: Vec::new(),
		}
	}

	fn load(path: &Path) -> Result<Self, String> {
		let Some(bytes) = read_bounded(path, MAX_BYTES)? else {
			return Ok(Self::empty());
		};
		let record = match serde_json::from_slice(&bytes).map_err(|error| error.to_string())? {
			DiskHistoryRecord::Current(current)
			if current.schema_version == VERSION && current.revision != 0 =>
				{
					current
				}
			DiskHistoryRecord::Legacy(legacy) if legacy.schema_version == 1 => Self {
				entries: legacy.entries,
				..Self::empty()
			},
			_ => return Err("Unsupported checkpoint transfer history".into()),
		};
		if record.entries.len() > record.retention.limit() {
			return Err("Unsupported checkpoint transfer history".into());
		}
		for entry in &record.entries {
			entry.validate()?;
		}
		Ok(record)
	}

	fn trim(&mut self) {
		let removed = self.entries.len().saturating_sub(self.retention.limit());
		self.entries.drain(..removed);
	}

	fn encoded(&self) -> Result<Vec<u8>, String> {
		let bytes = serde_json::to_vec(self).map_err(|error| error.to_string())?;
		if bytes.len() > MAX_BYTES {
			return Err("Checkpoint transfer history exceeds 32 KiB".into());
		}
		Ok(bytes)
	}
}

pub(crate) struct HistoryView {
	pub(crate) retention: HistoryRetention,
	pub(crate) entries: Vec<HistoryEntry>,
}

pub(crate) struct ReviewedHistoryRetention {
	retention: HistoryRetention,
	pub(crate) rows_to_remove: usize,
	record_sha256: [u8; 32],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ClearHistoryScope {
	Slot(SaveSlot),
	CurrentProject,
}

impl ClearHistoryScope {
	fn includes(self, entry: &HistoryEntry) -> bool {
		match self {
			Self::Slot(slot) => entry.slot == slot_number(slot),
			Self::CurrentProject => true,
		}
	}
}

pub(crate) struct ReviewedHistoryClear {
	scope: ClearHistoryScope,
	rows_to_remove: usize,
	record_sha256: [u8; 32],
}

impl ReviewedHistoryClear {
	pub(crate) fn scope(&self) -> ClearHistoryScope {
		self.scope
	}

	pub(crate) fn rows_to_remove(&self) -> usize {
		self.rows_to_remove
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

	#[cfg(test)]
	fn slot(&self, slot: SaveSlot) -> Result<Vec<HistoryEntry>, String> {
		self.snapshot(Some(slot))
	}

	pub(crate) fn snapshot(&self, slot: Option<SaveSlot>) -> Result<Vec<HistoryEntry>, String> {
		Ok(self.view(slot)?.entries)
	}

	pub(crate) fn view(&self, slot: Option<SaveSlot>) -> Result<HistoryView, String> {
		let HistoryRecord {
			retention, entries, ..
		} = self.read()?;
		Ok(HistoryView {
			retention,
			entries: entries
				.into_iter()
				.rev()
				.filter(|entry| slot.is_none_or(|slot| entry.slot == slot_number(slot)))
				.collect(),
		})
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
		self.update_record(|record| {
			record.entries.push(entry);
			record.trim();
			Ok(())
		})
	}

	pub(crate) fn review_retention(
		&self,
		retention: HistoryRetention,
	) -> Result<ReviewedHistoryRetention, String> {
		let record = self.read()?;
		Ok(ReviewedHistoryRetention {
			retention,
			rows_to_remove: record.entries.len().saturating_sub(retention.limit()),
			record_sha256: self.review_digest(&record)?,
		})
	}

	pub(crate) fn set_retention(&self, review: &ReviewedHistoryRetention) -> Result<usize, String> {
		self.update_record(|record| {
			self.check_review(record, review.record_sha256)?;
			record.retention = review.retention;
			let before = record.entries.len();
			record.trim();
			Ok(before - record.entries.len())
		})
	}

	pub(crate) fn review_clear(
		&self,
		scope: ClearHistoryScope,
	) -> Result<ReviewedHistoryClear, String> {
		let record = self.read()?;
		Ok(ReviewedHistoryClear {
			scope,
			rows_to_remove: record
				.entries
				.iter()
				.filter(|entry| scope.includes(entry))
				.count(),
			record_sha256: self.review_digest(&record)?,
		})
	}

	pub(crate) fn clear_reviewed(&self, review: &ReviewedHistoryClear) -> Result<usize, String> {
		if review.rows_to_remove == 0 {
			return Err("There are no transfer history entries to clear".into());
		}
		self.update_record(|record| {
			self.check_review(record, review.record_sha256)?;
			let before = record.entries.len();
			record.entries.retain(|entry| !review.scope.includes(entry));
			Ok(before - record.entries.len())
		})
	}

	fn read(&self) -> Result<HistoryRecord, String> {
		self.folder(false)?.map_or_else(
			|| Ok(HistoryRecord::empty()),
			|folder| HistoryRecord::load(&folder.join(FILE)),
		)
	}

	fn update_record<T>(
		&self,
		change: impl FnOnce(&mut HistoryRecord) -> Result<T, String>,
	) -> Result<T, String> {
		let folder = self
			.folder(true)?
			.ok_or("Transfer history storage is unavailable")?;
		let path = folder.join(FILE);
		let _lock = lock(&folder.join(LOCK_FILE))?;
		let mut record = HistoryRecord::load(&path)?;
		let result = change(&mut record)?;
		record.revision = record
			.revision
			.checked_add(1)
			.ok_or("Transfer history revision is exhausted")?;
		ordinary_file(&path)?;
		replace_bytes(&path, &record.encoded()?)?;
		Ok(result)
	}

	fn review_digest(&self, record: &HistoryRecord) -> Result<[u8; 32], String> {
		let profile = self
			.profile
			.canonicalize()
			.map_err(|error| error.to_string())?;
		let mut digest = Sha256::new();
		digest.update(b"game-engine-checkpoint-transfer-history-v2");
		for field in [
			profile.as_os_str().as_encoded_bytes(),
			self.project_save_id.as_bytes(),
		] {
			digest.update((field.len() as u64).to_le_bytes());
			digest.update(field);
		}
		digest.update(record.encoded()?);
		Ok(digest.finalize().into())
	}

	fn check_review(&self, record: &HistoryRecord, reviewed: [u8; 32]) -> Result<(), String> {
		if self.review_digest(record)? != reviewed {
			return Err("Transfer history changed. Review it again.".into());
		}
		Ok(())
	}

	fn folder(&self, create: bool) -> Result<Option<PathBuf>, String> {
		profile_directory(
			self.profile,
			&["GameEngine", "checkpoints", self.project_save_id],
			create,
		)
	}
}

#[derive(Serialize)]
struct HistoryAudit<'a> {
	schema_version: u32,
	entries: Vec<&'a HistoryEntry>,
}

/// Publish the reviewed projection to a fresh file without reloading history.
pub(crate) fn publish_audit<'a>(
	path: &Path,
	entries: impl IntoIterator<Item=&'a HistoryEntry>,
) -> Result<usize, String> {
	let entries: Vec<_> = entries.into_iter().take(MAX_ENTRIES + 1).collect();
	let count = entries.len();
	if count > MAX_ENTRIES {
		return Err("Checkpoint transfer history exceeds 64 entries".into());
	}
	let bytes = serde_json::to_vec(&HistoryAudit {
		schema_version: AUDIT_VERSION,
		entries,
	})
		.map_err(|error| error.to_string())?;
	if bytes.len() > MAX_BYTES {
		return Err("Checkpoint transfer history exceeds 32 KiB".into());
	}
	let parent = path
		.parent()
		.filter(|parent| !parent.as_os_str().is_empty())
		.unwrap_or(Path::new("."));
	ordinary_directory(parent, false, false)?;
	save_new_bytes(path, &bytes)?;
	Ok(count)
}

pub(crate) fn slot_number(slot: SaveSlot) -> u8 {
	match slot {
		SaveSlot::One => 1,
		SaveSlot::Two => 2,
		SaveSlot::Three => 3,
	}
}

#[cfg(test)]
#[path = "../tests/unit/checkpoint_transfer_history.rs"]
mod tests;
