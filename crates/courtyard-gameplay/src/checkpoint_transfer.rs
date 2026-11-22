//! A bounded transfer format for one current authored-release checkpoint.

use super::{
	CheckpointRegistry, CheckpointSave, ContinueAvailability, MAX_SAVE_BYTES, ProjectData,
	SaveSlot, SceneData, SceneId, Sha256, SlotFiles, SlotGeneration, lock_slot, publish_record,
	storage::read_bounded_record,
};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::path::Path;

const MAX_TRANSFER_BYTES: usize = 32 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImportIntent {
	EmptyOnly,
	ReplaceCurrent,
}

/// Tokens from a read-only preview. Both must still match at import.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportReview {
	archive: [u8; 32],
	destination_generation: [u8; 32],
}

impl ImportReview {
	pub fn from_hex(archive: &str, destination_generation: &str) -> Result<Self, String> {
		Ok(Self {
			archive: decode_digest(archive)?,
			destination_generation: decode_digest(destination_generation)?,
		})
	}
}

/// A reviewed archive and its intended local destination.
pub struct CheckpointImport<'a> {
	pub profile: &'a Path,
	pub slot: SaveSlot,
	pub archive: &'a Path,
	pub review: &'a ImportReview,
	pub intent: ImportIntent,
	pub project: &'a ProjectData,
}

#[derive(Debug, Serialize)]
pub struct ImportPreview {
	pub archive_sha256: String,
	pub destination_generation_sha256: String,
	pub project_save_id: String,
	pub checkpoint_id: String,
	pub chapter_label: Option<String>,
	pub destination: &'static str,
	pub replacement_required: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TransferEnvelope {
	schema_version: u32,
	project_save_id: String,
	record_sha256: String,
	record_utf8: String,
}

impl CheckpointRegistry {
	/// Read the exact generation token without decoding or reconstructing the scene.
	pub fn slot_transfer_generation(
		&self,
		profile: &Path,
		slot: SaveSlot,
	) -> Result<SlotGeneration, String> {
		Ok(self.read_generations(profile, slot)?.generation(slot))
	}

	/// Read one selected primary generation and return its portable envelope.
	pub fn export_slot_transfer(
		&self,
		profile: &Path,
		slot: SaveSlot,
		expected: &SlotGeneration,
		project: &ProjectData,
		load_authored: impl FnMut(&SceneId) -> Result<SceneData, String>,
	) -> Result<Vec<u8>, String> {
		self.export_slot_transfer_with_checkpoint(profile, slot, expected, project, load_authored)
			.map(|(envelope, _)| envelope)
	}

	/// Return portable bytes and the identifier of that exact validated record.
	pub fn export_slot_transfer_with_checkpoint(
		&self,
		profile: &Path,
		slot: SaveSlot,
		expected: &SlotGeneration,
		project: &ProjectData,
		load_authored: impl FnMut(&SceneId) -> Result<SceneData, String>,
	) -> Result<(Vec<u8>, String), String> {
		let files = self.read_generations(profile, slot)?;
		files.verify(slot, expected)?;
		let record = files.primary.ok_or("Select an occupied checkpoint slot")?;
		let saved = self.current_record(project, &record, load_authored)?;
		let envelope = TransferEnvelope {
			schema_version: 1,
			project_save_id: self.project_save_id().to_owned(),
			record_sha256: encode_digest(&digest(&record)),
			record_utf8: String::from_utf8(record).map_err(|_| "Checkpoint is not UTF-8")?,
		};
		let bytes = serde_json::to_vec(&envelope).map_err(|error| error.to_string())?;
		if bytes.len() > MAX_TRANSFER_BYTES {
			return Err("Checkpoint transfer exceeds its size limit".into());
		}
		Ok((bytes, saved.checkpoint_id().to_owned()))
	}

	/// Validate the transfer and current destination without creating or changing files.
	pub fn preview_slot_transfer(
		&self,
		profile: &Path,
		slot: SaveSlot,
		path: &Path,
		project: &ProjectData,
		mut load_authored: impl FnMut(&SceneId) -> Result<SceneData, String>,
	) -> Result<ImportPreview, String> {
		let (archive, _, saved) = self.read_transfer(path, project, &mut load_authored)?;
		let files = self.read_generations(profile, slot)?;
		let occupied = self.validate_destination(&files, project, load_authored)?;
		let chapter_label = self.chapter_label(saved.scene_id());
		Ok(ImportPreview {
			archive_sha256: encode_digest(&archive),
			destination_generation_sha256: encode_digest(
				&self.transfer_destination_token(profile, slot, &files)?,
			),
			project_save_id: self.project_save_id().into(),
			checkpoint_id: saved.checkpoint_id().into(),
			chapter_label,
			destination: slot.label(),
			replacement_required: occupied,
		})
	}

	/// Import the reviewed bytes under the same process lock used by the Save action.
	pub fn import_slot_transfer(
		&self,
		request: CheckpointImport<'_>,
		load_authored: impl FnMut(&SceneId) -> Result<SceneData, String>,
	) -> Result<(), String> {
		self.import_slot_transfer_with_checkpoint(request, load_authored)
			.map(|_| ())
	}

	/// Return the exact checkpoint identifier only after the archive is published.
	pub fn import_slot_transfer_with_checkpoint(
		&self,
		request: CheckpointImport<'_>,
		load_authored: impl FnMut(&SceneId) -> Result<SceneData, String>,
	) -> Result<String, String> {
		self.publish_transfer(request, load_authored, publish_record)
	}

	pub(super) fn publish_transfer(
		&self,
		request: CheckpointImport<'_>,
		mut load_authored: impl FnMut(&SceneId) -> Result<SceneData, String>,
		mut publish: impl FnMut(&Path, &str, &[u8]) -> Result<(), String>,
	) -> Result<String, String> {
		let CheckpointImport {
			profile,
			slot,
			archive,
			review,
			intent,
			project,
		} = request;
		let (archive_digest, record, saved) =
			self.read_transfer(archive, project, &mut load_authored)?;
		if archive_digest != review.archive {
			return Err("Checkpoint transfer changed. Preview it again before importing".into());
		}
		// Reject a stale preview before even creating a missing profile directory.
		if self.transfer_destination_token(profile, slot, &self.read_generations(profile, slot)?)?
			!= review.destination_generation
		{
			return Err("Destination slot changed. Preview it again before importing".into());
		}
		let parent = self
			.slot_parent(profile, true)?
			.ok_or("Courtyard save directory is unavailable")?;
		let _lock = lock_slot(&parent, slot)?;
		let files = SlotFiles::read(&parent, slot)?;
		if self.transfer_destination_token(profile, slot, &files)? != review.destination_generation
		{
			return Err("Destination slot changed. Preview it again before importing".into());
		}
		let occupied = self.validate_destination(&files, project, load_authored)?;
		if occupied {
			if intent != ImportIntent::ReplaceCurrent {
				return Err(
					"This slot contains a checkpoint. Explicit replacement is required".into(),
				);
			}
			if let Some(primary) = files.primary.as_deref() {
				publish(&parent, slot.previous(), primary)?;
			}
		}
		publish(&parent, slot.primary(), &record)?;
		Ok(saved.checkpoint_id().to_owned())
	}

	fn read_transfer(
		&self,
		path: &Path,
		project: &ProjectData,
		load_authored: impl FnMut(&SceneId) -> Result<SceneData, String>,
	) -> Result<([u8; 32], Vec<u8>, CheckpointSave), String> {
		let bytes = read_bounded_record(path, MAX_TRANSFER_BYTES)?
			.ok_or("Checkpoint transfer file is missing")?;
		let envelope: TransferEnvelope = serde_json::from_slice(&bytes)
			.map_err(|error| format!("Invalid checkpoint transfer: {error}"))?;
		if envelope.schema_version != 1 || envelope.project_save_id != self.project_save_id() {
			return Err(
				"Checkpoint transfer belongs to another project or uses an unsupported version"
					.into(),
			);
		}
		let record = envelope.record_utf8.into_bytes();
		if record.is_empty()
			|| record.len() > MAX_SAVE_BYTES
			|| decode_digest(&envelope.record_sha256)? != digest(&record)
		{
			return Err("Checkpoint transfer record has an invalid length or digest".into());
		}
		let saved = self.current_record(project, &record, load_authored)?;
		Ok((digest(&bytes), record, saved))
	}

	fn transfer_destination_token(
		&self,
		profile: &Path,
		slot: SaveSlot,
		files: &SlotFiles,
	) -> Result<[u8; 32], String> {
		let canonical_profile = profile.canonicalize().map_err(super::storage_error)?;
		Ok(files
			.generation(slot)
			.transfer_token(&canonical_profile, self.project_save_id()))
	}

	fn validate_destination(
		&self,
		files: &SlotFiles,
		project: &ProjectData,
		load_authored: impl FnMut(&SceneId) -> Result<SceneData, String>,
	) -> Result<bool, String> {
		if files.previous_error.is_some() {
			return Err("The destination backup is unavailable".into());
		}
		if let Some(primary) = files.primary.as_deref() {
			self.current_record(project, primary, load_authored)
				.map_err(|error| format!("The existing destination cannot be replaced: {error}"))?;
			return Ok(true);
		}
		if files.previous.is_some() {
			return Err(
				"The destination contains a backup. Continue or recover it before replacement"
					.into(),
			);
		}
		Ok(false)
	}

	fn current_record(
		&self,
		project: &ProjectData,
		bytes: &[u8],
		mut load_authored: impl FnMut(&SceneId) -> Result<SceneData, String>,
	) -> Result<CheckpointSave, String> {
		let saved = self.decode_save(bytes).map_err(|error| error.message())?;
		let authored = load_authored(saved.scene_id())?;
		let (_, _, status) = self.reconstruct_with_status(&saved, project, &authored)?;
		if status != ContinueAvailability::Current {
			return Err("Checkpoint transfer requires a current authored-release save".into());
		}
		Ok(saved)
	}
}

fn digest(bytes: &[u8]) -> [u8; 32] {
	Sha256::digest(bytes).into()
}

fn encode_digest(bytes: &[u8; 32]) -> String {
	use std::fmt::Write;
	let mut hex = String::with_capacity(64);
	for byte in bytes {
		let _ = write!(hex, "{byte:02x}");
	}
	hex
}

fn decode_digest(value: &str) -> Result<[u8; 32], String> {
	if value.len() != 64
		|| !value
			.bytes()
			.all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
	{
		return Err(
			"Checkpoint transfer tokens require exactly 64 lowercase hexadecimal digits".into(),
		);
	}
	let mut bytes = [0; 32];
	for (index, byte) in bytes.iter_mut().enumerate() {
		*byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
			.map_err(|error| error.to_string())?;
	}
	Ok(bytes)
}
