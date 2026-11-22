//! Bounded, project-scoped named input layouts in local Player preferences.

use super::{input_preset::PortablePreset, preferences::Preferences};
use crate::local_player_files::{
	ordinary_directory, ordinary_file, ordinary_transfer, read_bounded,
};
use runtime::scene_io::{replace_bytes, save_new_bytes};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

const VERSION: u32 = 2;
const BACKUP_VERSION: u32 = 1;
const MAX_PRESETS: usize = 8;
const MAX_LIBRARY_BYTES: usize = 40 * 1024;
pub(super) const MAX_LABEL_BYTES: usize = 32;
pub(super) const BACKUP_SUFFIX: &str = ".input-library.json";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SavedLayout {
	pub(super) label: String,
	pub(super) id: u64,
	revision: u64,
	pub(super) favorite: bool,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Entry {
	layout: SavedLayout,
	preset: PortablePreset,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Record {
	schema_version: u32,
	next_id: u64,
	entries: Vec<Entry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyLayout {
	label: String,
	id: u64,
	revision: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyEntry {
	layout: LegacyLayout,
	preset: PortablePreset,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyRecord {
	schema_version: u32,
	next_id: u64,
	entries: Vec<LegacyEntry>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum StoredRecord {
	Current(Record),
	Legacy(LegacyRecord),
}

impl Record {
	fn load(path: &Path) -> Result<Self, String> {
		let bytes = read_bounded(path, MAX_LIBRARY_BYTES)?;
		Self::decode(bytes.as_deref())
	}

	fn decode(bytes: Option<&[u8]>) -> Result<Self, String> {
		let Some(bytes) = bytes else {
			return Ok(Self::empty());
		};
		let stored: StoredRecord = serde_json::from_slice(bytes)
			.map_err(|error| format!("Saved layouts are invalid: {error}"))?;
		let record = match stored {
			StoredRecord::Current(record) if record.schema_version == VERSION => record,
			StoredRecord::Legacy(record) if record.schema_version == 1 => Self {
				schema_version: VERSION,
				next_id: record.next_id,
				entries: record
					.entries
					.into_iter()
					.map(|entry| Entry {
						layout: SavedLayout {
							label: entry.layout.label,
							id: entry.layout.id,
							revision: entry.layout.revision,
							favorite: false,
						},
						preset: entry.preset,
					})
					.collect(),
			},
			_ => return Err("Unsupported saved layout library".into()),
		};
		if record.next_id == 0 || record.entries.len() > MAX_PRESETS {
			return Err("Unsupported saved layout library".into());
		}
		for (index, entry) in record.entries.iter().enumerate() {
			let layout = &entry.layout;
			if layout.id == 0
				|| layout.id >= record.next_id
				|| layout.revision == 0
				|| normalize_label(&layout.label)? != layout.label
				|| record.entries[..index]
				.iter()
				.any(|prior| prior.layout.id == layout.id || prior.layout.label == layout.label)
			{
				return Err("Saved layouts contain an invalid or repeated identity".into());
			}
			entry.preset.encoded()?;
		}
		Ok(record)
	}

	fn empty() -> Self {
		Self {
			schema_version: VERSION,
			next_id: 1,
			entries: Vec::new(),
		}
	}

	fn insert(&mut self, label: String, preset: PortablePreset) -> Result<(), String> {
		let id = self.next_id;
		self.next_id = id
			.checked_add(1)
			.ok_or("Saved layout identifiers are exhausted")?;
		self.entries.push(Entry {
			layout: SavedLayout {
				label,
				id,
				revision: 1,
				favorite: false,
			},
			preset,
		});
		Ok(())
	}

	fn require_unique_label(&self, label: &str, except: Option<u64>) -> Result<(), String> {
		if self
			.entries
			.iter()
			.any(|entry| entry.layout.label == label && Some(entry.layout.id) != except)
		{
			Err("A saved layout already uses that name".into())
		} else {
			Ok(())
		}
	}

	fn index(&self, reviewed: &SavedLayout) -> Result<usize, String> {
		self.entries
			.iter()
			.position(|entry| &entry.layout == reviewed)
			.ok_or_else(|| "Saved layout changed. Reopen the list and review it again.".into())
	}
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct BackupEntry {
	label: String,
	preset: PortablePreset,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LibraryBackup {
	schema_version: u32,
	entries: Vec<BackupEntry>,
}

impl LibraryBackup {
	fn read(
		path: &Path,
		validate: &impl Fn(&PortablePreset) -> Result<(), String>,
	) -> Result<(Self, [u8; 32]), String> {
		if ordinary_transfer(path, BACKUP_SUFFIX)?.is_none() {
			return Err("Choose an ordinary input library backup file".into());
		}
		let bytes =
			read_bounded(path, MAX_LIBRARY_BYTES)?.ok_or("Input library backup is unavailable")?;
		let source_sha256 = Sha256::digest(&bytes).into();
		let mut backup: Self = serde_json::from_slice(&bytes)
			.map_err(|error| format!("Invalid input library backup: {error}"))?;
		if backup.schema_version != BACKUP_VERSION || backup.entries.len() > MAX_PRESETS {
			return Err("Unsupported input library backup".into());
		}
		for entry in &mut backup.entries {
			entry.label = normalize_label(&entry.label)?;
			entry.preset.encoded()?;
			validate(&entry.preset)?;
		}
		backup
			.entries
			.sort_unstable_by(|a, b| a.label.cmp(&b.label));
		if backup
			.entries
			.windows(2)
			.any(|pair| pair[0].label == pair[1].label)
		{
			return Err("Input library backup contains repeated layout names".into());
		}
		Ok((backup, source_sha256))
	}
}

/// A restore plan owns the editable choices for one exact source and destination.
#[derive(Debug)]
pub(super) struct LibraryBackupReview {
	backup_sha256: [u8; 32],
	destination_sha256: [u8; 32],
	local_labels: Vec<String>,
	pub(super) rows: Vec<RestoreRow>,
	pub(super) available: usize,
}

#[derive(Debug)]
pub(super) struct RestoreRow {
	pub(super) source_label: String,
	pub(super) conflict: bool,
	choice: RestoreChoice,
}

#[derive(Debug)]
enum RestoreChoice {
	UseLabel(String),
	Skip,
}

impl RestoreRow {
	pub(super) fn label(&self) -> Option<&str> {
		match &self.choice {
			RestoreChoice::UseLabel(label) => Some(label),
			RestoreChoice::Skip => None,
		}
	}
}

impl LibraryBackupReview {
	pub(super) fn blocked(&self) -> Option<&'static str> {
		let count = self.retained();
		if count == 0 {
			Some("No new layouts to restore")
		} else if count > self.available {
			Some("Library capacity exceeded")
		} else {
			None
		}
	}

	pub(super) fn retained(&self) -> usize {
		self.rows.iter().filter(|row| row.label().is_some()).count()
	}

	pub(super) fn skip(&mut self, index: usize) {
		self.rows[index].choice = RestoreChoice::Skip;
	}

	pub(super) fn use_label(&mut self, index: usize, label: &str) -> Result<(), String> {
		let label = normalize_label(label)?;
		if !self.label_available(index, &label) {
			return Err("A local or incoming layout already uses that name".into());
		}
		self.rows[index].choice = RestoreChoice::UseLabel(label);
		Ok(())
	}

	pub(super) fn suggestion(&self, index: usize) -> String {
		let base = &self.rows[index].source_label;
		for suffix in 2..=MAX_PRESETS * 2 + 1 {
			let suffix = format!(" {suffix}");
			let stem = base[..base.len().min(MAX_LABEL_BYTES - suffix.len())].trim_end();
			let candidate = format!("{stem}{suffix}");
			if self.label_available(index, &candidate) {
				return candidate;
			}
		}
		// Eight destination labels and seven other proposals leave at least one
		// of the sixteen candidates above free.
		unreachable!("bounded layout review exhausted its suggestions")
	}

	fn label_available(&self, index: usize, label: &str) -> bool {
		self.local_labels.iter().all(|existing| existing != label)
			&& self
			.rows
			.iter()
			.enumerate()
			.all(|(other, row)| other == index || row.label() != Some(label))
	}
}

pub(super) enum OrganizeLayout<'a> {
	SetFavorite(bool),
	MoveAdjacent(&'a SavedLayout),
}

pub(super) struct PresetLibrary<'a> {
	preferences: &'a Preferences,
}

impl<'a> PresetLibrary<'a> {
	pub(super) fn new(preferences: &'a Preferences) -> Self {
		Self { preferences }
	}

	pub(super) fn layouts(&self) -> Result<Vec<SavedLayout>, String> {
		let mut layouts: Vec<_> = self
			.read()?
			.into_iter()
			.flat_map(|record| record.entries)
			.map(|entry| entry.layout)
			.collect();
		layouts.sort_by_key(|layout| !layout.favorite);
		Ok(layouts)
	}

	pub(super) fn save(&self, label: &str, preset: PortablePreset) -> Result<(), String> {
		let label = normalize_label(label)?;
		preset.encoded()?;
		self.update(move |record| {
			if record.entries.len() == MAX_PRESETS {
				return Err("The library holds eight layouts. Delete one before saving.".into());
			}
			record.require_unique_label(&label, None)?;
			record.insert(label, preset)
		})
	}

	pub(super) fn export(
		&self,
		path: &Path,
		verify_scope: impl FnOnce() -> Result<(), String>,
		validate: impl Fn(&PortablePreset) -> Result<(), String>,
	) -> Result<usize, String> {
		let (bytes, count) = self.preferences.with_record_lock(true, |_, controls| {
			verify_scope()?;
			let record = Record::load(&controls.with_extension("preset-library.json"))?;
			let mut entries = Vec::with_capacity(record.entries.len());
			for Entry { layout, preset } in record.entries {
				validate(&preset)?;
				entries.push(BackupEntry {
					label: layout.label,
					preset,
				});
			}
			entries.sort_unstable_by(|a, b| a.label.cmp(&b.label));
			let count = entries.len();
			let backup = LibraryBackup {
				schema_version: BACKUP_VERSION,
				entries,
			};
			Ok((encode_library(&backup)?, count))
		})?;
		let parent = path
			.parent()
			.filter(|parent| !parent.as_os_str().is_empty())
			.unwrap_or(Path::new("."));
		ordinary_directory(parent, false, false)?;
		save_new_bytes(path, &bytes)?;
		Ok(count)
	}

	pub(super) fn review_backup(
		&self,
		path: &Path,
		validate: impl Fn(&PortablePreset) -> Result<(), String>,
	) -> Result<LibraryBackupReview, String> {
		let (backup, backup_sha256) = LibraryBackup::read(path, &validate)?;
		let bytes = self
			.preferences
			.preset_library_path()?
			.map(|path| read_bounded(&path, MAX_LIBRARY_BYTES))
			.transpose()?
			.flatten();
		let record = Record::decode(bytes.as_deref())?;
		let destination_sha256 = self.destination_digest(bytes.as_deref())?;
		let local_labels: Vec<_> = record
			.entries
			.into_iter()
			.map(|entry| entry.layout.label)
			.collect();
		let rows = backup
			.entries
			.into_iter()
			.map(|entry| {
				let conflict = local_labels.contains(&entry.label);
				let choice = if conflict {
					RestoreChoice::Skip
				} else {
					RestoreChoice::UseLabel(entry.label.clone())
				};
				RestoreRow {
					source_label: entry.label,
					conflict,
					choice,
				}
			})
			.collect();
		Ok(LibraryBackupReview {
			backup_sha256,
			destination_sha256,
			available: MAX_PRESETS - local_labels.len(),
			local_labels,
			rows,
		})
	}

	pub(super) fn restore_backup(
		&self,
		path: &Path,
		review: &LibraryBackupReview,
		verify_scope: impl FnOnce() -> Result<(), String>,
		validate: impl Fn(&PortablePreset) -> Result<(), String>,
	) -> Result<usize, String> {
		if let Some(reason) = review.blocked() {
			return Err(reason.into());
		}
		self.update_with_bytes(|record, bytes| {
			verify_scope()?;
			let (backup, source_sha256) = LibraryBackup::read(path, &validate)?;
			if source_sha256 != review.backup_sha256 {
				return Err("Input library backup changed. Review it again.".into());
			}
			if self.destination_digest(bytes)? != review.destination_sha256 {
				return Err("Local saved layouts changed. Review the backup again.".into());
			}
			if backup.entries.len() != review.rows.len() {
				return Err("Backup rows changed. Review the backup again.".into());
			}
			let mut additions = Vec::new();
			for (entry, row) in backup.entries.into_iter().zip(&review.rows) {
				if entry.label != row.source_label {
					return Err("Backup rows changed. Review the backup again.".into());
				}
				if let Some(label) = row.label() {
					let label = normalize_label(label)?;
					record.require_unique_label(&label, None)?;
					if additions.iter().any(|(other, _)| other == &label) {
						return Err("Two incoming layouts use the same name".into());
					}
					additions.push((label, entry.preset));
				}
			}
			let count = additions.len();
			if count == 0 || count > MAX_PRESETS - record.entries.len() {
				return Err("The reviewed layouts no longer fit the local library".into());
			}
			for (label, preset) in additions {
				record.insert(label, preset)?;
			}
			Ok(count)
		})
	}

	pub(super) fn organize(
		&self,
		reviewed: &SavedLayout,
		action: OrganizeLayout<'_>,
	) -> Result<(), String> {
		self.update(|record| {
			let index = record.index(reviewed)?;
			match action {
				OrganizeLayout::SetFavorite(favorite) => {
					let layout = &mut record.entries[index].layout;
					layout.favorite = favorite;
					layout.advance_revision()?;
				}
				OrganizeLayout::MoveAdjacent(neighbor) => {
					let other = record.index(neighbor)?;
					if index == other
						|| reviewed.favorite != neighbor.favorite
						|| record.entries[index.min(other) + 1..index.max(other)]
						.iter()
						.any(|entry| entry.layout.favorite == reviewed.favorite)
					{
						return Err("Layout order changed. Review the list again.".into());
					}
					record.entries[index].layout.advance_revision()?;
					record.entries.swap(index, other);
				}
			}
			Ok(())
		})
	}

	pub(super) fn rename(&self, reviewed: &SavedLayout, label: &str) -> Result<(), String> {
		let label = normalize_label(label)?;
		self.update(|record| {
			let index = record.index(reviewed)?;
			record.require_unique_label(&label, Some(reviewed.id))?;
			let layout = &mut record.entries[index].layout;
			if layout.label != label {
				layout.advance_revision()?;
				layout.label = label;
			}
			Ok(())
		})
	}

	pub(super) fn delete(&self, reviewed: &SavedLayout) -> Result<(), String> {
		self.update(|record| {
			let index = record.index(reviewed)?;
			record.entries.remove(index);
			Ok(())
		})
	}

	/// On apply, the caller holds the same controls lock as library writers.
	pub(super) fn source(&self, reviewed: &SavedLayout) -> Result<Vec<u8>, String> {
		let record = self.read()?.ok_or("Saved layout is unavailable")?;
		record.entries[record.index(reviewed)?].preset.encoded()
	}

	fn read(&self) -> Result<Option<Record>, String> {
		self.preferences
			.preset_library_path()?
			.as_deref()
			.map(Record::load)
			.transpose()
	}

	fn destination_digest(&self, bytes: Option<&[u8]>) -> Result<[u8; 32], String> {
		self.preferences
			.library_review_digest(bytes.unwrap_or_default())
	}

	fn update<T>(
		&self,
		change: impl FnOnce(&mut Record) -> Result<T, String>,
	) -> Result<T, String> {
		self.update_with_bytes(|record, _| change(record))
	}

	fn update_with_bytes<T>(
		&self,
		change: impl FnOnce(&mut Record, Option<&[u8]>) -> Result<T, String>,
	) -> Result<T, String> {
		self.preferences.with_record_lock(true, |_, controls| {
			let path = controls.with_extension("preset-library.json");
			let bytes = read_bounded(&path, MAX_LIBRARY_BYTES)?;
			let mut record = Record::decode(bytes.as_deref())?;
			let result = change(&mut record, bytes.as_deref())?;
			ordinary_file(&path)?;
			replace_bytes(&path, &encode_library(&record)?)?;
			Ok(result)
		})
	}
}

impl SavedLayout {
	fn advance_revision(&mut self) -> Result<(), String> {
		self.revision = self
			.revision
			.checked_add(1)
			.ok_or("Saved layout revision is exhausted")?;
		Ok(())
	}
}

fn encode_library(record: &impl Serialize) -> Result<Vec<u8>, String> {
	let bytes = serde_json::to_vec(record).map_err(|error| error.to_string())?;
	if bytes.len() > MAX_LIBRARY_BYTES {
		return Err("Saved layouts exceed 40 KiB".into());
	}
	Ok(bytes)
}

fn normalize_label(label: &str) -> Result<String, String> {
	if label.is_empty()
		|| label.len() > MAX_LABEL_BYTES
		|| !label
		.bytes()
		.all(|byte| byte.is_ascii_alphanumeric() || b" -".contains(&byte))
	{
		return Err("Use 1-32 letters, digits, spaces or hyphens for the layout name".into());
	}
	let normalized = label
		.split_ascii_whitespace()
		.collect::<Vec<_>>()
		.join(" ")
		.to_ascii_uppercase();
	if normalized.is_empty() {
		return Err("Enter a layout name".into());
	}
	Ok(normalized)
}
