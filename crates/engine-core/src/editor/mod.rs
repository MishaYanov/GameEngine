//! CPU document application service. Transport/UI and GPU ownership stay outside.
mod factory;
mod history;
mod scene_documents;
pub use factory::CommandRequestBuilder;
use scene_documents::SceneDraft;
mod operations;
mod prefab;
use crate::command::{
	CommandDescriptorDto, EditorCommandDto as Command, EditorCommandRequest, EditorSnapshotDto,
	SnapSettingsDto,
};
use crate::{
	Bounds3, EntityKey,
	project::ProjectData,
	scene_catalog::SceneId,
	scene_data::SceneData,
	simulation_session::{
		ConfiguredBehaviorFactory, GameModuleFactory, SimulationSession, TickOutcome,
	},
};
use history::{History, Patch};
pub use prefab::PrefabData;
use std::{
	collections::{HashMap, HashSet},
	sync::Arc,
};

const MAX_SELECTION: usize = 4096;

pub struct EditorSession {
	active_scene_id: SceneId,
	saved_path: Option<String>,
	drafts: HashMap<SceneId, SceneDraft>,
	data: Arc<SceneData>,
	project: Arc<ProjectData>,
	saved_project: Arc<ProjectData>,
	module_factory: Arc<dyn GameModuleFactory>,
	saved: Arc<SceneData>,
	selection: Vec<EntityKey>,
	history: History,
	transaction: Option<Transaction>,
	play: Option<SimulationSession>,
	snap: SnapSettingsDto,
	revision: u64,
	scene_revision: u64,
	bounds: HashMap<(String, usize), Bounds3>,
}
struct Transaction {
	label: String,
	before: Arc<SceneData>,
	project: Arc<ProjectData>,
	selection: Vec<EntityKey>,
}

/// Factory builds validated command intent; widgets and shortcuts use the same path.
pub struct CommandFactory;
impl CommandFactory {
	pub fn capabilities() -> Vec<CommandDescriptorDto> {
		[
			("add_scene", "Add scene", "scene", true, false),
			("switch_scene", "Switch scene", "scene", false, false),
			("remove_scene", "Remove scene", "scene", true, false),
			(
				"set_startup_scene",
				"Set startup scene",
				"scene",
				true,
				false,
			),
			(
				"change_scene_path",
				"Change scene path",
				"scene",
				true,
				false,
			),
			("set_project", "Configure project", "project", true, false),
			(
				"set_entity_input",
				"Set controlled entity input",
				"simulation",
				false,
				true,
			),
			("select", "Select entities", "selection", false, true),
			("pick_selection", "Pick entity", "selection", false, false),
			("rename", "Rename", "entity", true, false),
			("spawn", "Create entity", "entity", true, false),
			("set_transforms", "Set transforms", "transform", true, false),
			(
				"translate_selection",
				"Move selection",
				"transform",
				true,
				false,
			),
			(
				"rotate_selection",
				"Rotate selection",
				"transform",
				true,
				false,
			),
			(
				"scale_selection",
				"Scale selection",
				"transform",
				true,
				false,
			),
			(
				"duplicate_selection",
				"Duplicate group",
				"entity",
				true,
				false,
			),
			("delete_selection", "Delete group", "entity", true, false),
			("reparent", "Change parent", "hierarchy", true, false),
			(
				"set_material",
				"Set material override",
				"material",
				true,
				false,
			),
			("set_collider", "Set collider", "collision", true, false),
			("upsert_light", "Set light", "lighting", true, false),
			("remove_light", "Remove light", "lighting", true, false),
			("set_camera", "Set camera", "camera", true, false),
			("frame_selection", "Frame selection", "camera", true, false),
			("orbit_camera", "Orbit", "camera", true, false),
			("pan_camera", "Pan", "camera", true, false),
			("dolly_camera", "Dolly", "camera", true, false),
			("set_snap", "Configure snapping", "settings", false, false),
			(
				"begin_transaction",
				"Begin grouped edit",
				"history",
				false,
				false,
			),
			(
				"commit_transaction",
				"Commit grouped edit",
				"history",
				true,
				false,
			),
			(
				"cancel_transaction",
				"Cancel grouped edit",
				"history",
				false,
				false,
			),
			("undo", "Undo", "history", false, false),
			("redo", "Redo", "history", false, false),
			("start_play", "Play", "simulation", false, false),
			("stop_play", "Stop", "simulation", false, true),
			("set_paused", "Pause simulation", "simulation", false, true),
			(
				"set_play_input",
				"Set movement intent",
				"simulation",
				false,
				true,
			),
		]
		.into_iter()
		.map(
			|(name, label, category, undoable, available_in_play)| CommandDescriptorDto {
				name: name.into(),
				label: label.into(),
				category: category.into(),
				undoable,
				available_in_play,
			},
		)
		.collect()
	}
}

impl EditorSession {
	pub fn new(data: SceneData) -> Result<Self, String> {
		let project = ProjectData::for_scene("Untitled", "scenes/scene.json", &data)?;
		Self::for_project(data, project)
	}
	pub fn for_project(data: SceneData, project: ProjectData) -> Result<Self, String> {
		project.validate_scene(&data)?;
		let active_scene_id = project.startup_scene_id.clone();
		let saved_path = Some(project.startup_scene_path()?.to_owned());
		let project = Arc::new(project);
		let data = Arc::new(data);
		Ok(Self {
			active_scene_id,
			saved_path,
			drafts: HashMap::new(),
			saved_project: Arc::clone(&project),
			project,
			module_factory: Arc::new(ConfiguredBehaviorFactory),
			saved: Arc::clone(&data),
			data,
			selection: Vec::new(),
			history: History::default(),
			transaction: None,
			play: None,
			snap: SnapSettingsDto::default(),
			revision: 0,
			scene_revision: 0,
			bounds: HashMap::new(),
		})
	}
	pub fn project(&self) -> &ProjectData {
		&self.project
	}
	pub fn project_document(&self) -> Arc<ProjectData> {
		Arc::clone(&self.project)
	}
	pub fn simulation_stats(&self) -> Option<(u64, u64, crate::collision::CollisionStats)> {
		self.play.as_ref().map(|play| {
			(
				play.tick_index(),
				play.world().matrix_updates(),
				play.collision_stats(),
			)
		})
	}
	pub fn set_module_factory(
		&mut self,
		factory: Arc<dyn GameModuleFactory>,
	) -> Result<(), String> {
		self.require_edit()?;
		self.module_factory = factory;
		Ok(())
	}
	pub fn render_world(&self) -> Option<&crate::world::RuntimeWorld> {
		self.play.as_ref().map(SimulationSession::world)
	}
	pub fn revision(&self) -> u64 {
		self.revision
	}
	/// A new project/session must not accept requests addressed to the previous
	/// document that happened to have the same numeric revision.
	pub fn supersede_revision(&mut self, previous: u64) -> Result<(), String> {
		let revision = self
			.revision
			.max(previous)
			.checked_add(1)
			.ok_or("Editor revision space is exhausted; restart the editor session")?;
		self.revision = revision;
		Ok(())
	}
	pub fn replace_document(&mut self, data: SceneData) -> Result<(), String> {
		if self.project.scenes.iter().count() != 1 {
			return Err("Single-scene replacement cannot replace a multi-scene workspace; prepare a complete workspace reload".into());
		}
		let mut project = (*self.project).clone();
		let keys: HashSet<_> = data.entities.iter().map(|e| &e.key).collect();
		project
			.behaviors
			.retain(|b| b.scene_id != self.active_scene_id || keys.contains(b.entity()));
		self.replace_project_document(data, project)
	}
	/// Replace both authored inputs only after validation. A reload establishes
	/// a fresh saved baseline and clears transient simulation and history.
	pub fn replace_project_document(
		&mut self,
		data: SceneData,
		project: ProjectData,
	) -> Result<(), String> {
		project.validate_scene(&data)?;
		self.require_no_transaction()?;
		self.ensure_revision_capacity()?;
		self.play = None;
		self.active_scene_id = project.startup_scene_id.clone();
		self.saved_path = Some(project.startup_scene_path()?.to_owned());
		self.drafts.clear();
		self.data = Arc::new(data);
		self.saved = Arc::clone(&self.data);
		self.project = Arc::new(project);
		self.saved_project = Arc::clone(&self.project);
		self.selection.clear();
		self.history = History::default();
		self.bounds.clear();
		self.scene_revision += 1;
		self.bump();
		Ok(())
	}
	pub fn scene_revision(&self) -> u64 {
		self.scene_revision
	}
	pub fn is_playing(&self) -> bool {
		self.play.is_some()
	}

	pub fn play_controlled_targets(&self) -> Option<&[EntityKey]> {
		self.play
			.as_ref()
			.map(SimulationSession::controlled_targets)
	}

	pub fn is_paused(&self) -> bool {
		self.play.as_ref().is_some_and(SimulationSession::paused)
	}
	/// Native lifecycle neutralization is transient input state, not an edit or
	/// a keyboard binding, so it does not advance the command revision.
	pub fn clear_play_input(&mut self) {
		if let Some(play) = self.play.as_mut() {
			play.clear_input();
		}
	}
	pub fn needs_tick(&self) -> bool {
		self.play
			.as_ref()
			.is_some_and(SimulationSession::needs_tick)
	}
	pub fn selection(&self) -> &[EntityKey] {
		&self.selection
	}
	/// GPU adapters retain resources only while current content or bounded undo
	/// records can restore an entity. The core never owns those GPU resources.
	pub fn retained_entity_keys(&self) -> HashSet<EntityKey> {
		let mut keys: HashSet<_> = self
			.data
			.entities
			.iter()
			.map(|entity| entity.key.clone())
			.collect();
		self.history.retain_entity_keys(&mut keys);
		if let Some(transaction) = &self.transaction {
			keys.extend(
				transaction
					.before
					.entities
					.iter()
					.map(|entity| entity.key.clone()),
			);
		}
		keys
	}
	pub fn authored_document(&self) -> Arc<SceneData> {
		Arc::clone(&self.data)
	}
	pub fn render_document(&self) -> Arc<SceneData> {
		self.play
			.as_ref()
			.map_or_else(|| Arc::clone(&self.data), SimulationSession::document)
	}
	pub fn has_transaction(&self) -> bool {
		self.transaction.is_some()
	}
	pub fn mark_saved(&mut self) -> Result<(), String> {
		if self.has_transaction() {
			return Err("Commit or cancel the current edit before saving".into());
		}
		if self.active_scene_dirty() || self.saved_project != self.project {
			self.ensure_revision_capacity()?;
			self.saved_path = Some(self.project.scene_path(&self.active_scene_id)?.to_owned());
			self.saved = Arc::clone(&self.data);
			self.saved_project = Arc::clone(&self.project);
			self.bump();
		}
		Ok(())
	}
	pub fn is_dirty(&self) -> bool {
		self.project != self.saved_project || self.scene_states().iter().any(|s| s.dirty)
	}
	pub fn mark_scene_saved(&mut self) -> Result<(), String> {
		self.require_no_transaction()?;
		if self.active_scene_dirty() {
			self.ensure_revision_capacity()?;
			self.saved_path = Some(self.project.scene_path(&self.active_scene_id)?.to_owned());
			self.saved = Arc::clone(&self.data);
			self.bump();
		}
		Ok(())
	}
	pub fn mark_project_saved(&mut self) -> Result<(), String> {
		self.require_no_transaction()?;
		if self.saved_project != self.project {
			self.ensure_revision_capacity()?;
			self.saved_project = Arc::clone(&self.project);
			self.bump();
		}
		Ok(())
	}
	pub fn set_model_bounds(&mut self, path: String, mesh: usize, bounds: Bounds3) {
		self.bounds.insert((path, mesh), bounds);
	}
	pub(crate) fn clear_model_bounds(&mut self) {
		self.bounds.clear();
	}
	pub fn snapshot(&self) -> Result<EditorSnapshotDto, String> {
		Ok(EditorSnapshotDto {
			active_scene_id: self.active_scene_id.clone(),
			project_dirty: self.project != self.saved_project,
			scenes: self.scene_states(),
			revision: self.revision,
			scene_revision: self.scene_revision,
			document: self.data.as_ref().clone(),
			project: self.project.as_ref().clone(),
			selection: self
				.selection
				.iter()
				.map(|key| key.as_str().to_owned())
				.collect(),
			mode: if self.is_playing() { "play" } else { "edit" }.into(),
			paused: self.play.as_ref().is_some_and(SimulationSession::paused),
			dirty: self.is_dirty(),
			can_undo: self.history.can_undo(),
			can_redo: self.history.can_redo(),
			transaction: self.transaction.as_ref().map(|t| t.label.clone()),
			snap: self.snap.clone(),
		})
	}

	pub fn execute(&mut self, request: EditorCommandRequest) -> Result<EditorSnapshotDto, String> {
		self.execute_command(request)?;
		self.snapshot()
	}

	/// Execute intent without allocating a frontend snapshot. Explicit queries
	/// may request the complete document; hot command paths need only receipts.
	pub fn execute_command(&mut self, request: EditorCommandRequest) -> Result<(), String> {
		CommandFactory::validate(&request.command)?;
		self.execute_validated_command(request)
	}
	pub(crate) fn execute_validated_command(
		&mut self,
		request: EditorCommandRequest,
	) -> Result<(), String> {
		if request.expected_revision != self.revision {
			return Err(format!(
				"Revision conflict: expected {}, current {}",
				request.expected_revision, self.revision
			));
		}
		self.ensure_revision_capacity()?;
		let command = request.command;
		match command {
			Command::AddScene {
				id,
				asset_path,
				document,
			} => self.add_scene(id, asset_path, document)?,
			Command::SwitchScene { scene_id } => self.switch_scene(scene_id)?,
			Command::RemoveScene { scene_id } => self.remove_scene(scene_id)?,
			Command::SetStartupScene { scene_id } => self.set_startup_scene(scene_id)?,
			Command::ChangeScenePath {
				scene_id,
				asset_path,
			} => self.change_scene_path(scene_id, asset_path)?,
			Command::PickSelection {
				pixel,
				extent,
				additive,
			} => {
				self.require_edit()?;
				let data = self.data.as_ref();
				let camera = crate::Camera {
					position: data.camera.position,
					yaw: data.camera.yaw,
					pitch: data.camera.pitch,
					vertical_fov: data.camera.vertical_fov,
					near: data.camera.near_plane,
					far: data.camera.far_plane,
				};
				let ray = crate::picking::CameraRay::from_screen(&camera, pixel, extent)?;
				let world = data.world_matrices()?;
				let mut nearest = ray.max_distance;
				let mut selected = None;
				for (entity, matrix) in data.entities.iter().zip(world) {
					if !entity.renderable {
						continue;
					}
					let Some(bounds) = self
						.bounds
						.get(&(entity.model_path.clone(), entity.mesh_index.unwrap_or(0)))
					else {
						continue;
					};
					if !matrix.is_finite() || matrix.determinant() == 0.0 {
						continue;
					}
					let inverse = matrix.inverse();
					if let Some(distance) = bounds.ray_distance(
						inverse.transform_point3(ray.origin),
						inverse.transform_vector3(ray.direction),
						nearest,
					) && (selected.is_none() || distance < nearest)
					{
						nearest = distance;
						selected = Some(entity.key.clone());
					}
				}
				let mut selection = if additive {
					self.selection.clone()
				} else {
					Vec::new()
				};
				if let Some(key) = selected
					&& !selection.contains(&key)
				{
					if selection.len() >= MAX_SELECTION {
						return Err("Selection exceeds 4096 entities".into());
					}
					selection.push(key);
				}
				if selection != self.selection {
					self.selection = selection;
					self.bump();
				}
			}
			Command::Select { keys } => {
				if keys.len() > MAX_SELECTION {
					return Err("Selection exceeds 4096 entities".into());
				}
				let mut selection = Vec::with_capacity(keys.len());
				let document = self
					.play
					.as_ref()
					.map_or(self.data.as_ref(), |play| play.world().data());
				let available: HashSet<_> = document
					.entities
					.iter()
					.map(|entity| entity.key.as_str())
					.collect();
				let mut unique = HashSet::with_capacity(keys.len());
				for key in keys {
					let key = EntityKey::new(key)?;
					if !available.contains(key.as_str()) {
						return Err("Selection contains a missing entity".into());
					}
					if unique.insert(key.clone()) {
						selection.push(key);
					}
				}
				if selection != self.selection {
					self.selection = selection;
					self.bump();
				}
			}
			Command::SetProject { project } => {
				if project.scenes != self.project.scenes {
					return Err("Use scene catalog commands to change scene entries".into());
				}
				self.require_edit()?;
				self.commit_project(
					(*self.data).clone(),
					project,
					self.selection.clone(),
					"Configure project",
				)?;
			}
			Command::StartPlay => {
				self.require_edit()?;
				self.require_no_transaction()?;
				self.play = Some(SimulationSession::with_factory_for_scene(
					(*self.project).clone(),
					self.active_scene_id.clone(),
					Arc::clone(&self.data),
					self.module_factory.as_ref(),
				)?);
				self.scene_revision += 1;
				self.bump();
			}
			Command::StopPlay => {
				if self.play.take().is_some() {
					self.scene_revision += 1;
					self.bump();
				}
			}
			Command::SetPaused { paused } => {
				let play = self.play.as_mut().ok_or("Play mode is not active")?;
				if play.paused() != paused {
					play.set_paused(paused);
					self.bump();
				}
			}
			Command::SetPlayInput { movement } => {
				let play = self.play.as_mut().ok_or("Play mode is not active")?;
				if play.set_controlled_input(movement)? {
					self.bump();
				}
			}
			Command::SetEntityInput { entity, movement } => {
				let entity = EntityKey::new(entity)?;
				let play = self.play.as_mut().ok_or("Play mode is not active")?;
				if play.set_input(&entity, movement)? {
					self.bump();
				}
			}
			Command::SetSnap { settings } => {
				self.require_edit()?;
				if ![
					settings.translation,
					settings.rotation_radians,
					settings.scale,
				]
				.iter()
				.all(|v| v.is_finite() && *v > 0.0 && *v <= 1e6)
				{
					return Err("Snap increments must be finite and positive".into());
				}
				if self.snap != settings {
					self.snap = settings;
					self.bump();
				}
			}
			Command::BeginTransaction { label } => {
				self.require_edit()?;
				self.require_no_transaction()?;
				if label.trim().is_empty()
					|| label.len() > 256
					|| label.chars().any(char::is_control)
				{
					return Err("An edit label needs 1–256 bytes".into());
				}
				self.transaction = Some(Transaction {
					label,
					before: Arc::clone(&self.data),
					project: Arc::clone(&self.project),
					selection: self.selection.clone(),
				});
				self.bump();
			}
			Command::CommitTransaction => {
				self.require_edit()?;
				let transaction = self
					.transaction
					.as_ref()
					.ok_or("No grouped edit is active")?;
				let patch = Patch::between(
					&transaction.before,
					&self.data,
					&transaction.project,
					&self.project,
					&transaction.selection,
					&self.selection,
					transaction.label.clone(),
				)?
				.in_scene(self.active_scene_id.clone());
				self.history.record(patch)?;
				self.transaction = None;
				self.bump();
			}
			Command::CancelTransaction => {
				self.require_edit()?;
				let transaction = self.transaction.take().ok_or("No grouped edit is active")?;
				self.data = transaction.before;
				self.project = transaction.project;
				self.selection = transaction.selection;
				self.scene_revision += 1;
				self.bump();
			}
			Command::Undo | Command::Redo => {
				self.restore_history(matches!(command, Command::Redo))?
			}
			mutation => {
				self.require_edit()?;
				let mut next = (*self.data).clone();
				let mut selection = self.selection.clone();
				operations::apply(
					&mut next,
					&mut selection,
					&self.snap,
					&self.bounds,
					mutation,
				)?;
				self.commit(next, selection, "Edit scene")?;
			}
		}
		Ok(())
	}

	pub fn replace_entities(
		&mut self,
		entities: Vec<crate::scene_data::EntityData>,
		selection: Vec<EntityKey>,
		label: &str,
	) -> Result<EditorSnapshotDto, String> {
		self.replace_entities_command(entities, selection, label)?;
		self.snapshot()
	}
	pub fn replace_entities_command(
		&mut self,
		entities: Vec<crate::scene_data::EntityData>,
		selection: Vec<EntityKey>,
		label: &str,
	) -> Result<(), String> {
		self.require_edit()?;
		let mut next = (*self.data).clone();
		next.entities.extend(entities);
		self.commit(next, selection, label)
	}
	fn commit(
		&mut self,
		next: SceneData,
		selection: Vec<EntityKey>,
		label: &str,
	) -> Result<(), String> {
		let mut project = (*self.project).clone();
		let keys: HashSet<_> = next.entities.iter().map(|e| &e.key).collect();
		project
			.behaviors
			.retain(|b| b.scene_id != self.active_scene_id || keys.contains(b.entity()));
		self.commit_project(next, project, selection, label)
	}
	fn commit_project(
		&mut self,
		next: SceneData,
		project: ProjectData,
		selection: Vec<EntityKey>,
		label: &str,
	) -> Result<(), String> {
		self.validate_project_documents(&project, &self.active_scene_id, &next)?;
		self.check_document_budget(Some((&self.active_scene_id, &next)))?;
		validate_selection(&next, &selection)?;
		if next == *self.data && project == *self.project && selection == self.selection {
			return Ok(());
		}
		self.ensure_revision_capacity()?;
		if self.transaction.is_none() {
			let patch = Patch::between(
				&self.data,
				&next,
				&self.project,
				&project,
				&self.selection,
				&selection,
				label.into(),
			)?
			.in_scene(self.active_scene_id.clone());
			self.history.record(patch)?;
		}
		self.install_project(next, project, selection);
		Ok(())
	}
	fn install_project(
		&mut self,
		next: SceneData,
		project: ProjectData,
		selection: Vec<EntityKey>,
	) {
		if self.data.entities != next.entities || self.data.lights != next.lights {
			self.scene_revision += 1;
		}
		self.data = Arc::new(next);
		self.project = Arc::new(project);
		self.selection = selection;
		self.bump();
	}
	fn bump(&mut self) {
		self.revision += 1;
	}
	fn ensure_revision_capacity(&self) -> Result<(), String> {
		if self.revision == u64::MAX || self.scene_revision == u64::MAX {
			Err("Editor revision space is exhausted; restart the editor session".into())
		} else {
			Ok(())
		}
	}
	fn require_edit(&self) -> Result<(), String> {
		if self.is_playing() {
			Err("Stop Play before changing authored content".into())
		} else {
			Ok(())
		}
	}
	fn require_no_transaction(&self) -> Result<(), String> {
		if self.has_transaction() {
			Err("Commit or cancel the current grouped edit first".into())
		} else {
			Ok(())
		}
	}

	pub fn tick(&mut self, elapsed: f64) -> Result<TickOutcome, String> {
		if !elapsed.is_finite() || elapsed < 0.0 {
			return Err("Elapsed time must be finite and nonnegative".into());
		}
		if self.scene_revision == u64::MAX {
			return Err("Render revision space exhausted".into());
		}
		let Some(play) = self.play.as_mut() else {
			return Ok(TickOutcome::default());
		};
		let outcome = play.tick(elapsed)?;
		if !outcome.changes.is_empty() {
			self.scene_revision += 1;
		}
		Ok(outcome)
	}
}

fn validate_selection(data: &SceneData, selection: &[EntityKey]) -> Result<(), String> {
	if selection.len() > MAX_SELECTION {
		return Err("Selection exceeds 4096 entities".into());
	}
	let available: HashSet<_> = data.entities.iter().map(|entity| &entity.key).collect();
	let mut unique = HashSet::with_capacity(selection.len());
	for key in selection {
		if !available.contains(key) || !unique.insert(key) {
			return Err("Selection must contain distinct existing entities".into());
		}
	}
	Ok(())
}
