//! Profile-scoped Practice slot, transfer file admission, and reviewed publication.

use super::{
	Checkpoint, MAX_CHECKPOINT_BYTES, PreflightBlock,
	audit::{Direction, MAX_AUDIT_BYTES, MAX_ROWS, Receipt, export_bytes},
	sha256,
};
use crate::local_player_files::{
	lock, ordinary_directory, ordinary_file, ordinary_transfer, profile_directory, read_bounded,
};
use runtime::scene_io::{replace_bytes, save_new_bytes};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
	path::{Path, PathBuf},
	sync::atomic::{AtomicU64, Ordering},
	time::{SystemTime, UNIX_EPOCH},
};

const SLOT_SCHEMA: u32 = 1;
const PROFILE_DIRECTORY: &[&str] = &["GameEngine", "Cinder Oath Practice"];
const TRANSFER_DIRECTORY: &[&str] = &["GameEngine", "Checkpoint Transfers"];
const MAX_SLOT_BYTES: usize = MAX_AUDIT_BYTES + MAX_CHECKPOINT_BYTES;

#[derive(Clone, Debug)]
pub(crate) struct TransferFile {
	directory: PathBuf,
	pub(crate) name: String,
	suffix: &'static str,
}

impl TransferFile {
	pub(crate) fn new(directory: &Path, name: String, suffix: &'static str) -> Self {
		Self {
			directory: directory.to_owned(),
			name,
			suffix,
		}
	}

	pub(crate) fn path(&self) -> Result<PathBuf, String> {
		let directory = ordinary_directory(&self.directory, false, false)?
			.filter(|path| path == &self.directory)
			.ok_or("Transfer folder changed. Open the file picker again.")?;
		let path = directory.join(&self.name);
		if path.parent() != Some(directory.as_path())
			|| ordinary_transfer(&path, self.suffix)?.as_deref() != Some(self.name.as_str())
		{
			return Err("Selected transfer file is unavailable".into());
		}
		Ok(path)
	}

	fn checkpoint_bytes(&self) -> Result<Vec<u8>, PreflightBlock> {
		let invalid = PreflightBlock::InvalidArchive;
		let path = self.path().map_err(|_| invalid)?;
		read_bounded(&path, MAX_CHECKPOINT_BYTES)
			.map_err(|_| invalid)?
			.ok_or(invalid)
	}
}

pub(crate) enum Preflight {
	Ready(Box<Review>),
	Blocked(PreflightBlock),
}

pub(crate) struct Review {
	source: TransferFile,
	pub(crate) archive: Checkpoint,
	pub(crate) source_sha256: [u8; 32],
	pub(crate) destination_sha256: [u8; 32],
	pub(crate) replaces: bool,
}

impl Review {
	pub(crate) fn filename(&self) -> &str {
		&self.source.name
	}
}

pub(crate) enum Attempt {
	Committed,
	NoSlotWrite(StaleTransfer),
	InspectSlot(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StaleTransfer {
	Archive,
	Destination,
	InstalledArena(PreflightBlock),
}

impl StaleTransfer {
	pub(crate) fn label(self) -> &'static str {
		match self {
			Self::Archive => "Checkpoint file changed",
			Self::Destination => "Practice slot changed",
			Self::InstalledArena(problem) => problem.label(),
		}
	}
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SlotRecord {
	schema_version: u32,
	generation: u64,
	checkpoint: Option<Checkpoint>,
	entries: Vec<Receipt>,
}

impl Default for SlotRecord {
	fn default() -> Self {
		Self {
			schema_version: SLOT_SCHEMA,
			generation: 0,
			checkpoint: None,
			entries: Vec::new(),
		}
	}
}

impl SlotRecord {
	fn read(bytes: Option<&[u8]>) -> Result<Self, String> {
		let Some(bytes) = bytes else {
			return Ok(Self::default());
		};
		let record: Self =
			serde_json::from_slice(bytes).map_err(|_| "Practice slot data is invalid")?;
		if record.schema_version != SLOT_SCHEMA
			|| record.generation == 0
			|| !record
				.checkpoint
				.as_ref()
				.is_none_or(Checkpoint::is_cinder_entry)
			|| record.entries.len() > MAX_ROWS
			|| !record.entries.iter().all(Receipt::is_cinder)
		{
			return Err("Practice slot data is invalid".into());
		}
		Ok(record)
	}

	fn append(&mut self, direction: Direction) {
		self.entries.insert(0, Receipt::cinder(direction));
		self.entries.truncate(MAX_ROWS);
	}

	fn publish(mut self, path: &Path) -> Result<(), String> {
		self.generation = self
			.generation
			.checked_add(1)
			.ok_or("Practice slot generation is full")?;
		let bytes = serde_json::to_vec_pretty(&self).map_err(|error| error.to_string())?;
		if bytes.len() > MAX_SLOT_BYTES {
			return Err("Practice slot exceeded its file limit".into());
		}
		ordinary_file(path)?;
		replace_bytes(path, &bytes)
	}
}

pub(crate) struct PracticeStore {
	root: PathBuf,
	install_key: String,
}

impl PracticeStore {
	pub(crate) fn new(profile: &Path, manifest: &Path) -> Result<Self, String> {
		let root = ordinary_directory(profile, false, false)?.ok_or("No local Player profile")?;
		let manifest = manifest
			.canonicalize()
			.map_err(|_| "Cannot identify installed project")?;
		let install_key = format!(
			"{:x}",
			Sha256::digest(manifest.as_os_str().as_encoded_bytes())
		);
		Ok(Self { root, install_key })
	}

	pub(crate) fn transfer_directory(
		&self,
		configured: Option<&Path>,
		create: bool,
	) -> Result<Option<PathBuf>, String> {
		if let Some(path) = configured {
			return ordinary_directory(path, false, false);
		}
		profile_directory(&self.root, TRANSFER_DIRECTORY, create)
	}

	pub(crate) fn checkpoint(&self) -> Result<Option<Checkpoint>, String> {
		Ok(self.load()?.1.checkpoint)
	}

	pub(crate) fn save(&self, checkpoint: Checkpoint) -> Result<(), String> {
		self.update(|record| record.checkpoint = Some(checkpoint))
	}

	pub(crate) fn record_export(&self) -> Result<(), String> {
		self.update(|record| record.append(Direction::Export))
	}

	pub(crate) fn audit(&self, direction: Option<Direction>) -> Result<Vec<u8>, String> {
		export_bytes(&self.load()?.1.entries, direction)
	}

	pub(crate) fn preflight(
		&self,
		file: &TransferFile,
		arena: impl FnOnce() -> Result<Checkpoint, PreflightBlock>,
	) -> Preflight {
		match self.preview(file, arena) {
			Ok(review) => Preflight::Ready(Box::new(review)),
			Err(problem) => Preflight::Blocked(problem),
		}
	}

	fn preview(
		&self,
		file: &TransferFile,
		arena: impl FnOnce() -> Result<Checkpoint, PreflightBlock>,
	) -> Result<Review, PreflightBlock> {
		let bytes = file.checkpoint_bytes()?;
		let archive: Checkpoint =
			serde_json::from_slice(&bytes).map_err(|_| PreflightBlock::InvalidArchive)?;
		if !archive.is_well_formed() {
			return Err(PreflightBlock::InvalidArchive);
		}
		archive.check_compatibility(&arena()?)?;
		let (destination_sha256, destination) = self
			.load()
			.map_err(|_| PreflightBlock::DestinationUnavailable)?;
		Ok(Review {
			source: file.clone(),
			archive,
			source_sha256: sha256(&bytes),
			destination_sha256,
			replaces: destination.checkpoint.is_some(),
		})
	}

	pub(crate) fn import(
		&self,
		review: &Review,
		arena: impl FnOnce() -> Result<Checkpoint, PreflightBlock>,
	) -> Attempt {
		let verify_source = || {
			review
				.source
				.checkpoint_bytes()
				.is_ok_and(|bytes| sha256(&bytes) == review.source_sha256)
		};
		if !verify_source() {
			return Attempt::NoSlotWrite(StaleTransfer::Archive);
		}
		if !self
			.load()
			.is_ok_and(|(digest, _)| digest == review.destination_sha256)
		{
			return Attempt::NoSlotWrite(StaleTransfer::Destination);
		}
		let (path, _guard) = match self.lock_record() {
			Ok(record) => record,
			Err(_) => return Attempt::NoSlotWrite(StaleTransfer::Destination),
		};
		let (digest, mut record) = match self.read_at(Some(&path)) {
			Ok(value) => value,
			Err(_) => return Attempt::NoSlotWrite(StaleTransfer::Destination),
		};
		if digest != review.destination_sha256 {
			return Attempt::NoSlotWrite(StaleTransfer::Destination);
		}
		if let Err(problem) = arena().and_then(|arena| review.archive.check_compatibility(&arena)) {
			return Attempt::NoSlotWrite(StaleTransfer::InstalledArena(problem));
		}
		if !verify_source() {
			return Attempt::NoSlotWrite(StaleTransfer::Archive);
		}

		record.checkpoint = Some(review.archive.clone());
		record.append(Direction::Import);
		match record.publish(&path) {
			Ok(()) => Attempt::Committed,
			Err(error) => Attempt::InspectSlot(error),
		}
	}

	fn update(&self, edit: impl FnOnce(&mut SlotRecord)) -> Result<(), String> {
		let (path, _guard) = self.lock_record()?;
		let (_, mut record) = self.read_at(Some(&path))?;
		edit(&mut record);
		record.publish(&path)
	}

	fn lock_record(&self) -> Result<(PathBuf, std::fs::File), String> {
		let path = self.path(true)?.ok_or("Practice slot folder unavailable")?;
		let guard = lock(&path.with_extension("lock"))?;
		Ok((path, guard))
	}

	fn load(&self) -> Result<([u8; 32], SlotRecord), String> {
		self.read_at(self.path(false)?.as_deref())
	}

	fn path(&self, create: bool) -> Result<Option<PathBuf>, String> {
		if ordinary_directory(&self.root, false, false)?.as_deref() != Some(self.root.as_path()) {
			return Err("Local Player profile has changed".into());
		}
		Ok(profile_directory(&self.root, PROFILE_DIRECTORY, create)?
			.map(|folder| folder.join(format!("{}-practice.json", self.install_key))))
	}

	fn read_at(&self, path: Option<&Path>) -> Result<([u8; 32], SlotRecord), String> {
		let bytes = match path {
			Some(path) => read_bounded(path, MAX_SLOT_BYTES)?,
			None => None,
		};
		let mut digest = Sha256::new();
		digest.update(b"game-engine-cinder-practice-slot-v1");
		for field in [
			self.root.as_os_str().as_encoded_bytes(),
			self.install_key.as_bytes(),
		] {
			digest.update((field.len() as u64).to_le_bytes());
			digest.update(field);
		}
		if let Some(bytes) = &bytes {
			digest.update(bytes);
		}
		Ok((
			digest.finalize().into(),
			SlotRecord::read(bytes.as_deref())?,
		))
	}
}

pub(crate) fn publish_transfer(
	directory: &Path,
	prefix: &str,
	suffix: &str,
	bytes: &[u8],
) -> Result<(), String> {
	static NEXT_EXPORT: AtomicU64 = AtomicU64::new(0);
	let elapsed = SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map_err(|_| "System clock is before the Unix epoch")?
		.as_millis();
	let nonce = NEXT_EXPORT.fetch_add(1, Ordering::Relaxed);
	let pid = std::process::id();
	let path = directory.join(format!("{prefix}-{elapsed}-{pid}-{nonce}{suffix}"));
	save_new_bytes(&path, bytes)
}
