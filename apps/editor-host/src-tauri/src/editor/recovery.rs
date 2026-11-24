//! Bounded recovery of committed authored drafts, kept beside the project manifest.
use super::{
	documents::{
		disk_fingerprint, fingerprint, read_bounded, resolve_output, same_destination,
		terminate_scene_json, verify_saved_parent, Fingerprint,
	},
	EditorSnapshotDto, EditorWorkspace, ProjectData, SceneData, SceneId,
};
use runtime::{
	project::MAX_PROJECT_BYTES, scene_catalog::MAX_PROJECT_SCENES, scene_data::MAX_SCENE_BYTES,
};
use std::{
	fs::{self, File, OpenOptions},
	io::{self, Read, Write},
	path::{Path, PathBuf},
	sync::{
		atomic::{AtomicU64, Ordering},
		Arc,
	},
	time::{SystemTime, UNIX_EPOCH},
};

const FILE_NAME: &str = ".engine-draft-recovery.json";
const FORMAT_VERSION: u32 = 1;
const MAX_RECOVERY_BYTES: usize = 32 * 1024 * 1024;

pub(super) struct RecoveryState {
	path: PathBuf,
	scope: Fingerprint,
	awaiting_review: bool,
	last_write: Option<(u64, Fingerprint)>,
	write_epoch: u64,
}

impl RecoveryState {
	pub(super) fn open(manifest: &Path) -> Self {
		let path = manifest.with_file_name(FILE_NAME);
		let awaiting_review = !matches!(
			fs::symlink_metadata(&path),
			Err(error) if error.kind() == io::ErrorKind::NotFound
		);
		Self {
			path,
			scope: fingerprint(manifest.as_os_str().as_encoded_bytes()),
			awaiting_review,
			last_write: None,
			write_epoch: 0,
		}
	}
}

enum StoredRecovery {
	Complete(Vec<u8>),
	Oversized(Fingerprint),
}

impl StoredRecovery {
	fn token(&self) -> Fingerprint {
		match self {
			Self::Complete(bytes) => fingerprint(bytes),
			Self::Oversized(token) => *token,
		}
	}

	fn checkpoint(&self) -> Result<Checkpoint, String> {
		match self {
			Self::Complete(bytes) => decode(bytes),
			Self::Oversized(_) => Err("The recovery file exceeds the 32 MiB byte limit.".into()),
		}
	}
}

#[derive(serde::Serialize)]
pub(crate) struct RecoveryOffer {
	pub token: Fingerprint,
	pub captured_at_ms: u64,
	pub files: Vec<String>,
	pub issue: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Checkpoint {
	schema_version: u32,
	scope: Fingerprint,
	base_manifest: Option<Fingerprint>,
	captured_at_ms: u64,
	active_scene: SceneId,
	project: Option<String>,
	scenes: Vec<CheckpointScene>,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CheckpointScene {
	id: SceneId,
	path: String,
	base: Option<Fingerprint>,
	document: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
	payload: Checkpoint,
	digest: Fingerprint,
}

struct RecoveryInputs {
	project: ProjectData,
	scene_bases: Vec<(String, Fingerprint)>,
}

type CapturedScene = (SceneId, String, Option<Fingerprint>, Arc<SceneData>);

pub(crate) struct RecoveryCapture {
	generation: u64,
	revision: u64,
	write_epoch: u64,
	path: PathBuf,
	payload: Checkpoint,
	project: Option<Arc<ProjectData>>,
	scenes: Vec<CapturedScene>,
}

impl RecoveryCapture {
	/// Serialization and file flushing occur off the workspace lock.
	pub(crate) fn stage(mut self) -> Result<StagedRecovery, String> {
		self.payload.project = self
			.project
			.as_deref()
			.map(ProjectData::to_json)
			.transpose()?;
		let mut bytes = self.payload.project.as_ref().map_or(0, String::len);
		for (id, path, base, scene) in self.scenes {
			let document = scene.to_json()?;
			bytes = bytes.saturating_add(document.len());
			if bytes > MAX_RECOVERY_BYTES {
				return Err("Drafts exceed the recovery checkpoint limit".into());
			}
			self.payload.scenes.push(CheckpointScene {
				id,
				path,
				base,
				document,
			});
		}
		let digest = fingerprint(&encode(&self.payload)?);
		let encoded = encode(&Envelope {
			payload: self.payload,
			digest,
		})?;
		stage_file(
			&self.path,
			&encoded,
			self.generation,
			self.revision,
			self.write_epoch,
		)
	}
}

pub(crate) struct StagedRecovery {
	temporary: PathBuf,
	path: PathBuf,
	generation: u64,
	revision: u64,
	write_epoch: u64,
	hash: Fingerprint,
}

impl Drop for StagedRecovery {
	fn drop(&mut self) {
		if verify_saved_parent(&self.temporary).is_ok() {
			let _ = fs::remove_file(&self.temporary);
		}
	}
}

impl EditorWorkspace {
	pub(crate) fn capture_recovery(&self) -> Result<Option<RecoveryCapture>, String> {
		let session = self.backend.session();
		if self.recovery.awaiting_review
			|| !session.is_dirty()
			|| session.has_transaction()
			|| session.is_playing()
			|| self
				.recovery
				.last_write
				.is_some_and(|(revision, _)| revision == session.revision())
		{
			return Ok(None);
		}
		let state = session.state();
		let project_dirty = state.project_dirty;
		let mut scenes = Vec::new();
		for state in state.scenes {
			if project_dirty || state.dirty {
				let base = self.saved_files.scene(&state.asset_path);
				let document = session.scene_document(&state.id)?;
				scenes.push((state.id, state.asset_path, base, document));
			}
		}
		Ok(Some(RecoveryCapture {
			generation: self.generation,
			revision: session.revision(),
			write_epoch: self.recovery.write_epoch,
			path: self.recovery.path.clone(),
			payload: Checkpoint {
				schema_version: FORMAT_VERSION,
				scope: self.recovery.scope,
				base_manifest: self.saved_files.manifest,
				captured_at_ms: SystemTime::now()
					.duration_since(UNIX_EPOCH)
					.unwrap_or_default()
					.as_millis()
					.try_into()
					.unwrap_or(u64::MAX),
				active_scene: session.active_scene_id().clone(),
				project: None,
				scenes: Vec::new(),
			},
			project: project_dirty.then(|| session.project_document()),
			scenes,
		}))
	}

	pub(crate) fn publish_recovery(&mut self, staged: StagedRecovery) -> Result<bool, String> {
		if staged.generation != self.generation
			|| staged.path != self.recovery.path
			|| staged.write_epoch != self.recovery.write_epoch
			|| self.recovery.awaiting_review
			|| self
				.recovery
				.last_write
				.is_some_and(|(revision, _)| revision >= staged.revision)
			|| !self.backend.session().is_dirty()
		{
			return Ok(false);
		}
		if let Some(existing) = read_checkpoint(&staged.path)? {
			if self.recovery.last_write.map(|(_, hash)| hash) != Some(existing.token()) {
				self.recovery.awaiting_review = true;
				return Err(
					"A changed recovery file requires review before it can be replaced.".into(),
				);
			}
		}
		// As with ordinary saves, the comparison and rename coordinate this editor's writer.
		// Another process can change the file between the two filesystem operations.
		fs::rename(&staged.temporary, &staged.path)
			.map_err(|error| format!("Could not publish draft recovery: {error}"))?;
		self.recovery.last_write = Some((staged.revision, staged.hash));
		Ok(true)
	}

	pub(crate) fn review_recovery(&mut self) -> Result<Option<RecoveryOffer>, String> {
		if !self.recovery.awaiting_review {
			return Ok(None);
		}
		let Some(file) = read_checkpoint(&self.recovery.path)? else {
			self.recovery.awaiting_review = false;
			return Ok(None);
		};
		let token = file.token();
		let payload = file.checkpoint();
		let (captured_at_ms, files, issue) = match payload {
			Ok(payload) => {
				let mut files = payload
					.scenes
					.iter()
					.map(|scene| scene.path.clone())
					.collect::<Vec<_>>();
				if payload.project.is_some() {
					files.push("project.json".into());
				}
				let issue = self.check_recovery_bases(&payload).err();
				(payload.captured_at_ms, files, issue)
			}
			Err(error) => (0, Vec::new(), Some(error)),
		};
		Ok(Some(RecoveryOffer {
			token,
			captured_at_ms,
			files,
			issue,
		}))
	}

	pub(crate) fn apply_recovery(
		&mut self,
		revision: u64,
		token: Fingerprint,
		restore: bool,
	) -> Result<EditorSnapshotDto, String> {
		self.check_revision(revision)?;
		if !self.recovery.awaiting_review {
			return Err("The recovery review is no longer current".into());
		}
		let file = read_checkpoint(&self.recovery.path)?
			.ok_or("The recovery file was removed. Reopen recovery review.")?;
		if token != file.token() {
			return Err("The recovery file changed. Reopen recovery review.".into());
		}
		if restore {
			let payload = file.checkpoint()?;
			let inputs = self.check_recovery_bases(&payload)?;
			let next_scene = resolve_output(
				&self.asset_root,
				inputs.project.scene_path(&payload.active_scene)?,
			)?;
			let mut documents = Vec::new();
			for scene in &payload.scenes {
				let mut document = SceneData::from_json(&scene.document)?;
				document.migrate_legacy_lights()?;
				documents.push((scene.id.clone(), document));
			}
			self.backend
				.restore_editor_recovery(inputs.project, payload.active_scene, documents)
				.map_err(|error| error.to_string())?;
			for (path, base) in inputs.scene_bases {
				self.saved_files.record_scene(&path, base);
			}
			self.scene_path = next_scene;
			self.cache_bounds();
			// The restored drafts still depend on this checkpoint until Save or
			// an explicit discard. A later checkpoint can replace it.
			self.recovery.last_write = Some((self.backend.session().revision(), token));
		} else {
			fs::remove_file(&self.recovery.path)
				.map_err(|error| format!("Could not discard draft recovery: {error}"))?;
		}
		self.recovery.awaiting_review = false;
		self.recovery.write_epoch = self.recovery.write_epoch.wrapping_add(1);
		self.backend.snapshot().map_err(|error| error.to_string())
	}

	fn check_recovery_bases(&self, checkpoint: &Checkpoint) -> Result<RecoveryInputs, String> {
		if checkpoint.scope != self.recovery.scope {
			return Err("This recovery file belongs to a different project location.".into());
		}
		verify_saved_parent(&self.project_path)?;
		let manifest = disk_fingerprint(&self.project_path, MAX_PROJECT_BYTES)?;
		let intended_manifest = checkpoint
			.project
			.as_ref()
			.map(|source| fingerprint(source.as_bytes()));
		if manifest != self.saved_files.manifest
			|| (manifest != checkpoint.base_manifest
				&& intended_manifest.is_none_or(|intended| Some(intended) != manifest))
		{
			return Err("The project manifest changed after this recovery was captured.".into());
		}
		let project = match &checkpoint.project {
			Some(source) => ProjectData::from_json(source)?,
			None => self.backend.session().project().clone(),
		};
		let mut seen = std::collections::HashSet::new();
		let mut bases = Vec::new();
		for scene in &checkpoint.scenes {
			if project.scene_path(&scene.id)? != scene.path || !seen.insert(&scene.id) {
				return Err("Recovery scene paths must match its project catalog".into());
			}
			let target = resolve_output(&self.asset_root, &scene.path)?;
			let disk = disk_fingerprint(&target, MAX_SCENE_BYTES)?;
			if self
				.saved_files
				.scene(&scene.path)
				.is_some_and(|base| Some(base) != disk)
			{
				return Err(format!(
					"Scene '{}' changed after the project opened.",
					scene.path
				));
			}
			let mut intended = scene.document.clone();
			terminate_scene_json(&mut intended);
			if disk != scene.base && disk != Some(fingerprint(intended.as_bytes())) {
				return Err(format!(
					"Scene '{}' changed after this recovery was captured.",
					scene.path
				));
			}
			if let Some(base) = disk {
				bases.push((scene.path.clone(), base));
			}
		}
		let mut paths: Vec<PathBuf> = Vec::new();
		for entry in project.scenes.iter() {
			let output = resolve_output(&self.asset_root, entry.asset_path())?;
			for other in &paths {
				if same_destination(&output, other)? {
					return Err("Recovery catalog scenes must use distinct files".into());
				}
			}
			paths.push(output);
			if let Ok(disk_path) = self.backend.session().project().scene_path(entry.id()) {
				if disk_path != entry.asset_path() && !seen.contains(entry.id()) {
					return Err("Recovery lacks the document for a renamed scene".into());
				}
			}
		}
		Ok(RecoveryInputs {
			project,
			scene_bases: bases,
		})
	}

	pub(super) fn recovery_after_save(&mut self) {
		self.recovery.write_epoch = self.recovery.write_epoch.wrapping_add(1);
		if !self.backend.session().is_dirty() {
			self.clear_generated_recovery();
		}
	}

	/// Explicit workspace discard or successful full save retires this session's file.
	pub(crate) fn clear_generated_recovery(&mut self) {
		if let Some((_, hash)) = self.recovery.last_write.take() {
			let removed = match read_checkpoint(&self.recovery.path) {
				Ok(Some(StoredRecovery::Complete(bytes))) if fingerprint(&bytes) == hash => {
					fs::remove_file(&self.recovery.path).is_ok()
				}
				Ok(None) => true,
				_ => false,
			};
			if !removed {
				self.recovery.awaiting_review = true;
				engine_logger::warn!(target: "editor", "A draft recovery file could not be retired and remains available for review");
			}
		}
	}
}

fn read_checkpoint(path: &Path) -> Result<Option<StoredRecovery>, String> {
	verify_saved_parent(path)?;
	match fs::symlink_metadata(path) {
		Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => {
			if meta.len() > MAX_RECOVERY_BYTES as u64 {
				// Keep disk may discard an oversized file explicitly. Scope its
				// review token using file metadata and a small prefix, without
				// loading or hashing the unbounded body.
				let mut sample = format!(
					"oversized:{:?}:{:?}:{}",
					meta.created(),
					meta.modified(),
					meta.len()
				)
				.into_bytes();
				File::open(path)
					.and_then(|file| file.take(4096).read_to_end(&mut sample))
					.map_err(|error| error.to_string())?;
				Ok(Some(StoredRecovery::Oversized(fingerprint(&sample))))
			} else {
				read_bounded(path, MAX_RECOVERY_BYTES as u64)
					.map(|bytes| Some(StoredRecovery::Complete(bytes)))
			}
		}
		Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
		Ok(_) => Err("Recovery path must be a regular file".into()),
		Err(error) => Err(format!("Could not inspect draft recovery: {error}")),
	}
}

fn decode(bytes: &[u8]) -> Result<Checkpoint, String> {
	let envelope: Envelope = serde_json::from_slice(bytes)
		.map_err(|error| format!("The recovery file is corrupt: {error}"))?;
	if envelope.payload.schema_version != FORMAT_VERSION {
		return Err("The recovery file was written in an unsupported format.".into());
	}
	if fingerprint(&encode(&envelope.payload)?) != envelope.digest {
		return Err("The recovery file is corrupt.".into());
	}
	if envelope.payload.scenes.len() > MAX_PROJECT_SCENES {
		return Err("Recovery contains too many scenes.".into());
	}
	if envelope.payload.project.is_none() && envelope.payload.scenes.is_empty() {
		return Err("The recovery file is empty.".into());
	}
	Ok(envelope.payload)
}

struct BoundedBytes(Vec<u8>);

impl Write for BoundedBytes {
	fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
		if self.0.len().saturating_add(bytes.len()) > MAX_RECOVERY_BYTES {
			return Err(io::Error::other(
				"Drafts exceed the recovery checkpoint limit",
			));
		}
		self.0.extend_from_slice(bytes);
		Ok(bytes.len())
	}

	fn flush(&mut self) -> io::Result<()> {
		Ok(())
	}
}

fn encode(value: &impl serde::Serialize) -> Result<Vec<u8>, String> {
	let mut writer = BoundedBytes(Vec::new());
	serde_json::to_writer(&mut writer, value).map_err(|error| error.to_string())?;
	Ok(writer.0)
}

fn stage_file(
	path: &Path,
	bytes: &[u8],
	generation: u64,
	revision: u64,
	write_epoch: u64,
) -> Result<StagedRecovery, String> {
	static NEXT_FILE: AtomicU64 = AtomicU64::new(0);
	verify_saved_parent(path)?;
	for _ in 0..32 {
		let id = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
		let temporary = path.with_file_name(format!("{FILE_NAME}.tmp-{}-{id}", std::process::id()));
		match OpenOptions::new()
			.create_new(true)
			.write(true)
			.open(&temporary)
		{
			Ok(mut file) => {
				let staged = StagedRecovery {
					temporary,
					path: path.to_owned(),
					generation,
					revision,
					write_epoch,
					hash: fingerprint(bytes),
				};
				let result = file.write_all(bytes).and_then(|()| file.sync_all());
				drop(file);
				result.map_err(|error| format!("Could not write draft recovery: {error}"))?;
				return Ok(staged);
			}
			Err(error) if error.kind() == io::ErrorKind::AlreadyExists => (),
			Err(error) => return Err(format!("Could not prepare draft recovery: {error}")),
		}
	}
	Err("Could not allocate a draft recovery file".into())
}
