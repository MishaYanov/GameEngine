//! Bounded slot files and process-level publication ownership.

use std::{
	fs::{self, File, OpenOptions},
	io::{self, Read, Write},
	path::Path,
	sync::atomic::{AtomicU64, Ordering},
};

use sha2::{Digest, Sha256};

use super::{ContinueAvailability, MAX_SAVE_BYTES};

/// A fixed file identity within one Courtyard profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveSlot {
	One,
	Two,
	Three,
}

impl SaveSlot {
	pub const ALL: [Self; 3] = [Self::One, Self::Two, Self::Three];

	pub fn label(self) -> &'static str {
		match self {
			Self::One => "Slot 1",
			Self::Two => "Slot 2",
			Self::Three => "Slot 3",
		}
	}

	pub(super) fn primary(self) -> &'static str {
		match self {
			Self::One => "slot.json",
			Self::Two => "slot-2.json",
			Self::Three => "slot-3.json",
		}
	}

	pub(super) fn previous(self) -> &'static str {
		match self {
			Self::One => "slot.previous.json",
			Self::Two => "slot-2.previous.json",
			Self::Three => "slot-3.previous.json",
		}
	}

	fn lock_file(self) -> &'static str {
		match self {
			Self::One => "slot.lock",
			Self::Two => "slot-2.lock",
			Self::Three => "slot-3.lock",
		}
	}
}

/// The exact primary and previous generations observed in a selected slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotGeneration {
	slot: SaveSlot,
	primary: Option<[u8; 32]>,
	previous: Option<[u8; 32]>,
	previous_unavailable: bool,
}

impl SlotGeneration {
	/// A readable primary can still be Continued when its backup is unavailable.
	pub fn can_save(self) -> bool {
		!self.previous_unavailable
	}

	pub(super) fn transfer_token(self, profile: &Path, project_save_id: &str) -> [u8; 32] {
		let mut hasher = Sha256::new();
		hasher.update(b"courtyard-portable-slot-generation-v1");
		for identity in [
			profile.as_os_str().as_encoded_bytes(),
			project_save_id.as_bytes(),
		] {
			hasher.update((identity.len() as u64).to_le_bytes());
			hasher.update(identity);
		}
		hasher.update([match self.slot {
			SaveSlot::One => 1,
			SaveSlot::Two => 2,
			SaveSlot::Three => 3,
		}]);
		for (generation, unavailable) in [
			(self.primary, false),
			(self.previous, self.previous_unavailable),
		] {
			match generation {
				Some(hash) => {
					hasher.update([1]);
					hasher.update(hash);
				}
				None => hasher.update([if unavailable { 2 } else { 0 }]),
			}
		}
		hasher.finalize().into()
	}
}

/// Read-only save and recovery status for a slot picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotInspection {
	pub primary: ContinueAvailability,
	pub checkpoint_id: Option<String>,
	pub recovery_checkpoint_id: Option<String>,
	pub chapter_label: Option<String>,
	pub recovery_chapter_label: Option<String>,
	pub generation: Option<SlotGeneration>,
}

impl Default for SlotInspection {
	fn default() -> Self {
		Self {
			primary: ContinueAvailability::Unreadable,
			checkpoint_id: None,
			recovery_checkpoint_id: None,
			chapter_label: None,
			recovery_chapter_label: None,
			generation: None,
		}
	}
}

/// An explicit request to Continue or Recover, respectively.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreSource {
	Current,
	Previous,
}

#[derive(Default)]
pub(super) struct SlotFiles {
	pub(super) primary: Option<Vec<u8>>,
	pub(super) previous: Option<Vec<u8>>,
	pub(super) previous_error: Option<String>,
}

impl SlotFiles {
	pub(super) fn read(parent: &Path, slot: SaveSlot) -> Result<Self, String> {
		let primary = read_record(&parent.join(slot.primary()))?;
		let (previous, previous_error) = match read_record(&parent.join(slot.previous())) {
			Ok(bytes) => (bytes, None),
			Err(error) => (None, Some(error)),
		};
		Ok(Self {
			primary,
			previous,
			previous_error,
		})
	}

	pub(super) fn generation(&self, slot: SaveSlot) -> SlotGeneration {
		SlotGeneration {
			slot,
			primary: self.primary.as_deref().map(record_digest),
			previous: self.previous.as_deref().map(record_digest),
			previous_unavailable: self.previous_error.is_some(),
		}
	}

	pub(super) fn verify(&self, slot: SaveSlot, expected: &SlotGeneration) -> Result<(), String> {
		if self.generation(slot) != *expected {
			return Err("Courtyard save slot changed. Inspect it again before proceeding".into());
		}
		Ok(())
	}
}

pub(super) fn is_ordinary(metadata: &fs::Metadata, directory: bool) -> bool {
	#[cfg(windows)]
	{
		use std::os::windows::fs::MetadataExt;
		const REPARSE_POINT: u32 = 0x400;
		if metadata.file_attributes() & REPARSE_POINT != 0 {
			return false;
		}
	}
	if directory {
		metadata.is_dir()
	} else {
		metadata.is_file()
	}
}

fn ordinary_file_if_present(path: &Path) -> Result<bool, String> {
	match fs::symlink_metadata(path) {
		Ok(metadata) if is_ordinary(&metadata, false) => Ok(true),
		Ok(_) => Err("Courtyard save slot must be an ordinary file".into()),
		Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
		Err(error) => Err(storage_error(error)),
	}
}

pub(super) fn read_record(path: &Path) -> Result<Option<Vec<u8>>, String> {
	read_bounded_record(path, MAX_SAVE_BYTES)
}

pub(super) fn read_bounded_record(path: &Path, limit: usize) -> Result<Option<Vec<u8>>, String> {
	if !ordinary_file_if_present(path)? {
		return Ok(None);
	}
	let mut options = OpenOptions::new();
	options.read(true);
	let file = open_ordinary(&mut options, path)?;
	let mut bytes = Vec::new();
	file.take((limit + 1) as u64)
		.read_to_end(&mut bytes)
		.map_err(storage_error)?;
	if bytes.len() > limit {
		return Err("Courtyard save exceeds its size limit".into());
	}
	Ok(Some(bytes))
}

pub(super) fn lock_slot(parent: &Path, slot: SaveSlot) -> Result<File, String> {
	let path = parent.join(slot.lock_file());
	ordinary_file_if_present(&path)?;
	let mut options = OpenOptions::new();
	options.read(true).write(true).create(true).truncate(false);
	let file = open_ordinary(&mut options, &path)?;
	// Holding the handle keeps the OS lock until the save completes or the process exits.
	file.try_lock()
		.map_err(|error| format!("Could not lock Courtyard save slot: {error}"))?;
	Ok(file)
}

fn open_ordinary(options: &mut OpenOptions, path: &Path) -> Result<File, String> {
	#[cfg(windows)]
	{
		use std::os::windows::fs::OpenOptionsExt;
		const OPEN_REPARSE_POINT: u32 = 0x0020_0000;
		options.custom_flags(OPEN_REPARSE_POINT);
	}
	let file = options.open(path).map_err(storage_error)?;
	if !is_ordinary(&file.metadata().map_err(storage_error)?, false) {
		return Err("Courtyard slot is not an ordinary file".into());
	}
	Ok(file)
}

pub(super) fn publish_record(parent: &Path, name: &str, bytes: &[u8]) -> Result<(), String> {
	static NEXT: AtomicU64 = AtomicU64::new(0);
	let destination = parent.join(name);
	ordinary_file_if_present(&destination)?;
	let temporary = parent.join(format!(
		".slot-{}-{}.next",
		std::process::id(),
		NEXT.fetch_add(1, Ordering::Relaxed),
	));
	let mut file = OpenOptions::new()
		.write(true)
		.create_new(true)
		.open(&temporary)
		.map_err(storage_error)?;
	let write = file.write_all(bytes).and_then(|()| file.sync_all());
	drop(file);
	let result = write.and_then(|()| fs::rename(&temporary, &destination));
	if let Err(error) = result {
		let _ = fs::remove_file(&temporary);
		return Err(storage_error(error));
	}
	Ok(())
}

pub(super) fn storage_error(error: io::Error) -> String {
	format!("Courtyard save storage: {error}")
}

fn record_digest(bytes: &[u8]) -> [u8; 32] {
	Sha256::digest(bytes).into()
}
