use super::*;

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
		let path = self
			.backend
			.session()
			.project()
			.scene_path(self.backend.session().active_scene_id())?;
		let output = self.scene_output(path, Some(self.backend.session().active_scene_id()))?;
		file_operation(
			"save_scene",
			runtime::scene_io::save_data(&output, &self.backend.session().authored_document()),
		)?;
		self.backend.mark_scene_saved().map_err(|e| e.to_string())?;
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
			return Err("Save or discard open Animation Clip changes before reloading the project".into());
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
		let project = match next_kind {
			WorkspaceKind::ManifestBacked => {
				file_operation("reload_project", ProjectData::load(&self.project_path))?
			}
			WorkspaceKind::StandaloneScene => self.backend.session().project().clone(),
		};
		let scene_path = resolve_existing(&self.asset_root, project.startup_scene_path()?)?;
		let data = file_operation("reload_scene", runtime::scene_io::load_data(&scene_path))?.data;
		let documents = load_project_documents(&self.asset_root, &project, &data)?;
		let mut candidate = runtime::EngineBuilder::new(data, project)
			.with_scenes(documents)
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
		self.backend = candidate;
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
		let document =
			file_operation("register_scene", runtime::scene_io::load_data(&resolved))?.data;
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
		file_operation(
			"save_scene_as",
			runtime::scene_io::save_data(&output, &self.backend.session().authored_document()),
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
		self.backend.snapshot().map_err(|e| e.to_string())
	}

	pub fn save_project(&mut self, revision: u64) -> Result<EditorSnapshotDto, String> {
		self.check_revision(revision)?;
		if self.backend.session().has_transaction() || self.backend.session().is_playing() {
			return Err(
				"Stop Play and finish the grouped edit before saving project settings".into(),
			);
		}
		let project = self.backend.session().project();
		self.backend.session().validate_all_documents()?;
		let mut files = HashSet::new();
		for entry in project.scenes.iter() {
			if !files.insert(resolve_existing(&self.asset_root, entry.asset_path())?) {
				return Err("Catalog scenes resolve to the same file".into());
			}
		}
		file_operation("save_project", atomic_json(&self.project_path, project))?;
		// The manifest now exists even if marking its saved baseline later fails.
		self.workspace_kind = WorkspaceKind::ManifestBacked;
		self.backend
			.mark_project_saved()
			.map_err(|e| e.to_string())?;
		engine_logger::info!(target:"editor","Project settings saved: generation={}, revision={}",self.generation,self.backend.session().revision());
		self.backend.snapshot().map_err(|e| e.to_string())
	}

	/// Persist dirty authored documents without changing the active scene.
	/// Successful replacements advance their own baselines before the next write.
	pub fn save_all(&mut self, revision: u64) -> Result<EditorSnapshotDto, String> {
		self.check_revision(revision)?;
		let session = self.backend.session();
		if session.is_playing() || session.has_transaction() {
			return Err("Stop Play and finish the grouped edit before saving all".into());
		}
		session.validate_all_documents()?;
		let state = session.state();
		let mut scenes = Vec::new();
		// Preflight every output before writing, including unsaved paths and aliases.
		for scene in state.scenes {
			let output = self.scene_output(&scene.asset_path, Some(&scene.id))?;
			if scene.dirty {
				let document = session.scene_document(&scene.id)?;
				document.to_json()?;
				scenes.push((scene.id, output, document));
			}
		}
		let manifest_exists = match fs::symlink_metadata(&self.project_path) {
			Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => true,
			Ok(_) => return Err("Project manifest must be a regular file".into()),
			Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
			Err(error) => return Err(error.to_string()),
		};
		let write_manifest = state.project_dirty || !manifest_exists;
		let project = session.project_document();
		if write_manifest {
			project.to_json()?;
			let parent = self
				.project_path
				.parent()
				.ok_or("Project manifest has no parent")?;
			if !parent.is_dir() {
				return Err("Project folder is unavailable".into());
			}
		}
		let writes = scenes.len() as u64 + u64::from(write_manifest);
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
		for (id, output, document) in scenes {
			file_operation(
				"save_all_scene",
				runtime::scene_io::save_data(&output, &document),
			)?;
			self.backend
				.mark_scene_saved_by_id(&id)
				.map_err(|e| e.to_string())?;
		}
		if write_manifest {
			file_operation(
				"save_all_project",
				atomic_json(&self.project_path, project.as_ref()),
			)?;
			self.workspace_kind = WorkspaceKind::ManifestBacked;
			self.backend
				.mark_project_saved()
				.map_err(|e| e.to_string())?;
		}
		self.backend.snapshot().map_err(|e| e.to_string())
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
fn same_destination(left: &Path, right: &Path) -> Result<bool, String> {
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
fn same_destination(left: &Path, right: &Path) -> Result<bool, String> {
	Ok(left == right)
}
fn resolve_output(root: &Path, path: &str) -> Result<PathBuf, String> {
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
fn atomic_json(path: &Path, value: &impl serde::Serialize) -> Result<(), String> {
	let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
	if bytes.len() > 16 * 1024 * 1024 {
		return Err("Prefab exceeds 16 MiB".into());
	}
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
	let result = file.write_all(&bytes).and_then(|_| file.sync_all());
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

pub(super) fn load_project_documents(
	root: &Path,
	project: &ProjectData,
	startup: &SceneData,
) -> Result<Vec<(SceneId, runtime::scene_data::SceneData)>, String> {
	let mut seen = HashSet::new();
	let mut documents = Vec::new();
	let mut retained_bytes = 0usize;
	for entry in project.scenes.iter() {
		let path = resolve_existing(root, entry.asset_path())?;
		if !seen.insert(path.clone()) {
			return Err("Catalog entries resolve to the same scene file".into());
		}
		let document = if entry.id() == &project.startup_scene_id {
			Cow::Borrowed(startup)
		} else {
			Cow::Owned(file_operation("load_scene", runtime::scene_io::load_data(&path))?.data)
		};
		retained_bytes = retained_bytes.saturating_add(
			document
				.serialized_size()?
				.saturating_mul(4)
				.saturating_add(std::mem::size_of::<runtime::scene_data::SceneData>()),
		);
		if retained_bytes > runtime::editor::EditorSession::MAX_RETAINED_DOCUMENT_BYTES {
			return Err("Retained scene document byte budget exceeded".into());
		}
		project.validate_scene_for(entry.id(), &document)?;
		if entry.id() != &project.startup_scene_id {
			documents.push((entry.id().clone(), document.into_owned()));
		}
	}
	Ok(documents)
}
