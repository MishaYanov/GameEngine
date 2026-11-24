//! Authored document loading and checked per-file publication.
use super::*;
use runtime::{project::MAX_PROJECT_BYTES, scene_data::MAX_SCENE_BYTES};
use sha2::{Digest, Sha256};

pub(super) type Fingerprint = [u8; 32];

#[derive(Default)]
pub(super) struct SavedFiles {
	pub(super) manifest: Option<Fingerprint>,
	scenes: HashMap<String, Fingerprint>,
}

impl SavedFiles {
	pub(super) fn new(manifest: Option<Fingerprint>, scenes: HashMap<String, Fingerprint>) -> Self {
		Self { manifest, scenes }
	}

	pub(super) fn scene(&self, path: &str) -> Option<Fingerprint> {
		self.scenes.get(&scene_path_key(path)).copied()
	}

	pub(super) fn record_scene(&mut self, path: &str, fingerprint: Fingerprint) {
		self.scenes.insert(scene_path_key(path), fingerprint);
	}
}

pub(super) struct LoadedScenes {
	pub(super) scenes: Vec<(SceneId, SceneData)>,
	pub(super) fingerprints: HashMap<String, Fingerprint>,
}

#[derive(Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SaveOutcome {
	Written,
	Conflict,
	Failed,
	Blocked,
}

#[derive(Debug, serde::Serialize)]
pub(crate) struct SaveFileReport {
	pub path: String,
	pub outcome: SaveOutcome,
	pub message: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub(crate) struct SaveAllReport {
	pub snapshot: EditorSnapshotDto,
	pub files: Vec<SaveFileReport>,
}

enum SaveTarget {
	Scene {
		id: SceneId,
		path: String,
		document: Arc<SceneData>,
	},
	Manifest {
		project: Arc<ProjectData>,
	},
}

impl SaveTarget {
	fn path(&self) -> &str {
		match self {
			Self::Scene { path, .. } => path,
			Self::Manifest { .. } => "project.json",
		}
	}

	fn encode(&self) -> Result<String, String> {
		match self {
			Self::Scene { document, .. } => scene_json(document),
			Self::Manifest { project } => project.to_json(),
		}
	}

	fn destination(
		&self,
		workspace: &EditorWorkspace,
	) -> Result<(PathBuf, Option<Fingerprint>, usize), String> {
		match self {
			Self::Scene { id, path, .. } => Ok((
				workspace.scene_output(path, Some(id))?,
				workspace.saved_files.scene(path),
				MAX_SCENE_BYTES,
			)),
			Self::Manifest { .. } => {
				verify_saved_parent(&workspace.project_path)?;
				Ok((
					workspace.project_path.clone(),
					workspace.saved_files.manifest,
					MAX_PROJECT_BYTES,
				))
			}
		}
	}
}

impl EditorWorkspace {
	pub(super) fn scene_output(
		&self,
		path: &str,
		owner: Option<&SceneId>,
	) -> Result<PathBuf, String> {
		let output = resolve_output(&self.asset_root, path)?;
		for entry in self.backend.session().project().scenes.iter() {
			if Some(entry.id()) != owner {
				let other = resolve_output(&self.asset_root, entry.asset_path())?;
				if same_destination(&output, &other)? {
					return Err(
						"Scene destination is already reserved by another catalog scene".into(),
					);
				}
			}
		}
		Ok(output)
	}
	/// Asset files cannot claim any registered scene destination, including a
	/// scene whose document is still an unsaved retained draft.
	#[cfg_attr(test, allow(dead_code))] // IPC/native callers are absent from include-based CPU tests.
	pub fn validate_asset_destination(&self, path: &str) -> Result<(), String> {
		self.scene_output(path, None).map(|_| ())
	}
	pub fn save(&mut self, revision: u64) -> Result<EditorSnapshotDto, String> {
		self.check_revision(revision)?;
		if self.backend.session().has_transaction() {
			return Err("Commit or cancel the grouped edit before saving".into());
		}
		let id = self.backend.session().active_scene_id().clone();
		let path = self.backend.session().project().scene_path(&id)?.to_owned();
		let document = self.backend.session().authored_document();
		self.write_save_target(&SaveTarget::Scene { id, path, document })
			.map_err(|(_, message)| message)?;
		engine_logger::info!(target: "editor", "Scene saved: generation={}, revision={}", self.generation, self.backend.session().revision());
		self.backend.snapshot().map_err(|e| e.to_string())
	}

	pub fn reload_with_policy(
		&mut self,
		revision: u64,
		policy: ReloadPolicy,
	) -> Result<EditorSnapshotDto, String> {
		self.check_revision(revision)?;
		if self.has_dirty_clip_timelines() {
			return Err(
				"Save or discard open Animation Clip changes before reloading the project".into(),
			);
		}
		if self.has_dirty_prop_sources() {
			return Err("Save or discard open Prop changes before reloading the project".into());
		}
		if self.backend.session().has_transaction()
			|| (self.backend.session().is_playing() && matches!(policy, ReloadPolicy::RequireClean))
		{
			return Err("Stop Play and finish grouped edits before reloading".into());
		}
		if matches!(policy, ReloadPolicy::RequireClean) && self.backend.session().is_dirty() {
			return Err(
				"Workspace contains unsaved edits; request discard_unsaved explicitly to reload"
					.into(),
			);
		}
		let next_kind = if matches!(self.workspace_kind, WorkspaceKind::ManifestBacked)
			|| self.project_path.try_exists().map_err(|e| e.to_string())?
		{
			WorkspaceKind::ManifestBacked
		} else {
			WorkspaceKind::StandaloneScene
		};
		let (project, manifest_fingerprint) = match next_kind {
			WorkspaceKind::ManifestBacked => {
				let (project, fingerprint) =
					read_project_with_fingerprint(&self.project_path, "reload_project")?;
				(project, Some(fingerprint))
			}
			WorkspaceKind::StandaloneScene => (self.backend.session().project().clone(), None),
		};
		let scene_path = resolve_existing(&self.asset_root, project.startup_scene_path()?)?;
		let (data, startup_fingerprint) = read_scene_with_fingerprint(&scene_path, "reload_scene")?;
		let documents =
			load_project_documents(&self.asset_root, &project, &data, startup_fingerprint)?;
		let saved_files = SavedFiles::new(manifest_fingerprint, documents.fingerprints);
		let mut candidate = runtime::EngineBuilder::new(data, project)
			.with_scenes(documents.scenes)
			.module_resolver(Arc::new(gameplay_catalog::resolve))
			.animation_resolver(animation_resolver(&self.asset_root))
			.input_actions_resolver(input_actions_resolver(&self.asset_root))
			.animator_resolver(animator_resolver(&self.asset_root))
			.control_graph_resolver(control_graph_resolver(&self.asset_root))
			.build()
			.map_err(|e| e.to_string())?;
		candidate
			.supersede_revision(revision)
			.map_err(|e| e.to_string())?;
		let snapshot = candidate.snapshot().map_err(|e| e.to_string())?;
		self.jobs.advance_generation().map_err(|e| e.to_string())?;
		self.clear_generated_recovery();
		self.backend = candidate;
		self.recovery = recovery::RecoveryState::open(&self.project_path);
		self.animation_preview = None;
		self.authored_clip_preview = None;
		self.clip_timeline_drafts = Default::default();
		self.camera_preview = None;
		self.prop_preview = None;
		self.prop_preview_request = 0;
		self.prefab_source_drafts = Default::default();
		// The replacement owns a fresh journal and scene revision sequence. A
		// new workspace generation also forces existing viewport caches to resync.
		self.generation = NEXT_WORKSPACE.fetch_add(1, Ordering::Relaxed);
		self.workspace_kind = next_kind;
		self.saved_files = saved_files;
		self.scene_path = scene_path;
		self.metadata.clear();
		self.pending.clear();
		self.bounds_seen.clear();
		self.bounds_references.clear();
		self.bounds_queue.clear();
		self.completions.clear();
		self.completion_bytes = 0;
		self.cache_bounds();
		Ok(snapshot)
	}

	fn check_compound_capacity(&self) -> Result<(), String> {
		if self.backend.session().revision().checked_add(2).is_none()
			|| self.backend.change_cursor().checked_add(4).is_none()
		{
			return Err("Workspace revision/event space is exhausted".into());
		}
		Ok(())
	}

	pub fn register_scene(
		&mut self,
		path: &str,
		revision: u64,
	) -> Result<EditorSnapshotDto, String> {
		self.check_revision(revision)?;
		let resolved = resolve_existing(&self.asset_root, path)?;
		for entry in self.backend.session().project().scenes.iter() {
			if resolve_existing(&self.asset_root, entry.asset_path())
				.ok()
				.as_ref() == Some(&resolved)
			{
				return Err("Scene file is already registered".into());
			}
		}
		let (document, fingerprint) = read_scene_with_fingerprint(&resolved, "register_scene")?;
		let id = SceneId::generate();
		self.check_compound_capacity()?;
		self.backend
			.execute(EditorCommandRequest {
				expected_revision: revision,
				command: Command::AddScene {
					id: id.clone(),
					asset_path: path.into(),
					document,
				},
			})
			.map_err(|e| e.to_string())?;
		self.backend
			.mark_scene_saved_by_id(&id)
			.map_err(|e| e.to_string())?;
		self.saved_files.record_scene(path, fingerprint);
		self.backend.snapshot().map_err(|e| e.to_string())
	}

	pub fn save_scene_as(
		&mut self,
		path: &str,
		revision: u64,
	) -> Result<EditorSnapshotDto, String> {
		self.check_revision(revision)?;
		if self.backend.session().is_playing() || self.backend.session().has_transaction() {
			return Err("Stop Play and finish grouped edits before saving a scene as".into());
		}
		self.check_compound_capacity()?;
		let id = self.backend.session().active_scene_id().clone();
		runtime::scene_catalog::SceneDescriptor::new(id.clone(), path)?;
		let output = self.scene_output(path, Some(&id))?;
		if output.exists() {
			return Err("Save As requires a new destination file".into());
		}
		for entry in self.backend.session().project().scenes.iter() {
			if entry.id() != &id && entry.asset_path().eq_ignore_ascii_case(path) {
				return Err("Scene path is already registered".into());
			}
		}
		// An unsuccessful catalog commit may leave a new unreferenced file;
		// the old file and authored state are preserved.
		let bytes = scene_json(&self.backend.session().authored_document())?;
		file_operation(
			"save_scene_as",
			runtime::scene_io::save_new_bytes(&output, bytes.as_bytes()),
		)?;
		self.execute_receipt(EditorCommandRequest {
			expected_revision: revision,
			command: Command::ChangeScenePath {
				scene_id: id,
				asset_path: path.into(),
			},
		})
		.map_err(|e| e.to_string())?;
		self.backend.mark_scene_saved().map_err(|e| e.to_string())?;
		self.saved_files
			.record_scene(path, fingerprint(bytes.as_bytes()));
		self.recovery_after_save();
		self.backend.snapshot().map_err(|e| e.to_string())
	}

	pub fn save_project(&mut self, revision: u64) -> Result<EditorSnapshotDto, String> {
		self.check_revision(revision)?;
		if self.backend.session().has_transaction() || self.backend.session().is_playing() {
			return Err(
				"Stop Play and finish the grouped edit before saving project settings".into(),
			);
		}
		let project = self.backend.session().project_document();
		self.backend.session().validate_all_documents()?;
		let mut files = HashSet::new();
		for entry in project.scenes.iter() {
			if !files.insert(resolve_existing(&self.asset_root, entry.asset_path())?) {
				return Err("Catalog scenes resolve to the same file".into());
			}
		}
		self.write_save_target(&SaveTarget::Manifest { project })
			.map_err(|(_, message)| message)?;

		engine_logger::info!(target:"editor","Project settings saved: generation={}, revision={}",self.generation,self.backend.session().revision());
		self.backend.snapshot().map_err(|e| e.to_string())
	}

	/// Compatibility entry point preserves the existing snapshot/error contract.
	#[cfg_attr(not(test), allow(dead_code))]
	pub fn save_all(&mut self, revision: u64) -> Result<EditorSnapshotDto, String> {
		let report = self.save_all_report(revision)?;
		if let Some(message) = report.files.into_iter().find_map(|file| file.message) {
			return Err(message);
		}
		Ok(report.snapshot)
	}

	/// Check every destination before publication and keep successful baselines on partial failure.
	pub(crate) fn save_all_report(&mut self, revision: u64) -> Result<SaveAllReport, String> {
		self.check_revision(revision)?;
		let session = self.backend.session();
		if session.is_playing() || session.has_transaction() {
			return Err("Stop Play and finish the grouped edit before saving all".into());
		}
		session.validate_all_documents()?;
		let state = session.state();
		let project = session.project_document();
		let write_manifest = state.project_dirty || self.saved_files.manifest.is_none();
		let mut targets = Vec::new();
		let mut clean_references = Vec::new();
		for scene in state.scenes {
			if scene.dirty {
				targets.push(SaveTarget::Scene {
					document: session.scene_document(&scene.id)?,
					id: scene.id,
					path: scene.asset_path,
				});
			} else if write_manifest {
				clean_references.push((scene.id, scene.asset_path));
			} else {
				self.scene_output(&scene.asset_path, Some(&scene.id))?;
			}
		}
		if write_manifest {
			targets.push(SaveTarget::Manifest { project });
		}
		let mut files = targets
			.iter()
			.map(|target| SaveFileReport {
				path: target.path().to_owned(),
				outcome: SaveOutcome::Blocked,
				message: None,
			})
			.collect::<Vec<_>>();
		let writes = targets.len() as u64;
		if writes > 0
			&& (session.revision().checked_add(writes + 1).is_none()
				|| session.scene_revision() == u64::MAX
				|| self
					.backend
					.change_cursor()
					.checked_add((writes + 1) * 2)
					.is_none())
		{
			return Err("Workspace revision/event space is exhausted".into());
		}
		for (id, path) in clean_references {
			if let Err(message) = self.verify_catalog_scene(&id, &path) {
				files.push(SaveFileReport {
					path,
					outcome: SaveOutcome::Conflict,
					message: Some(message),
				});
				return self.save_report(files);
			}
		}

		for (target, file) in targets.iter().zip(&mut files) {
			let check = target
				.destination(self)
				.map_err(|error| (SaveOutcome::Failed, error))
				.and_then(|(output, baseline, limit)| {
					verify_disk_baseline(&output, baseline, limit)
						.map_err(|error| (SaveOutcome::Conflict, error))
				})
				.and_then(|()| {
					target
						.encode()
						.map(drop)
						.map_err(|error| (SaveOutcome::Failed, error))
				});
			if let Err((outcome, message)) = check {
				file.outcome = outcome;
				file.message = Some(message);
				return self.save_report(files);
			}
		}
		if !write_manifest {
			if let Err(message) = verify_saved_parent(&self.project_path).and_then(|()| {
				verify_disk_baseline(
					&self.project_path,
					self.saved_files.manifest,
					MAX_PROJECT_BYTES,
				)
			}) {
				files.push(SaveFileReport {
					path: "project.json".into(),
					outcome: SaveOutcome::Conflict,
					message: Some(message),
				});
				return self.save_report(files);
			}
		}
		for (target, file) in targets.into_iter().zip(&mut files) {
			match self.write_save_target(&target) {
				Ok(()) => file.outcome = SaveOutcome::Written,
				Err((outcome, message)) => {
					file.outcome = outcome;
					file.message = Some(message);
					break;
				}
			}
		}
		self.save_report(files)
	}

	fn save_report(&self, files: Vec<SaveFileReport>) -> Result<SaveAllReport, String> {
		let snapshot = self.backend.snapshot().map_err(|error| error.to_string())?;
		Ok(SaveAllReport { snapshot, files })
	}

	fn verify_catalog_scene(&self, id: &SceneId, path: &str) -> Result<(), String> {
		let output = self.scene_output(path, Some(id))?;
		let baseline = self.saved_files.scene(path)
			.ok_or_else(|| format!("Save conflict: '{path}' has no saved file baseline. Save the scene before saving the project."))?;
		verify_disk_baseline(&output, Some(baseline), MAX_SCENE_BYTES)
	}

	fn write_save_target(&mut self, target: &SaveTarget) -> Result<(), (SaveOutcome, String)> {
		let (output, baseline, limit) = target
			.destination(self)
			.map_err(|error| (SaveOutcome::Failed, error))?;
		if let SaveTarget::Manifest { project } = target {
			for entry in project.scenes.iter() {
				self.verify_catalog_scene(entry.id(), entry.asset_path())
					.map_err(|error| (SaveOutcome::Conflict, error))?;
			}
		}

		let bytes = target
			.encode()
			.map_err(|error| (SaveOutcome::Failed, error))?;
		verify_disk_baseline(&output, baseline, limit)
			.map_err(|error| (SaveOutcome::Conflict, error))?;
		let result = match (baseline, target) {
			(None, _) => runtime::scene_io::save_new_bytes(&output, bytes.as_bytes()),
			(_, SaveTarget::Scene { .. }) => {
				runtime::scene_io::replace_bytes(&output, bytes.as_bytes())
			}
			(_, SaveTarget::Manifest { .. }) => replace_project_bytes(&output, bytes.as_bytes()),
		};
		file_operation("save_document", result).map_err(|error| (SaveOutcome::Failed, error))?;
		let fingerprint = fingerprint(bytes.as_bytes());
		match target {
			SaveTarget::Scene { id, path, .. } => {
				self.saved_files.record_scene(path, fingerprint);
				self.backend.mark_scene_saved_by_id(id)
			}
			SaveTarget::Manifest { .. } => {
				self.saved_files.manifest = Some(fingerprint);
				self.workspace_kind = WorkspaceKind::ManifestBacked;
				self.backend.mark_project_saved()
			}
		}
		.map_err(|error| (SaveOutcome::Failed, error.to_string()))?;
		self.recovery_after_save();
		Ok(())
	}
}

pub(super) fn fingerprint(bytes: &[u8]) -> Fingerprint {
	Sha256::digest(bytes).into()
}

fn scene_path_key(path: &str) -> String {
	if cfg!(windows) {
		path.to_lowercase()
	} else {
		path.to_owned()
	}
}

pub(super) fn verify_saved_parent(path: &Path) -> Result<(), String> {
	let saved_parent = path.parent().ok_or("Missing project directory")?;
	let current = saved_parent
		.canonicalize()
		.map_err(|error| format!("Cannot check the project directory: {error}"))?;
	if current != saved_parent || !current.is_dir() {
		return Err(
			"The project directory changed on disk. Reopen the project before writing.".into(),
		);
	}
	Ok(())
}

pub(super) fn disk_fingerprint(path: &Path, limit: usize) -> Result<Option<Fingerprint>, String> {
	let current = match fs::symlink_metadata(path) {
		Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => {
			Some(fingerprint(&read_bounded(path, limit as u64).map_err(
				|error| format!("Cannot check '{}' before saving: {error}", path.display()),
			)?))
		}
		Ok(_) => {
			return Err(format!(
				"Save conflict: '{}' is no longer a regular file.",
				path.display()
			))
		}
		Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
		Err(error) => {
			return Err(format!(
				"Cannot check '{}' before saving: {error}",
				path.display()
			))
		}
	};
	Ok(current)
}

pub(super) fn verify_disk_baseline(
	path: &Path,
	expected: Option<Fingerprint>,
	limit: usize,
) -> Result<(), String> {
	if disk_fingerprint(path, limit)? == expected {
		Ok(())
	} else {
		Err(format!(
			"Save conflict: '{}' changed on disk. Review the disk version before saving.",
			path.display()
		))
	}
}

pub(super) fn scene_json(data: &SceneData) -> Result<String, String> {
	let mut bytes = data.to_json()?;
	terminate_scene_json(&mut bytes);
	Ok(bytes)
}

pub(super) fn terminate_scene_json(source: &mut String) {
	if source.len() < MAX_SCENE_BYTES {
		source.push('\n');
	}
}

fn relative(path: &str) -> Result<(), String> {
	if path.is_empty()
		|| path.contains(['\\', ':', '\0'])
		|| Path::new(path).is_absolute()
		|| path.split('/').any(|part| part == ".." || part.is_empty())
	{
		return Err(
			"Use an asset-relative path with forward slashes and no parent traversal".into(),
		);
	}
	Ok(())
}
pub(super) fn resolve_existing(root: &Path, path: &str) -> Result<PathBuf, String> {
	relative(path)?;
	let candidate = root.join(path).canonicalize().map_err(|e| e.to_string())?;
	if !candidate.starts_with(root) || !candidate.is_file() {
		return Err("Requested file is outside the asset root or is not a file".into());
	}
	Ok(candidate)
}
#[cfg(windows)]
pub(super) fn same_destination(left: &Path, right: &Path) -> Result<bool, String> {
	use std::os::windows::ffi::OsStrExt;
	#[link(name = "kernel32")]
	extern "system" {
		fn CompareStringOrdinal(
			a: *const u16,
			a_len: i32,
			b: *const u16,
			b_len: i32,
			ignore_case: i32,
		) -> i32;
	}
	let a: Vec<u16> = left.as_os_str().encode_wide().collect();
	let b: Vec<u16> = right.as_os_str().encode_wide().collect();
	// SAFETY: Both pointers address live UTF-16 arrays of the supplied lengths.
	let result = unsafe {
		CompareStringOrdinal(
			a.as_ptr(),
			a.len().try_into().map_err(|_| "Path too long")?,
			b.as_ptr(),
			b.len().try_into().map_err(|_| "Path too long")?,
			1,
		)
	};
	match result {
		0 => Err("Cannot compare scene destinations".into()),
		2 => Ok(true),
		_ => Ok(false),
	}
}
#[cfg(not(windows))]
pub(super) fn same_destination(left: &Path, right: &Path) -> Result<bool, String> {
	Ok(left == right)
}
pub(super) fn resolve_output(root: &Path, path: &str) -> Result<PathBuf, String> {
	relative(path)?;
	let candidate = root.join(path);
	let parent = candidate
		.parent()
		.ok_or("Missing output directory")?
		.canonicalize()
		.map_err(|e| e.to_string())?;
	if !parent.starts_with(root) {
		return Err("Output directory is outside the asset root".into());
	}
	if candidate.exists() {
		resolve_existing(root, path)
	} else {
		Ok(parent.join(candidate.file_name().ok_or("Missing filename")?))
	}
}
pub(super) fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
	let mut bytes = Vec::new();
	File::open(path)
		.map_err(|e| e.to_string())?
		.take(limit + 1)
		.read_to_end(&mut bytes)
		.map_err(|e| e.to_string())?;
	if bytes.len() as u64 > limit {
		return Err("File exceeds byte limit".into());
	}
	Ok(bytes)
}
fn replace_project_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
	let mut temporary = None;
	for id in 0..128 {
		let candidate = path.with_file_name(format!(
			"{}.tmp-{}-{id}",
			path.file_name()
				.ok_or("Missing filename")?
				.to_string_lossy(),
			std::process::id()
		));
		match OpenOptions::new()
			.create_new(true)
			.write(true)
			.open(&candidate)
		{
			Ok(file) => {
				temporary = Some((candidate, file));
				break;
			}
			Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
			Err(error) => return Err(error.to_string()),
		}
	}
	let (temporary, mut file) = temporary.ok_or("Cannot allocate temporary file")?;
	let result = file.write_all(bytes).and_then(|_| file.sync_all());
	drop(file);
	if let Err(error) = result {
		let _ = fs::remove_file(&temporary);
		return Err(error.to_string());
	}
	if let Err(error) = fs::rename(&temporary, path) {
		let _ = fs::remove_file(&temporary);
		return Err(error.to_string());
	}
	Ok(())
}

pub(super) fn read_editor_scene(path: &Path, operation: &'static str) -> Result<SceneData, String> {
	read_scene_with_fingerprint(path, operation).map(|(scene, _)| scene)
}

pub(super) fn read_scene_with_fingerprint(
	path: &Path,
	operation: &'static str,
) -> Result<(SceneData, Fingerprint), String> {
	file_operation(
		operation,
		(|| {
			let bytes = read_bounded(path, MAX_SCENE_BYTES as u64)?;
			let source = std::str::from_utf8(&bytes).map_err(|error| error.to_string())?;
			let mut scene = SceneData::from_json(source)?;
			scene.migrate_legacy_lights()?;
			Ok((scene, fingerprint(&bytes)))
		})(),
	)
	.map_err(|error| format!("Failed to load scene '{}': {error}", path.display()))
}

pub(super) fn read_project_with_fingerprint(
	path: &Path,
	operation: &'static str,
) -> Result<(ProjectData, Fingerprint), String> {
	file_operation(
		operation,
		(|| {
			verify_saved_parent(path)?;
			let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
			if !metadata.is_file() || metadata.file_type().is_symlink() {
				return Err("Project manifest must be a regular file".into());
			}
			let bytes = read_bounded(path, MAX_PROJECT_BYTES as u64)?;
			let source = std::str::from_utf8(&bytes).map_err(|error| error.to_string())?;
			Ok((ProjectData::from_json(source)?, fingerprint(&bytes)))
		})(),
	)
	.map_err(|error| format!("Cannot read project: '{}': {error}", path.display()))
}

pub(super) fn load_project_documents(
	root: &Path,
	project: &ProjectData,
	startup: &SceneData,
	startup_fingerprint: Fingerprint,
) -> Result<LoadedScenes, String> {
	let mut seen = HashSet::new();
	let mut scenes = Vec::new();
	let mut fingerprints = HashMap::new();
	let mut retained_bytes = 0usize;
	for entry in project.scenes.iter() {
		let path = resolve_existing(root, entry.asset_path())?;
		if !seen.insert(path.clone()) {
			return Err("Catalog entries resolve to the same scene file".into());
		}
		let (document, fingerprint) = if entry.id() == &project.startup_scene_id {
			(Cow::Borrowed(startup), startup_fingerprint)
		} else {
			let (document, fingerprint) = read_scene_with_fingerprint(&path, "load_scene")?;
			(Cow::Owned(document), fingerprint)
		};
		retained_bytes = retained_bytes.saturating_add(
			document
				.serialized_size()?
				.saturating_mul(4)
				.saturating_add(std::mem::size_of::<SceneData>()),
		);
		if retained_bytes > runtime::editor::EditorSession::MAX_RETAINED_DOCUMENT_BYTES {
			return Err("Retained scene document byte budget exceeded".into());
		}
		project.validate_scene_for(entry.id(), &document)?;
		fingerprints.insert(scene_path_key(entry.asset_path()), fingerprint);
		if entry.id() != &project.startup_scene_id {
			scenes.push((entry.id().clone(), document.into_owned()));
		}
	}
	Ok(LoadedScenes {
		scenes,
		fingerprints,
	})
}
