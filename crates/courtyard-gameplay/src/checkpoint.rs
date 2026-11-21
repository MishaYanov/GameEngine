//! Versioned local Courtyard checkpoints and fresh-scene reconstruction.

use std::{
	collections::HashSet,
	fs::{self, File, OpenOptions},
	io::{self, Read, Write},
	path::{Path, PathBuf},
	sync::atomic::{AtomicU64, Ordering},
};

use engine_core::{
	EntityKey, SceneData,
	collision::query_overlaps,
	game_progress::GameProgress,
	project::ProjectData,
	scene_catalog::SceneId,
	scene_data::{EntityData, MaterialOverrideData},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{
	COURTYARD_RUN_ID, CampaignProgress,
	settings::{InteractionSettings, SceneInteractions},
};

#[path = "checkpoint_compatibility.rs"]
mod compatibility;

const SAVE_VERSION: u32 = 1;
const MAX_SAVE_BYTES: usize = 4096;
const MAX_CHECKPOINTS: usize = 128;

/// Compatibility of a local slot with the selected authored release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContinueAvailability {
	Empty,
	Current,
	CompatibleEarlierRelease,
	IncompatibleRelease,
	Unreadable,
}

impl ContinueAvailability {
	pub fn can_continue(self) -> bool {
		matches!(self, Self::Current | Self::CompatibleEarlierRelease)
	}
}

enum SlotReadError {
	Unreadable(String),
	Incompatible(String),
}

impl From<String> for SlotReadError {
	fn from(error: String) -> Self {
		Self::Unreadable(error)
	}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckpointDef {
	pub id: String,
	pub scene_id: SceneId,
	pub player: EntityKey,
	pub spawn: [f32; 3],
	pub activation_radius: f32,
	#[serde(default)]
	pub requires_key: bool,
	#[serde(default)]
	pub requires_gate: bool,
}

impl CheckpointDef {
	fn admits(&self, progress: &CampaignProgress) -> bool {
		!progress.completed
			&& !progress.courtyard_exit_reached
			&& (!self.requires_key || progress.has_courtyard_key)
			&& (!self.requires_gate || progress.route_gate_open)
	}

	fn admits_position(&self, position: [f32; 3]) -> bool {
		distance_squared(self.spawn, position) <= f64::from(self.activation_radius).powi(2)
	}
}

pub struct CheckpointRegistry {
	project_save_id: String,
	definitions: Vec<CheckpointDef>,
}

impl CheckpointRegistry {
	pub fn from_project(project: &ProjectData) -> Result<Option<Self>, String> {
		if project.gameplay_module.as_deref() != Some(COURTYARD_RUN_ID) {
			return Ok(None);
		}
		let settings = InteractionSettings::from_project(project)?;
		Self::from_settings(project, settings.as_ref())
	}

	pub(crate) fn from_settings(
		project: &ProjectData,
		settings: Option<&InteractionSettings>,
	) -> Result<Option<Self>, String> {
		let Some(settings) = settings else {
			return Ok(None);
		};
		let Some(project_save_id) = settings.project_save_id.as_ref() else {
			if !settings.checkpoints.is_empty() {
				return Err("Courtyard checkpoints require project_save_id".into());
			}
			return Ok(None);
		};
		if !canonical_uuid(project_save_id) {
			return Err("Courtyard project_save_id must be a lowercase canonical UUID".into());
		}

		if settings.checkpoints.len() > MAX_CHECKPOINTS {
			return Err(format!(
				"Courtyard supports at most {MAX_CHECKPOINTS} checkpoints"
			));
		}
		let mut ids = HashSet::with_capacity(settings.checkpoints.len());
		for (index, checkpoint) in settings.checkpoints.iter().enumerate() {
			if checkpoint.id.is_empty()
				|| checkpoint.id.len() > 64
				|| !checkpoint.id.bytes().all(|byte| {
				byte.is_ascii_lowercase()
					|| byte.is_ascii_digit()
					|| matches!(byte, b'_' | b'-')
			}) || !ids.insert(checkpoint.id.as_str())
			{
				return Err(
					"Checkpoint IDs must be unique and use 1–64 lowercase identifier bytes".into(),
				);
			}
			project.scenes.get(&checkpoint.scene_id).map_err(|_| {
				format!("Checkpoint {:?} references an unknown scene", checkpoint.id)
			})?;
			checkpoint.player.validate()?;
			if !matches!(
				settings.scenes.get(checkpoint.scene_id.as_str()),
				Some(SceneInteractions::Course { .. })
			) {
				return Err("Courtyard checkpoints require a configured course scene".into());
			}
			if !checkpoint
				.spawn
				.into_iter()
				.all(|axis| axis.is_finite() && axis.abs() <= 1_000_000.0)
				|| !checkpoint.activation_radius.is_finite()
				|| !(0.01..=8.0).contains(&checkpoint.activation_radius)
			{
				return Err(
					"Checkpoint spawn and activation radius exceed the supported bounds".into(),
				);
			}
			if checkpoint.requires_gate
				&& (!checkpoint.requires_key
				|| settings
				.route_gate
				.as_ref()
				.is_none_or(|route| route.scene_id != checkpoint.scene_id))
			{
				return Err(
					"A gate checkpoint must require the key and a route gate in its scene".into(),
				);
			}
			if settings.checkpoints[..index].iter().any(|previous| {
				previous.scene_id == checkpoint.scene_id
					&& distance_squared(previous.spawn, checkpoint.spawn)
					<= (f64::from(previous.activation_radius)
					+ f64::from(checkpoint.activation_radius))
					.powi(2)
			}) {
				return Err(
					"Checkpoint activation areas in the same scene must not overlap".into(),
				);
			}
		}
		Ok(Some(Self {
			project_save_id: project_save_id.clone(),
			definitions: settings.checkpoints.clone(),
		}))
	}

	pub fn project_save_id(&self) -> &str {
		&self.project_save_id
	}

	/// Select a checkpoint from the current committed Play document and facts.
	pub fn active(
		&self,
		scene_id: &SceneId,
		live: &SceneData,
		progress: &CampaignProgress,
	) -> Option<&CheckpointDef> {
		self.definitions.iter().find(|checkpoint| {
			&checkpoint.scene_id == scene_id
				&& checkpoint.admits(progress)
				&& live.bindings.get("player") == Some(&checkpoint.player)
				&& live.entities.iter().any(|entity| {
				entity.key == checkpoint.player
					&& entity.parent.is_none()
					&& checkpoint.admits_position(entity.transform.position)
			})
		})
	}

	/// Publish one durable slot beneath an existing, trusted local profile root.
	pub fn save(
		&self,
		profile_root: &Path,
		checkpoint: &CheckpointDef,
		project: &ProjectData,
		authored: &SceneData,
		progress: &CampaignProgress,
	) -> Result<(), String> {
		let definition = self.definition(&checkpoint.id, &checkpoint.scene_id)?;
		self.restore_document(definition, project, authored, progress)?;
		let saved = CheckpointSave {
			version: SAVE_VERSION,
			project_save_id: self.project_save_id.clone(),
			checkpoint_id: definition.id.clone(),
			scene_id: definition.scene_id.clone(),
			content: content_digest(project, authored)?,
			facts: SavedFacts {
				score: progress.score,
				has_key: progress.has_courtyard_key,
				route_gate_open: progress.route_gate_open,
			},
		};
		let checksum = save_checksum(&saved)?;
		let bytes = serde_json::to_vec(&SaveEnvelope { saved, checksum })
			.map_err(|error| error.to_string())?;
		if bytes.len() > MAX_SAVE_BYTES {
			return Err("Courtyard save exceeds its size limit".into());
		}
		let parent = self
			.slot_parent(profile_root, true)?
			.ok_or("Courtyard save directory is unavailable")?;
		write_slot(&parent, &bytes)
	}

	pub fn load(&self, profile_root: &Path) -> Result<Option<CheckpointSave>, String> {
		self.read_slot(profile_root).map_err(|error| match error {
			SlotReadError::Unreadable(message) | SlotReadError::Incompatible(message) => message,
		})
	}

	/// Inspect the current single slot without changing it or preparing a run.
	pub fn inspect(
		&self,
		profile_root: &Path,
		project: &ProjectData,
		load_authored: impl FnOnce(&SceneId) -> Result<SceneData, String>,
	) -> ContinueAvailability {
		let saved = match self.read_slot(profile_root) {
			Ok(Some(saved)) => saved,
			Ok(None) => return ContinueAvailability::Empty,
			Err(SlotReadError::Unreadable(_)) => return ContinueAvailability::Unreadable,
			Err(SlotReadError::Incompatible(_)) => {
				return ContinueAvailability::IncompatibleRelease;
			}
		};
		let authored = match load_authored(saved.scene_id()) {
			Ok(authored) => authored,
			Err(_) => return ContinueAvailability::Unreadable,
		};
		match content_digest(project, &authored) {
			Ok(content) => Self::compatibility(&saved, content),
			Err(_) => ContinueAvailability::Unreadable,
		}
	}

	fn read_slot(&self, profile_root: &Path) -> Result<Option<CheckpointSave>, SlotReadError> {
		let Some(parent) = self.slot_parent(profile_root, false)? else {
			return Ok(None);
		};
		let path = parent.join("slot.json");
		if !ordinary_file_if_present(&path)? {
			return Ok(None);
		}
		let file = open_slot(&path)?;
		if !is_ordinary(&file.metadata().map_err(storage_error)?, false) {
			return Err(SlotReadError::Unreadable(
				"Courtyard slot is not an ordinary file".into(),
			));
		}
		let mut bytes = Vec::new();
		file.take((MAX_SAVE_BYTES + 1) as u64)
			.read_to_end(&mut bytes)
			.map_err(storage_error)?;
		if bytes.len() > MAX_SAVE_BYTES {
			return Err(SlotReadError::Unreadable(
				"Courtyard save exceeds its size limit".into(),
			));
		}
		let record: SaveEnvelope = serde_json::from_slice(&bytes)
			.map_err(|error| format!("Invalid Courtyard save: {error}"))?;
		if record.saved.version != SAVE_VERSION {
			return Err(SlotReadError::Incompatible(
				"Unsupported Courtyard save version".into(),
			));
		}
		if record.checksum != save_checksum(&record.saved)? {
			return Err(SlotReadError::Unreadable(
				"Courtyard save checksum does not match".into(),
			));
		}
		self.saved_definition(&record.saved)
			.map_err(SlotReadError::Incompatible)?;
		Ok(Some(record.saved))
	}

	pub fn reconstruct(
		&self,
		saved: &CheckpointSave,
		project: &ProjectData,
		authored: &SceneData,
	) -> Result<(SceneData, CampaignProgress), String> {
		self.reconstruct_with_status(saved, project, authored)
			.map(|(document, progress, _)| (document, progress))
	}

	pub fn reconstruct_with_status(
		&self,
		saved: &CheckpointSave,
		project: &ProjectData,
		authored: &SceneData,
	) -> Result<(SceneData, CampaignProgress, ContinueAvailability), String> {
		let checkpoint = self.saved_definition(saved)?;
		let status = Self::compatibility(saved, content_digest(project, authored)?);
		if !status.can_continue() {
			return Err(
				"Courtyard save is incompatible with the current authored project or scene".into(),
			);
		}
		let progress = CampaignProgress {
			score: saved.facts.score,
			has_courtyard_key: saved.facts.has_key,
			route_gate_open: saved.facts.route_gate_open,
			..CampaignProgress::default()
		};
		let document = self.restore_document(checkpoint, project, authored, &progress)?;
		Ok((document, progress, status))
	}

	fn saved_definition(&self, saved: &CheckpointSave) -> Result<&CheckpointDef, String> {
		if saved.version != SAVE_VERSION {
			return Err("Unsupported Courtyard save version".into());
		}
		if saved.project_save_id != self.project_save_id {
			return Err("Courtyard save belongs to a different project".into());
		}
		self.definition(&saved.checkpoint_id, &saved.scene_id)
	}

	fn compatibility(saved: &CheckpointSave, authored_content: [u8; 32]) -> ContinueAvailability {
		if saved.content == authored_content {
			ContinueAvailability::Current
		} else if compatibility::approves_upgrade(saved, authored_content) {
			ContinueAvailability::CompatibleEarlierRelease
		} else {
			ContinueAvailability::IncompatibleRelease
		}
	}

	fn definition(&self, id: &str, scene_id: &SceneId) -> Result<&CheckpointDef, String> {
		self.definitions
			.iter()
			.find(|checkpoint| checkpoint.id == id && &checkpoint.scene_id == scene_id)
			.ok_or_else(|| format!("Unknown Courtyard checkpoint {id:?}"))
	}

	fn restore_document(
		&self,
		checkpoint: &CheckpointDef,
		project: &ProjectData,
		authored: &SceneData,
		progress: &CampaignProgress,
	) -> Result<SceneData, String> {
		progress.validate()?;
		if !checkpoint.admits(progress)
			|| (progress.route_gate_open && !progress.has_courtyard_key)
			|| (progress.has_courtyard_key && progress.score < 1)
		{
			return Err("Courtyard progress does not satisfy the checkpoint".into());
		}
		let settings = InteractionSettings::from_project(project)?
			.ok_or("Courtyard checkpoint has no authored settings")?;
		if settings.project_save_id.as_deref() != Some(self.project_save_id.as_str()) {
			return Err("Courtyard checkpoint project identity changed".into());
		}
		let Some(SceneInteractions::Course { exit_blocker, .. }) =
			settings.scenes.get(checkpoint.scene_id.as_str())
		else {
			return Err("Courtyard checkpoint has no authored course".into());
		};
		if authored.bindings.get("player") != Some(&checkpoint.player) {
			return Err("Checkpoint references a different player than the authored course".into());
		}
		let mut document = authored.clone();
		let player = entity_mut(&mut document, &checkpoint.player)?;
		if player.parent.is_some()
			|| !player.collision_enabled
			|| player
			.collider
			.as_ref()
			.is_none_or(|shape| shape.interaction().sensor)
		{
			return Err(
				"Checkpoint player must be a root entity with an enabled solid collider".into(),
			);
		}
		player.transform.position = checkpoint.spawn;

		if progress.has_courtyard_key {
			let key = authored.bindings.get("key").ok_or("Course key is absent")?;
			let visual = entity_mut(&mut document, key)?;
			visual.renderable = false;
			visual.collision_enabled = false;
			let exit = authored
				.bindings
				.get("exit")
				.ok_or("Course exit is absent")?;
			entity_mut(&mut document, exit)?.material = Some(MaterialOverrideData {
				base_color_factor: [0.1, 0.9, 0.25, 1.0],
			});
			if let Some(blocker) = exit_blocker {
				entity_mut(&mut document, blocker)?.collision_enabled = false;
			}
			// The authored key sensor remains valid for Play validation. The
			// first fixed step then reconciles it with the already collected key.
		}
		if progress.route_gate_open {
			let gate = settings
				.route_gate
				.as_ref()
				.filter(|route| route.scene_id == checkpoint.scene_id)
				.ok_or("Courtyard save references a gate outside the checkpoint scene")?;
			let entity = entity_mut(&mut document, &gate.gate)?;
			entity.renderable = false;
			entity.collision_enabled = false;
		}
		document.validate()?;
		if query_overlaps(&document)?.iter().any(|contact| {
			!contact.sensor
				&& contact.penetration > 0.000_1
				&& (contact.a == checkpoint.player || contact.b == checkpoint.player)
		}) {
			return Err(format!(
				"Checkpoint {:?} spawn overlaps solid geometry",
				checkpoint.id
			));
		}
		Ok(document)
	}

	fn slot_parent(&self, root: &Path, create: bool) -> Result<Option<PathBuf>, String> {
		if !root.is_absolute()
			|| !is_ordinary(&fs::symlink_metadata(root).map_err(storage_error)?, true)
		{
			return Err(
				"Courtyard profile root must be an existing ordinary absolute directory".into(),
			);
		}
		let mut parent = root.canonicalize().map_err(storage_error)?;
		for name in ["GameEngine", "checkpoints", &self.project_save_id] {
			let child = parent.join(name);
			if create {
				match fs::create_dir(&child) {
					Ok(()) => {}
					Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
					Err(error) => return Err(storage_error(error)),
				}
			}
			let metadata = match fs::symlink_metadata(&child) {
				Ok(metadata) => metadata,
				Err(error) if error.kind() == io::ErrorKind::NotFound && !create => {
					return Ok(None);
				}
				Err(error) => return Err(storage_error(error)),
			};
			if !is_ordinary(&metadata, true)
				|| child.canonicalize().map_err(storage_error)? != child
			{
				return Err("Courtyard save directory must not be redirected".into());
			}
			parent = child;
		}
		Ok(Some(parent))
	}
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckpointSave {
	version: u32,
	project_save_id: String,
	checkpoint_id: String,
	scene_id: SceneId,
	content: [u8; 32],
	facts: SavedFacts,
}

impl CheckpointSave {
	pub fn scene_id(&self) -> &SceneId {
		&self.scene_id
	}

	pub fn checkpoint_id(&self) -> &str {
		&self.checkpoint_id
	}
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedFacts {
	score: i32,
	has_key: bool,
	route_gate_open: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SaveEnvelope {
	saved: CheckpointSave,
	checksum: [u8; 32],
}

struct DigestWriter(Sha256);

impl Write for DigestWriter {
	fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
		self.0.update(buffer);
		Ok(buffer.len())
	}

	fn flush(&mut self) -> io::Result<()> {
		Ok(())
	}
}

fn content_digest(project: &ProjectData, authored: &SceneData) -> Result<[u8; 32], String> {
	let mut digest = DigestWriter(Sha256::new());
	digest.0.update(b"courtyard-checkpoint-content-v1\0");
	serde_json::to_writer(&mut digest, project).map_err(|error| error.to_string())?;
	digest.0.update(b"\0");
	serde_json::to_writer(&mut digest, authored).map_err(|error| error.to_string())?;
	Ok(digest.0.finalize().into())
}

fn save_checksum(saved: &CheckpointSave) -> Result<[u8; 32], String> {
	let mut digest = DigestWriter(Sha256::new());
	digest.0.update(b"courtyard-checkpoint-record-v1\0");
	serde_json::to_writer(&mut digest, saved).map_err(|error| error.to_string())?;
	Ok(digest.0.finalize().into())
}

fn canonical_uuid(value: &str) -> bool {
	value.len() == 36
		&& value.bytes().enumerate().all(|(index, byte)| match index {
		8 | 13 | 18 | 23 => byte == b'-',
		_ => byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte),
	})
}

fn distance_squared(first: [f32; 3], second: [f32; 3]) -> f64 {
	first
		.into_iter()
		.zip(second)
		.map(|(first, second)| (f64::from(first) - f64::from(second)).powi(2))
		.sum()
}

fn entity_mut<'a>(
	document: &'a mut SceneData,
	key: &EntityKey,
) -> Result<&'a mut EntityData, String> {
	document
		.entities
		.iter_mut()
		.find(|entity| &entity.key == key)
		.ok_or_else(|| format!("Checkpoint references a missing entity {:?}", key.as_str()))
}

fn is_ordinary(metadata: &fs::Metadata, directory: bool) -> bool {
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

fn open_slot(path: &Path) -> Result<File, String> {
	let mut options = OpenOptions::new();
	options.read(true);
	#[cfg(windows)]
	{
		use std::os::windows::fs::OpenOptionsExt;
		const OPEN_REPARSE_POINT: u32 = 0x0020_0000;
		options.custom_flags(OPEN_REPARSE_POINT);
	}
	options.open(path).map_err(storage_error)
}

fn write_slot(parent: &Path, bytes: &[u8]) -> Result<(), String> {
	static NEXT: AtomicU64 = AtomicU64::new(0);
	let destination = parent.join("slot.json");
	ordinary_file_if_present(&destination)?;
	let temporary = parent.join(format!(
		".slot-{}-{}.next",
		std::process::id(),
		NEXT.fetch_add(1, Ordering::Relaxed)
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

fn storage_error(error: io::Error) -> String {
	format!("Courtyard save storage: {error}")
}

#[cfg(test)]
#[path = "../tests/unit/checkpoint.rs"]
mod tests;
