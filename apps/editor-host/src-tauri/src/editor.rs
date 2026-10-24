//! Project-scoped CPU application service. No window/input policy lives here.
use crate::project_location::ProjectLocation;
use bridge::editor::{EditorCommandDto as Command, EditorCommandRequest, EditorSnapshotDto};
use runtime::scene_catalog::SceneId;
use runtime::{
	asset_jobs::{AssetJobError, AssetJobId, AssetJobLimits},
	asset_registry::{AssetMetadata, AssetRef},
	assets::{AssetImportService, AssetManager, ImportedScene},
	editor::PrefabData,
	project::ProjectData,
	scene_data::EntityData,
	Engine, EntityKey,
};
use std::{
	collections::{HashMap, HashSet, VecDeque},
	fs::{self, File, OpenOptions},
	io::{Read, Write},
	path::{Path, PathBuf},
	sync::atomic::{AtomicU64, Ordering},
};

#[derive(Clone, Copy, Default, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReloadPolicy {
	#[default]
	RequireClean,
	DiscardUnsaved,
}

static NEXT_WORKSPACE: AtomicU64 = AtomicU64::new(1);

pub struct EditorWorkspace {
	pub generation: u64,
	pub backend: runtime::Engine,
	pub asset_root: PathBuf,
	pub scene_path: PathBuf,
	pub project_path: PathBuf,
	workspace_kind: WorkspaceKind,
	metadata: AssetManager,
	jobs: AssetImportService,
	pending: HashMap<AssetJobId, PendingAsset>,
	bounds_seen: HashSet<AssetRef>,
	completions: VecDeque<AssetCompletion>,
	completion_bytes: usize,
}
/// A manifest-backed workspace never falls back to standalone mode merely
/// because its manifest disappears from disk.
#[derive(Clone, Copy)]
enum WorkspaceKind {
	StandaloneScene,
	ManifestBacked,
}

enum PendingAsset {
	Bounds,
	Import { position: [f32; 3], revision: u64 },
}
#[derive(Clone, serde::Serialize)]
pub struct AssetCompletion {
	pub job_id: AssetJobId,
	pub workspace_generation: u64,
	pub accepted: bool,
	pub error: Option<String>,
	pub error_code: Option<&'static str>,
	pub revision: u64,
}
#[derive(serde::Serialize)]
pub struct AssetTicket {
	pub job_id: AssetJobId,
	pub workspace_generation: u64,
}
impl AssetCompletion {
	fn byte_cost(&self) -> usize {
		512 + self.error.as_ref().map_or(0, |message| message.len() * 6)
	}
}
fn completion_message(mut message: String) -> String {
	if message.len() > 1024 {
		let mut end = 1024;
		while !message.is_char_boundary(end) {
			end -= 1;
		}
		message.truncate(end);
	}
	message.into_boxed_str().into_string()
}
impl EditorWorkspace {
	#[allow(dead_code)] // Compatibility constructor for non-IPC callers.
	pub fn open(asset_root: &str, scene_path: &str) -> Result<Self, String> {
		Self::open_with_notifier(asset_root, scene_path, || {})
	}
	pub fn open_with_notifier(
		asset_root: &str,
		scene_path: &str,
		on_ready: impl Fn() + Send + 'static,
	) -> Result<Self, String> {
		let asset_root = Path::new(asset_root)
			.canonicalize()
			.map_err(|e| format!("Cannot open assets: {e}"))?;
		if !asset_root.is_dir() {
			return Err("Asset root must be a directory".into());
		}
		let relative_scene = scene_path;
		let scene_path = resolve_existing(&asset_root, relative_scene)?;
		let parsed = file_operation("load_scene", runtime::scene_io::load_data(&scene_path))?;
		let project_path = asset_root
			.parent()
			.ok_or("Assets need a project directory")?
			.join("project.json");
		if project_path.exists()
			&& fs::symlink_metadata(&project_path)
				.map_err(|e| e.to_string())?
				.file_type()
				.is_symlink()
		{
			return Err("Project manifest must not be a symbolic link".into());
		}
		let (project, workspace_kind) = if project_path.exists() {
			let project = ProjectData::load(&project_path)?;
			if resolve_existing(&asset_root, project.startup_scene_path()?)? != scene_path {
				return Err("Open the configured startup scene for this project".into());
			}
			(project, WorkspaceKind::ManifestBacked)
		} else {
			(
				ProjectData::for_scene("Untitled", relative_scene, &parsed.data)?,
				WorkspaceKind::StandaloneScene,
			)
		};
		let documents = load_project_documents(&asset_root, &project)?;
		let backend = runtime::EngineBuilder::new(parsed.data, project)
			.with_scenes(documents)
			.build()
			.map_err(|e| e.to_string())?;

		let metadata = AssetManager::new(&asset_root).map_err(|e| e.to_string())?;

		let jobs =
			AssetImportService::with_notifier(&asset_root, AssetJobLimits::default(), on_ready)?;

		let mut workspace = Self::generate_wrokspace(
			asset_root,
			scene_path,
			project_path,
			workspace_kind,
			backend,
			metadata,
			jobs,
		);
		workspace.cache_bounds();

		engine_logger::info!(target: "editor", "Scene workspace loaded: generation={}", workspace.generation);
		Ok(workspace)
	}

	pub fn open_project_with_notifier(
		location: ProjectLocation,
		on_ready: impl Fn() + Send + 'static,
	) -> Result<Self, String> {
		let asset_root = location.assets().to_path_buf();
		let project_path = location.manifest().to_path_buf();

		let project = ProjectData::load(location.manifest())?;

		let scene_path = resolve_existing(location.assets(), project.startup_scene_path()?)?;

		let parsed = file_operation("load_scene", runtime::scene_io::load_data(&scene_path))?;

		let documents = load_project_documents(&asset_root, &project)?;
		let backend = runtime::EngineBuilder::new(parsed.data, project)
			.with_scenes(documents)
			.build()
			.map_err(|e| e.to_string())?;

		let metadata = AssetManager::new(location.assets()).map_err(|e| e.to_string())?;

		let jobs = AssetImportService::with_notifier(
			location.assets(),
			AssetJobLimits::default(),
			on_ready,
		)?;

		let mut workspace = Self::generate_wrokspace(
			asset_root,
			scene_path,
			project_path,
			WorkspaceKind::ManifestBacked,
			backend,
			metadata,
			jobs,
		);
		workspace.cache_bounds();

		engine_logger::info!(target: "editor", "Scene workspace loaded: generation={}", workspace.generation);
		Ok(workspace)
	}

	fn cache_bounds(&mut self) {
		let data = self.backend.session().authored_document();
		let references: HashSet<_> = data
			.entities
			.iter()
			.filter(|e| e.renderable)
			.filter_map(|entity| {
				AssetRef::new(
					entity.model_path.clone(),
					Some(entity.mesh_index.unwrap_or(0)),
				)
				.ok()
			})
			.collect();
		self.bounds_seen
			.retain(|reference| references.contains(reference));
		for reference in references {
			if self.bounds_seen.contains(&reference)
				|| self.backend.assets().find(&reference).is_some()
			{
				continue;
			}
			if let Ok(id) = self.jobs.try_submit(reference.clone(), 64 * 1024 * 1024) {
				self.pending.insert(id, PendingAsset::Bounds);
				self.bounds_seen.insert(reference);
			}
		}
	}
	pub fn submit_import(
		&mut self,
		path: String,
		position: [f32; 3],
		revision: u64,
		reserved_bytes: usize,
	) -> Result<AssetTicket, String> {
		self.check_revision(revision)?;
		if self.backend.session().is_playing() || self.backend.session().has_transaction() {
			return Err("Stop Play and finish the edit before importing".into());
		}
		if position.iter().any(|v| !v.is_finite()) {
			return Err("Import position must be finite".into());
		}
		resolve_existing(&self.asset_root, &path)?;
		let id = self
			.jobs
			.try_submit(AssetRef::new(path, None)?, reserved_bytes)
			.map_err(|e| e.to_string())?;
		self.pending
			.insert(id, PendingAsset::Import { position, revision });
		Ok(AssetTicket {
			job_id: id,
			workspace_generation: self.generation,
		})
	}
	pub fn cancel_import(&self, id: AssetJobId, generation: u64) -> Result<bool, String> {
		if generation != self.generation {
			return Err("Asset job belongs to another workspace generation".into());
		}
		self.jobs.cancel(id).map_err(|e| e.to_string())
	}
	pub fn recent_completions(&self) -> Vec<AssetCompletion> {
		self.completions.iter().cloned().collect()
	}
	pub fn drain_asset_jobs(&mut self) -> Vec<AssetCompletion> {
		let mut completions = Vec::new();
		while let Some(completed) = self.jobs.try_drain() {
			let Some(purpose) = self.pending.remove(&completed.id) else {
				continue;
			};
			let result = completed
				.result
				.map_err(|error| match error {
					AssetJobError::Cancelled => ("cancelled", "Asset import cancelled".to_owned()),
					AssetJobError::Import(message) => ("import_failed", message),
					AssetJobError::ByteLimit { .. } => (
						"byte_limit",
						"Asset import exceeded its byte reservation".to_owned(),
					),
				})
				.and_then(|asset| {
					if let PendingAsset::Import { position, revision } = purpose {
						self.check_revision(revision)
							.map_err(|message| ("conflict", message))?;
						if self.backend.session().is_playing()
							|| self.backend.session().has_transaction()
						{
							return Err((
								"invalid_state",
								"Stop Play and finish the edit before adopting an import"
									.to_owned(),
							));
						}
						self.adopt_import(&completed.reference.path, &asset.value, position)
							.map_err(|message| ("adoption_rejected", message))?;
					} else {
						self.cache_imported_bounds(&completed.reference.path, &asset.value)
							.map_err(|message| ("metadata_rejected", message))?;
					}
					Ok(())
				});
			let (error_code, error) = match result {
				Ok(()) => (None, None),
				Err((code, message)) => (Some(code), Some(completion_message(message))),
			};
			let completion = AssetCompletion {
				job_id: completed.id,
				workspace_generation: self.generation,
				accepted: error.is_none(),
				error,
				error_code,
				revision: self.backend.session().revision(),
			};
			engine_logger::debug!(target:"editor","Asset job completed: generation={}, job={}, accepted={}",self.generation,completion.job_id.value(),completion.accepted);
			let cost = completion.byte_cost();
			while self.completions.len() >= 64
				|| self.completion_bytes.saturating_add(cost) > 128 * 1024
			{
				if let Some(old) = self.completions.pop_front() {
					self.completion_bytes -= old.byte_cost();
				} else {
					break;
				}
			}
			self.completion_bytes += cost;
			self.completions.push_back(completion.clone());
			completions.push(completion);
		}
		self.cache_bounds();
		completions
	}
	fn cache_imported_bounds(
		&mut self,
		path: &str,
		imported: &ImportedScene,
	) -> Result<(), String> {
		self.backend
			.register_assets(Self::imported_metadata(path, imported)?)
			.map_err(|e| e.to_string())?;
		Ok(())
	}
	fn imported_metadata(
		path: &str,
		imported: &ImportedScene,
	) -> Result<Vec<(AssetRef, AssetMetadata)>, String> {
		let count = imported.meshes.len().max(1) as u64;
		imported
			.meshes
			.iter()
			.map(|mesh| {
				let bounds = imported
					.mesh_bounds
					.get(mesh.mesh_index)
					.ok_or("Imported mesh bounds are missing")?;
				Ok((
					AssetRef::new(path.to_owned(), Some(mesh.mesh_index))?,
					AssetMetadata {
						bounds: *bounds,
						estimated_bytes: imported.decoded_mesh_bytes as u64 / count,
					},
				))
			})
			.collect()
	}
	pub fn execute(&mut self, request: EditorCommandRequest) -> Result<EditorSnapshotDto, String> {
		self.execute_receipt(request).map_err(|e| e.to_string())?;
		self.backend.snapshot().map_err(|e| e.to_string())
	}
	pub fn execute_receipt(
		&mut self,
		request: EditorCommandRequest,
	) -> Result<runtime::CommandReceipt, runtime::BackendError> {
		// Preserve coordinator error ordering before filesystem preparation.
		self.check_revision(request.expected_revision)
			.map_err(|message| runtime::BackendError {
				code: runtime::BackendErrorCode::Conflict,
				message,
				current_revision: Some(self.backend.session().revision()),
			})?;
		runtime::editor::CommandFactory::validate(&request.command).map_err(|message| {
			runtime::BackendError {
				code: runtime::BackendErrorCode::InvalidCommand,
				message,
				current_revision: Some(self.backend.session().revision()),
			}
		})?;
		// Prepare filesystem-dependent intent before the coordinator mutates.
		let preflight: Result<(), String> = (|| {
			match &request.command {
				Command::AddScene { asset_path, .. } => {
					if self.scene_output(asset_path, None)?.exists() {
						return Err("Scene creation requires a new path; register an existing file explicitly".into());
					}
				}
				Command::ChangeScenePath {
					scene_id,
					asset_path,
				} => {
					self.scene_output(asset_path, Some(scene_id))?;
					let target = resolve_existing(&self.asset_root, asset_path)?;
					for entry in self.backend.session().project().scenes.iter() {
						if entry.id() != scene_id
							&& resolve_existing(&self.asset_root, entry.asset_path())
								.ok()
								.as_ref() == Some(&target)
						{
							return Err("Scene path resolves to another catalog scene".into());
						}
					}
					let document = self.backend.session().scene_document(scene_id)?;
					if runtime::scene_io::load_data(&target)?.data != *document {
						return Err(
							"Scene path must contain the authored document; use Save As".into()
						);
					}
				}
				_ => {}
			}
			Ok(())
		})();
		preflight.map_err(|message| runtime::BackendError {
			code: runtime::BackendErrorCode::CommandRejected,
			message,
			current_revision: Some(self.backend.session().revision()),
		})?;
		let receipt = self.backend.execute(request)?;
		// Relative paths are already domain-validated. Resolve filesystem access
		// only at file operations; no fallible path read after a committed edit.
		self.scene_path = self.asset_root.join(
			self.backend
				.session()
				.project()
				.scene_path(self.backend.session().active_scene_id())
				.expect("active catalog entry"),
		);
		if receipt.changes.topology_changed || receipt.changes.full_resync {
			self.cache_bounds();
		}
		Ok(receipt)
	}
	/// Compare prospective destinations too: unsaved scenes have no file to
	/// canonicalize yet. Resolve parent aliases before comparing file names.
	fn scene_output(&self, path: &str, owner: Option<&SceneId>) -> Result<PathBuf, String> {
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

	/// Compatibility API: calling reload explicitly discards workspace edits.
	pub fn reload(&mut self, revision: u64) -> Result<EditorSnapshotDto, String> {
		self.reload_with_policy(revision, ReloadPolicy::DiscardUnsaved)
	}

	pub fn reload_with_policy(
		&mut self,
		revision: u64,
		policy: ReloadPolicy,
	) -> Result<EditorSnapshotDto, String> {
		self.check_revision(revision)?;
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
		let documents = load_project_documents(&self.asset_root, &project)?;
		let mut candidate = runtime::EngineBuilder::new(data, project)
			.with_scenes(documents)
			.build()
			.map_err(|e| e.to_string())?;
		candidate
			.supersede_revision(revision)
			.map_err(|e| e.to_string())?;
		let snapshot = candidate.snapshot().map_err(|e| e.to_string())?;
		self.jobs.advance_generation().map_err(|e| e.to_string())?;
		self.backend = candidate;
		// The replacement owns a fresh journal and scene revision sequence. A
		// new workspace generation also forces existing viewport caches to resync.
		self.generation = NEXT_WORKSPACE.fetch_add(1, Ordering::Relaxed);
		self.workspace_kind = next_kind;
		self.scene_path = scene_path;
		self.metadata.clear();
		self.pending.clear();
		self.bounds_seen.clear();
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

	/// Capture validated saved inputs under the workspace lock. Compilation and
	/// installation run after the caller releases that lock.
	pub fn prepare_build(&self, revision: u64) -> Result<compiler::PreparedProject, String> {
		self.check_revision(revision)?;
		let session = self.backend.session();
		if session.is_playing() || session.has_transaction() || session.is_dirty() {
			return Err(
				"Stop Play, finish edits, and save both scene and project settings before building"
					.into(),
			);
		}
		let disk = ProjectData::load(&self.project_path)?;
		if disk != *session.project() {
			return Err("Project settings changed on disk; reload before building".into());
		}
		for entry in disk.scenes.iter() {
			let scene = resolve_existing(&self.asset_root, entry.asset_path())?;
			let document = self.backend.session().scene_document(entry.id())?;
			if runtime::scene_io::load_data(&scene)?.data != *document {
				return Err("Project scene changed on disk; reload before building".into());
			}
		}
		compiler::prepare_project(&disk, &self.asset_root)
	}

	pub fn check_revision(&self, revision: u64) -> Result<(), String> {
		if revision != self.backend.session().revision() {
			return Err(format!(
				"Revision conflict: expected {revision}, current {}",
				self.backend.session().revision()
			));
		}
		Ok(())
	}

	pub fn import_model(
		&mut self,
		path: &str,
		position: [f32; 3],
		revision: u64,
	) -> Result<EditorSnapshotDto, String> {
		self.check_revision(revision)?;
		if !position.iter().all(|v| v.is_finite()) {
			return Err("Import position must be finite".into());
		}
		resolve_existing(&self.asset_root, path)?;
		let imported = file_operation(
			"import_model",
			self.metadata.load_scene(path).map_err(|e| e.to_string()),
		)?;
		self.adopt_import(path, &imported, position)?;
		self.backend.snapshot().map_err(|e| e.to_string())
	}
	fn adopt_import(
		&mut self,
		path: &str,
		imported: &ImportedScene,
		position: [f32; 3],
	) -> Result<(), String> {
		let remap: std::collections::HashMap<_, _> = imported
			.nodes
			.iter()
			.map(|node| (node.node_index, EntityKey::generate()))
			.collect();
		let mut entities = Vec::with_capacity(imported.nodes.len());
		let mut selection = Vec::new();
		for node in &imported.nodes {
			let key = remap
				.get(&node.node_index)
				.ok_or("Missing imported node identity")?
				.clone();
			let mut transform = node.local_transform;
			let parent = node
				.parent_node_index
				.map(|index| remap.get(&index).cloned().ok_or("Missing imported parent"))
				.transpose()?;
			if parent.is_none() {
				for (value, offset) in transform.position.iter_mut().zip(position) {
					*value += offset;
				}
				selection.push(key.clone());
			}
			let mut entity = EntityData::new(key, node.name.clone(), path)
				.with_renderable(node.mesh_index.is_some());
			entity.parent = parent;
			entity.mesh_index = node.mesh_index;
			entity.transform = runtime::scene_data::TransformData {
				position: transform.position,
				rotation: transform.rotation,
				scale: transform.scale,
			};
			entities.push(entity);
		}
		self.backend
			.replace_entities_with_assets(
				entities,
				selection,
				"Import glTF scene",
				Self::imported_metadata(path, imported)?,
			)
			.map_err(|e| e.to_string())?;
		engine_logger::info!(target: "editor", "Model scene imported: generation={}, nodes={}, meshes={}", self.generation, imported.nodes.len(), imported.mesh_bounds.len());
		Ok(())
	}
	pub fn instantiate_prefab(
		&mut self,
		prefab: PrefabData,
		position: [f32; 3],
		revision: u64,
	) -> Result<EditorSnapshotDto, String> {
		self.check_revision(revision)?;
		let count = prefab.entities.len();
		self.backend
			.instantiate_prefab(prefab, position)
			.map_err(|e| e.to_string())?;
		self.cache_bounds();
		engine_logger::info!(target: "editor", "Prefab instantiated: generation={}, entities={count}", self.generation);
		self.backend.snapshot().map_err(|e| e.to_string())
	}
	pub fn save_prefab(&self, path: &str, revision: u64) -> Result<String, String> {
		self.check_revision(revision)?;
		let prefab = self.backend.session().export_prefab()?;
		let output = resolve_output(&self.asset_root, path)?;
		if output.extension().and_then(|e| e.to_str()) != Some("json") {
			return Err("Prefab files use .json".into());
		}
		file_operation("save_prefab", atomic_json(&output, &prefab))?;
		engine_logger::info!(target: "editor", "Prefab saved: generation={}, entities={}", self.generation, prefab.entities.len());
		Ok(path.into())
	}
	pub fn load_prefab(
		&mut self,
		path: &str,
		position: [f32; 3],
		revision: u64,
	) -> Result<EditorSnapshotDto, String> {
		self.check_revision(revision)?;
		let path = resolve_existing(&self.asset_root, path)?;
		let bytes = file_operation("read_prefab", read_bounded(&path, 16 * 1024 * 1024))?;
		let prefab =
			PrefabData::from_json(std::str::from_utf8(&bytes).map_err(|e| e.to_string())?)?;
		let count = prefab.entities.len();
		self.backend
			.instantiate_prefab(prefab, position)
			.map_err(|e| e.to_string())?;
		self.cache_bounds();
		engine_logger::info!(target: "editor", "Prefab loaded: generation={}, entities={count}", self.generation);
		self.backend.snapshot().map_err(|e| e.to_string())
	}

	fn generate_wrokspace(
		asset_root: PathBuf,
		scene_path: PathBuf,
		project_path: PathBuf,
		workspace_kind: WorkspaceKind,
		backend: Engine,
		metadata: AssetManager,
		jobs: AssetImportService,
	) -> EditorWorkspace {
		Self {
			generation: NEXT_WORKSPACE.fetch_add(1, Ordering::Relaxed),
			backend,
			asset_root,
			scene_path,
			project_path,
			workspace_kind,
			metadata,
			jobs,
			pending: HashMap::new(),
			bounds_seen: HashSet::new(),
			completions: VecDeque::new(),
			completion_bytes: 0,
		}
	}
}

fn file_operation<T>(operation: &'static str, result: Result<T, String>) -> Result<T, String> {
	result.inspect_err(|_| {
		engine_logger::error!(target: "editor", "Editor file operation failed: operation={operation}");
	})
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
fn resolve_existing(root: &Path, path: &str) -> Result<PathBuf, String> {
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
fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
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

fn load_project_documents(
	root: &Path,
	project: &ProjectData,
) -> Result<Vec<(SceneId, runtime::scene_data::SceneData)>, String> {
	let mut seen = HashSet::new();
	let mut documents = Vec::new();
	let mut retained_bytes = 0usize;
	for entry in project.scenes.iter() {
		let path = resolve_existing(root, entry.asset_path())?;
		if !seen.insert(path.clone()) {
			return Err("Catalog entries resolve to the same scene file".into());
		}
		let document = file_operation("load_scene", runtime::scene_io::load_data(&path))?.data;
		retained_bytes = retained_bytes.saturating_add(
			document
				.to_json()?
				.len()
				.saturating_mul(4)
				.saturating_add(std::mem::size_of::<runtime::scene_data::SceneData>()),
		);
		if retained_bytes > runtime::editor::EditorSession::MAX_RETAINED_DOCUMENT_BYTES {
			return Err("Retained scene document byte budget exceeded".into());
		}
		project.validate_scene_for(entry.id(), &document)?;
		if entry.id() != &project.startup_scene_id {
			documents.push((entry.id().clone(), document));
		}
	}
	Ok(documents)
}
