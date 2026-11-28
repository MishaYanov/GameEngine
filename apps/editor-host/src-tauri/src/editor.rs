//! Project-scoped CPU application service. No window/input policy lives here.
#[path = "editor/agent_context.rs"]
pub(crate) mod agent_context;
#[path = "editor/asset_sources.rs"]
pub(crate) mod asset_sources;
#[path = "editor/assets.rs"]
mod assets;
#[path = "editor/audio_audition.rs"]
pub(crate) mod audio_audition;
#[path = "editor/documents.rs"]
pub(crate) mod documents;
#[path = "prefab_sources.rs"]
pub(crate) mod prefab_sources;
#[path = "prop_preview.rs"]
pub(crate) mod prop_preview;
#[path = "editor/recovery.rs"]
pub(crate) mod recovery;
#[path = "editor/scene_verification.rs"]
pub(crate) mod scene_verification;
#[path = "editor/scene_warming.rs"]
pub(crate) mod scene_warming;

use crate::project_location::ProjectLocation;
use assets::PendingAsset;
pub use assets::{AssetCompletion, AssetTicket};
use bridge::editor::{EditorCommandDto as Command, EditorCommandRequest, EditorSnapshotDto};
use documents::{
	load_project_documents, read_bounded, read_editor_scene, read_project_with_fingerprint,
	read_scene_with_fingerprint, resolve_existing, SavedFiles,
};
use runtime::scene_catalog::SceneId;
use runtime::{
	asset_jobs::{AssetJobError, AssetJobId, AssetJobLimits},
	asset_registry::{AssetMetadata, AssetRef},
	assets::{AssetImportService, AssetManager, ImportedScene},
	editor::PrefabData,
	project::ProjectData,
	scene_data::{EntityData, SceneData},
	Engine, EntityKey,
};
use std::{
	borrow::Cow,
	collections::{HashMap, HashSet, VecDeque},
	fs::{self, File, OpenOptions},
	io::{Read, Write},
	path::{Path, PathBuf},
	sync::{
		atomic::{AtomicU64, Ordering},
		Arc,
	},
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
	// A pause and resume may both finish before the native worker next wakes.
	pub(crate) camera_capture_epoch: u64,
	pub(crate) animation_preview: Option<crate::animation_preview::AnimationPreview>,
	pub(crate) animation_preview_request: u64,
	pub(crate) clip_timeline_drafts: crate::animation_preview::timeline::ClipTimelineDrafts,
	pub(crate) authored_clip_preview:
		Option<crate::animation_preview::authored::AuthoredClipPreview>,
	pub(crate) camera_preview: Option<crate::camera_preview::CameraPreview>,
	pub(crate) prop_preview: Option<prop_preview::PropPreview>,
	pub(crate) prop_preview_request: u64,
	pub(crate) audio_audition: audio_audition::AudioAuditionLane,
	prefab_source_drafts: prefab_sources::PrefabSourceDrafts,
	pub generation: u64,
	pub(crate) asset_sources: asset_sources::AssetSources,
	scene_verification_gate: Arc<std::sync::Mutex<()>>,
	pub(crate) scene_warming: scene_warming::SceneWarming,
	pub(crate) scene_switch: Option<(u64, std::time::Instant, std::time::Instant)>,
	pub backend: runtime::Engine,
	pub asset_root: PathBuf,
	pub scene_path: PathBuf,
	pub project_path: PathBuf,
	workspace_kind: WorkspaceKind,
	saved_files: SavedFiles,
	recovery: recovery::RecoveryState,
	metadata: AssetManager,
	jobs: AssetImportService,
	pending: HashMap<AssetJobId, PendingAsset>,
	bounds_seen: HashSet<AssetRef>,
	bounds_references: HashSet<AssetRef>,
	bounds_queue: VecDeque<AssetRef>,
	#[cfg(test)]
	bounds_reference_scans: usize,
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
		let (scene, startup_fingerprint) = read_scene_with_fingerprint(&scene_path, "load_scene")?;
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
		let (project, workspace_kind, manifest_fingerprint) = if project_path.exists() {
			let (project, fingerprint) =
				read_project_with_fingerprint(&project_path, "load_project")?;
			if resolve_existing(&asset_root, project.startup_scene_path()?)? != scene_path {
				return Err("Open the configured startup scene for this project".into());
			}
			(project, WorkspaceKind::ManifestBacked, Some(fingerprint))
		} else {
			(
				ProjectData::for_scene("Untitled", relative_scene, &scene)?,
				WorkspaceKind::StandaloneScene,
				None,
			)
		};
		let documents = load_project_documents(&asset_root, &project, &scene, startup_fingerprint)?;
		let saved_files = SavedFiles::new(manifest_fingerprint, documents.fingerprints);
		let backend = runtime::EngineBuilder::new(scene, project)
			.with_scenes(documents.scenes)
			.module_resolver(gameplay_catalog::project_resolver(&asset_root, gameplay_catalog::project_scripts::ScriptSource::Rust))
			.animation_resolver(animation_resolver(&asset_root))
			.input_actions_resolver(input_actions_resolver(&asset_root))
			.control_graph_resolver(control_graph_resolver(&asset_root))
			.animator_resolver(animator_resolver(&asset_root))
			.build()
			.map_err(|e| e.to_string())?;

		let metadata = AssetManager::new(&asset_root).map_err(|e| e.to_string())?;

		let jobs =
			AssetImportService::with_notifier(&asset_root, AssetJobLimits::default(), on_ready)?;

		let mut workspace = Self::new_workspace(
			asset_root,
			scene_path,
			project_path,
			workspace_kind,
			backend,
			metadata,
			jobs,
		);
		workspace.saved_files = saved_files;
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

		let (project, fingerprint) =
			read_project_with_fingerprint(location.manifest(), "load_project")?;
		let manifest_fingerprint = Some(fingerprint);

		let scene_path = resolve_existing(location.assets(), project.startup_scene_path()?)?;

		let (scene, startup_fingerprint) = read_scene_with_fingerprint(&scene_path, "load_scene")?;

		let documents = load_project_documents(&asset_root, &project, &scene, startup_fingerprint)?;
		let saved_files = SavedFiles::new(manifest_fingerprint, documents.fingerprints);
		let backend = runtime::EngineBuilder::new(scene, project)
			.with_scenes(documents.scenes)
			.module_resolver(gameplay_catalog::project_resolver(&asset_root, gameplay_catalog::project_scripts::ScriptSource::Rust))
			.animation_resolver(animation_resolver(&asset_root))
			.input_actions_resolver(input_actions_resolver(&asset_root))
			.control_graph_resolver(control_graph_resolver(&asset_root))
			.animator_resolver(animator_resolver(&asset_root))
			.build()
			.map_err(|e| e.to_string())?;

		let metadata = AssetManager::new(location.assets()).map_err(|e| e.to_string())?;

		let jobs = AssetImportService::with_notifier(
			location.assets(),
			AssetJobLimits::default(),
			on_ready,
		)?;

		let mut workspace = Self::new_workspace(
			asset_root,
			scene_path,
			project_path,
			WorkspaceKind::ManifestBacked,
			backend,
			metadata,
			jobs,
		);
		workspace.saved_files = saved_files;
		workspace.cache_bounds();

		engine_logger::info!(target: "editor", "Scene workspace loaded: generation={}", workspace.generation);
		Ok(workspace)
	}

	pub fn execute(&mut self, request: EditorCommandRequest) -> Result<EditorSnapshotDto, String> {
		self.execute_receipt(request).map_err(|e| e.to_string())?;
		self.backend.snapshot().map_err(|e| e.to_string())
	}
	pub fn execute_incremental(
		&mut self,
		request: EditorCommandRequest,
	) -> Result<bridge::editor::EditorCommandUpdateDto, runtime::BackendError> {
		use bridge::editor::EditorCommandUpdateDto;
		let before = self.backend.session().authored_document();
		let receipt = self.execute_receipt(request)?;
		let after = self.backend.session().authored_document();
		let document_changed = !std::sync::Arc::ptr_eq(&before, &after);
		// The change journal is a notification contract. Bindings and entity order
		// are not represented by its key lists, so explicitly resync those cases.
		let requires_snapshot = receipt.changes.full_resync
			|| receipt.changes.topology_changed
			|| receipt.project_changed
			|| (document_changed
				&& (before.bindings != after.bindings
					|| before.environment != after.environment
					|| before.default_game_camera != after.default_game_camera
					|| !before
						.entities
						.iter()
						.map(|e| &e.key)
						.eq(after.entities.iter().map(|e| &e.key))));
		if requires_snapshot {
			return Ok(EditorCommandUpdateDto::Snapshot {
				receipt,
				snapshot: self.backend.snapshot()?,
			});
		}
		let changed: HashSet<_> = receipt.changes.changed_entities.iter().collect();
		let entities = if changed.is_empty() {
			Vec::new()
		} else {
			after
				.entities
				.iter()
				.filter(|e| changed.contains(&e.key))
				.cloned()
				.collect()
		};
		Ok(EditorCommandUpdateDto::Delta {
			state: self.backend.session().state(),
			entities,
			camera: receipt.changes.camera_changed.then(|| after.camera.clone()),
			lights: receipt
				.changes
				.lights_changed
				.then(|| after.projected_lights())
				.transpose()
				.map_err(|message| runtime::BackendError {
					code: runtime::BackendErrorCode::Internal,
					message,
					current_revision: Some(receipt.revision),
				})?,
			organization: receipt
				.organization_changed
				.then(|| after.organization.clone()),
			receipt,
		})
	}
	/// Keep preview lifetimes and scene timing in sync after a host operation.
	pub(crate) fn with_viewport_update<T>(
		&mut self,
		operation: impl FnOnce(&mut Self) -> T,
	) -> (T, bool) {
		use std::time::Instant;

		let scene = self.backend.session().active_scene_id().clone();
		let requested = Instant::now();
		let (result, camera_closed) = self.with_camera_preview_cleanup(operation);
		let prop_closed = self.discard_stale_prop_preview();
		let animation_closed = self.discard_stale_animation_preview();
		let clip_closed = self.discard_stale_authored_clip_preview();
		self.discard_stale_audio_audition();
		let current = self.backend.session();
		if current.active_scene_id() != &scene {
			self.scene_switch = Some((current.scene_revision(), requested, Instant::now()));
		}
		(
			result,
			camera_closed || prop_closed || animation_closed || clip_closed,
		)
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
		if matches!(
			&request.command,
			Command::AddScene { .. } | Command::ChangeScenePath { .. }
		) {
			runtime::editor::CommandFactory::validate(&request.command).map_err(|message| {
				runtime::BackendError {
					code: runtime::BackendErrorCode::InvalidCommand,
					message,
					current_revision: Some(self.backend.session().revision()),
				}
			})?;
		}
		// Prepare filesystem-dependent intent before the coordinator mutates.
		let preflight: Result<(), String> = (|| {
			match &request.command {
				Command::AddComponent { component, .. }
				| Command::ReplaceComponent { component, .. } => {
					if let runtime::components::ComponentData::Sky { settings } = &component.data {
						if let Some(id) = settings.background.image() {
							let catalog =
								runtime::assets::project_assets::load_catalog(&self.asset_root)?;
							let asset = catalog.sky_panorama(id)?;
							bridge::sky_panorama::project_info(
								&self.asset_root,
								&asset.path,
								asset.bytes,
							)?;
						}
					}
					if let runtime::components::ComponentData::AudioSource { settings } =
						&component.data
					{
						component.validate()?;
						if let Some(clip) = &settings.clip {
							runtime::assets::project_assets::load_catalog(&self.asset_root)?
								.audio_clip(&clip.asset)?;
						}
					}
				}
				Command::SetPlayCameraInput {
					workspace_generation,
					..
				} if *workspace_generation != self.generation => {
					return Err("Camera input belongs to a previous workspace".into());
				}
				Command::InstantiateLinkedPrefab { .. }
				| Command::UpdateLinkedPrefab { .. }
				| Command::UpdateLinkedPrefabReviewed { .. }
				| Command::RelinkLinkedPrefab { .. } => {
					return Err("Linked source changes require the guarded prefab command and a reviewed source draft".into());
				}
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
		let pauses = matches!(&request.command, Command::SetPaused { paused: true });
		let receipt = self.backend.execute(request)?;
		self.discard_stale_camera_preview();
		self.discard_stale_prop_preview();
		self.discard_stale_audio_audition();
		if pauses {
			self.camera_capture_epoch = self.camera_capture_epoch.wrapping_add(1);
		}
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
			let disk_scene = read_editor_scene(&scene, "prepare_build_scene")?;
			if disk_scene != *document {
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
	/// Capture every retained authored scene, including inactive unsaved drafts.
	fn new_workspace(
		asset_root: PathBuf,
		scene_path: PathBuf,
		project_path: PathBuf,
		workspace_kind: WorkspaceKind,
		backend: Engine,
		metadata: AssetManager,
		jobs: AssetImportService,
	) -> EditorWorkspace {
		let recovery = recovery::RecoveryState::open(&project_path);
		Self {
			animation_preview: None,
			animation_preview_request: 0,
			clip_timeline_drafts: Default::default(),
			authored_clip_preview: None,
			camera_preview: None,
			prop_preview: None,
			prop_preview_request: 0,
			audio_audition: Default::default(),
			prefab_source_drafts: Default::default(),
			asset_sources: Default::default(),
			generation: NEXT_WORKSPACE.fetch_add(1, Ordering::Relaxed),
			scene_verification_gate: Default::default(),
			camera_capture_epoch: 0,
			scene_switch: None,
			scene_warming: scene_warming::SceneWarming::default(),
			backend,
			asset_root,
			scene_path,
			project_path,
			workspace_kind,
			saved_files: SavedFiles::default(),
			recovery,
			metadata,
			jobs,
			pending: HashMap::new(),
			bounds_seen: HashSet::new(),
			bounds_references: HashSet::new(),
			bounds_queue: VecDeque::new(),
			#[cfg(test)]
			bounds_reference_scans: 0,
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

fn animator_resolver(root: &Path) -> runtime::animation::AnimatorResolver {
	let root = root.to_path_buf();
	Arc::new(move |project, scene_id, scene| {
		bridge::animator_graph::prepare_scene(&root, project, scene_id, scene)
	})
}

fn input_actions_resolver(root: &Path) -> runtime::simulation_session::InputActionsResolver {
	let root = root.to_path_buf();
	Arc::new(move |scene| bridge::input_actions::load_scene(&root, scene))
}

fn control_graph_resolver(root: &Path) -> runtime::simulation_session::ControlGraphResolver {
	let root = root.to_path_buf();
	Arc::new(move |project, scene, map| {
		bridge::controls_graph::prepare_scene(&root, project, scene, map)
	})
}

fn animation_resolver(root: &Path) -> runtime::animation::AnimationResolver {
	let root = root.to_path_buf();
	Arc::new(move |scene| bridge::animation_asset::prepare_scene(&root, scene))
}
#[cfg(test)]
#[path = "../tests/unit/tests_editor_optimizations.rs"]
mod optimization_tests;
