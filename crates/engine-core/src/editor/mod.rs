//! CPU document application service. Transport/UI and GPU ownership stay outside.
mod play;
mod selection;
mod transaction;
use selection::validate_selection;
mod factory;
mod history;
mod scene_documents;
pub use factory::{CommandFactory, CommandRequestBuilder};
use scene_documents::{DocumentCostCache, SceneDraft};
mod linked_prefab;
mod operations;
mod prefab;
mod runtime_lifecycle;
use crate::camera_input::CameraIntent;
use crate::command::{
	EditorCommandDto as Command, EditorCommandRequest, EditorSnapshotDto, PlayStatusDto,
	SnapSettingsDto,
};
use crate::game_progress::{GameProgress, ProgressAction};
use crate::game_session::{GameSession, PreparedScene};
use crate::simulation_session::MovementTarget;
use crate::{
	Bounds3, EntityKey,
	input::{GameKeyboardInput, InputProfile},
	project::ProjectData,
	scene_catalog::SceneId,
	scene_data::SceneData,
	simulation_session::{
		GameModuleFactory, GameModuleResolver, TickOutcome, resolve_default_gameplay,
	},
};
use history::{History, Patch};
pub use prefab::{PrefabData, PrefabGameplay};
use std::{
	collections::{HashMap, HashSet},
	sync::Arc,
};

const MAX_SELECTION: usize = 4096;

pub struct EditorSession {
	active_scene_id: SceneId,
	saved_path: Option<String>,
	drafts: HashMap<SceneId, SceneDraft>,
	document_costs: DocumentCostCache,
	data: Arc<SceneData>,
	project: Arc<ProjectData>,
	saved_project: Arc<ProjectData>,
	module_resolver: GameModuleResolver,
	animation_resolver: Option<crate::animation::AnimationResolver>,
	animator_resolver: Option<crate::animation::AnimatorResolver>,
	input_actions_resolver: Option<crate::simulation_session::InputActionsResolver>,
	control_graph_resolver: Option<crate::simulation_session::ControlGraphResolver>,
	saved: Arc<SceneData>,
	selection: Vec<EntityKey>,
	history: History,
	transaction: Option<Transaction>,
	play: Option<GameSession>,
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

impl EditorSession {
	pub fn new(data: SceneData) -> Result<Self, String> {
		let project = ProjectData::for_scene("Untitled", "scenes/scene.json", &data)?;
		Self::for_project(data, project)
	}
	pub fn for_project(mut data: SceneData, project: ProjectData) -> Result<Self, String> {
		data.refresh_game_camera_projection()?;
		project.validate_scene(&data)?;
		let active_scene_id = project.startup_scene_id.clone();
		let saved_path = Some(project.startup_scene_path()?.to_owned());
		let project = Arc::new(project);
		let data = Arc::new(data);
		Ok(Self {
			active_scene_id,
			saved_path,
			drafts: HashMap::new(),
			document_costs: DocumentCostCache::default(),
			saved_project: Arc::clone(&project),
			project,
			module_resolver: Arc::new(resolve_default_gameplay),
			animation_resolver: None,
			animator_resolver: None,
			input_actions_resolver: None,
			control_graph_resolver: None,
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
				play.active().tick_index(),
				play.active().world().matrix_updates(),
				play.active().collision_stats(),
			)
		})
	}
	/// Full deterministic native diagnostics for the active play scene; edit
	/// mode has no runtime component state. Opaque game-module state is excluded.
	pub fn native_replay_state(&self) -> serde_json::Value {
		self.play
			.as_ref()
			.map(|run| run.active().native_replay_state())
			.unwrap_or(serde_json::Value::Null)
	}
	/// Only the live Play camera contributes to replay evidence.
	pub fn camera_replay_state(
		&self,
	) -> Result<Option<crate::camera_replay::CameraReplayProjection>, String> {
		self.play
			.as_ref()
			.map(GameSession::camera_replay_state)
			.transpose()
	}

	pub fn set_module_factory(
		&mut self,
		factory: Arc<dyn GameModuleFactory>,
	) -> Result<(), String> {
		self.set_module_resolver(Arc::new(move |_| Ok(Arc::clone(&factory))))
	}

	pub fn set_module_resolver(&mut self, resolver: GameModuleResolver) -> Result<(), String> {
		self.require_edit()?;
		self.module_resolver = resolver;
		Ok(())
	}

	pub fn render_world(&self) -> Option<&crate::world::RuntimeWorld> {
		self.play.as_ref().map(|session| session.active().world())
	}
	pub fn set_animation_resolver(
		&mut self,
		resolver: crate::animation::AnimationResolver,
	) -> Result<(), String> {
		self.require_edit()?;
		self.animation_resolver = Some(resolver);
		Ok(())
	}
	pub fn set_animator_resolver(
		&mut self,
		resolver: crate::animation::AnimatorResolver,
	) -> Result<(), String> {
		self.require_edit()?;
		self.animator_resolver = Some(resolver);
		Ok(())
	}

	pub fn set_input_actions_resolver(
		&mut self,
		resolver: crate::simulation_session::InputActionsResolver,
	) -> Result<(), String> {
		self.require_edit()?;
		self.input_actions_resolver = Some(resolver);
		Ok(())
	}
	pub fn set_control_graph_resolver(
		&mut self,
		resolver: crate::simulation_session::ControlGraphResolver,
	) -> Result<(), String> {
		self.require_edit()?;
		self.control_graph_resolver = Some(resolver);
		Ok(())
	}
	pub fn particles(&self) -> impl Iterator<Item=crate::particles::ParticleSample> + '_ {
		self.play.iter().flat_map(|run| run.active().particles())
	}

	pub fn animations(&self) -> Option<&crate::animation::AnimationBank> {
		self.play.as_ref().map(|run| run.active().animations())
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
		mut data: SceneData,
		project: ProjectData,
	) -> Result<(), String> {
		data.refresh_game_camera_projection()?;
		project.validate_scene(&data)?;
		self.require_no_transaction()?;
		self.ensure_revision_capacity()?;
		self.play = None;
		self.active_scene_id = project.startup_scene_id.clone();
		self.saved_path = Some(project.startup_scene_path()?.to_owned());
		self.drafts.clear();
		self.document_costs = DocumentCostCache::default();
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
			.map(|session| session.active().controlled_targets())
	}

	pub fn is_paused(&self) -> bool {
		self.play
			.as_ref()
			.is_some_and(|play| play.active().paused())
	}
	/// Native lifecycle neutralization is transient input state, not an edit or
	/// a keyboard binding, so it does not advance the command revision.
	pub fn clear_play_input(&mut self) {
		if let Some(play) = self.play.as_mut() {
			play.active_mut().clear_input();
		}
	}

	pub fn set_game_keyboard_input(
		&mut self,
		target: &MovementTarget,
		input: GameKeyboardInput,
	) -> Result<(), String> {
		self.play
			.as_mut()
			.ok_or("Play mode is not active")?
			.active_mut()
			.set_game_keyboard_input(target, input)
	}

	pub fn set_game_keyboard_input_from_codes(
		&mut self,
		held_codes: &HashSet<String>,
	) -> Result<(), String> {
		let defaults = InputProfile::default();
		let profile = self.project.input_profile.as_ref().unwrap_or(&defaults);
		let input = profile.resolve_keyboard_codes(held_codes);
		self.set_game_keyboard_input(&MovementTarget::AllControlled, input)
	}

	pub fn needs_tick(&self) -> bool {
		self.play.as_ref().is_some_and(GameSession::needs_tick)
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
	/// Every authored replacement is validated before this session installs it.
	pub fn validated_authored_document(&self) -> crate::scene_data::ValidatedScene<'_> {
		crate::scene_data::ValidatedScene::from_validated(&self.data)
	}

	pub fn render_document(&self) -> Arc<SceneData> {
		self.play
			.as_ref()
			.map_or_else(|| Arc::clone(&self.data), |play| play.active().document())
	}
	pub fn orbit_state(&self) -> Option<&crate::game_camera::OrbitRuntimeState> {
		self.play.as_ref()?.active().orbit_state()
	}

	pub fn render_camera(&self) -> crate::Camera {
		if let Some(play) = &self.play {
			return play.active().camera().clone();
		}
		self.data.camera.to_camera()
	}
	pub fn set_play_visibility_capabilities(
		&mut self,
		available: HashSet<EntityKey>,
	) -> Result<(), String> {
		self.play
			.as_mut()
			.ok_or("Play mode is not active")?
			.active_mut()
			.set_visibility_capabilities(available)
	}
	pub fn has_play_visibility_capabilities(&self) -> bool {
		self.play
			.as_ref()
			.is_some_and(|play| play.active().has_visibility_capabilities())
	}

	pub fn has_transaction(&self) -> bool {
		self.transaction.is_some()
	}

	pub fn mark_saved(&mut self) -> Result<(), String> {
		if self.has_transaction() {
			return Err("Commit or cancel the current edit before saving".into());
		}
		if self.active_scene_dirty() || self.project_dirty() {
			self.ensure_revision_capacity()?;
			self.saved_path = Some(self.project.scene_path(&self.active_scene_id)?.to_owned());
			self.saved = Arc::clone(&self.data);
			self.saved_project = Arc::clone(&self.project);
			self.bump();
		}
		Ok(())
	}

	pub fn is_dirty(&self) -> bool {
		self.project_dirty()
			|| self
				.project
				.scenes
				.iter()
				.any(|entry| self.scene_dirty(entry).unwrap_or(false))
	}

	fn project_dirty(&self) -> bool {
		snapshots_differ(&self.project, &self.saved_project)
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
		if self.project_dirty() {
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
		let state = self.state();
		Ok(EditorSnapshotDto {
			active_scene_id: state.active_scene_id,
			play_session_id: state.play_session_id,
			project_dirty: state.project_dirty,
			scenes: state.scenes,
			revision: state.revision,
			scene_revision: state.scene_revision,
			document: self.data.as_ref().clone(),
			project: self.project.as_ref().clone(),
			selection: state.selection,
			mode: state.mode,
			paused: state.paused,
			dirty: state.dirty,
			can_undo: state.can_undo,
			can_redo: state.can_redo,
			transaction: state.transaction,
			snap: state.snap,
		})
	}

	/// Authoritative editor controls without cloning the scene or project.
	pub fn state(&self) -> crate::command::EditorStateDto {
		let scenes = self.scene_states();
		let project_dirty = self.project_dirty();
		let dirty = project_dirty || scenes.iter().any(|scene| scene.dirty);
		crate::command::EditorStateDto {
			active_scene_id: self.active_scene_id.clone(),
			play_session_id: self
				.play
				.as_ref()
				.map(|play| play.play_session_id().to_owned()),
			project_dirty,
			scenes,
			revision: self.revision,
			scene_revision: self.scene_revision,
			selection: self
				.selection
				.iter()
				.map(|key| key.as_str().to_owned())
				.collect(),
			mode: if self.is_playing() { "play" } else { "edit" }.into(),
			paused: self
				.play
				.as_ref()
				.is_some_and(|play| play.active().paused()),
			dirty,
			can_undo: self.history.can_undo(),
			can_redo: self.history.can_redo(),
			transaction: self.transaction.as_ref().map(|t| t.label.clone()),
			snap: self.snap.clone(),
		}
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
			} => self.pick_selection(pixel, extent, additive)?,
			Command::Select { keys } => self.select_keys(keys)?,
			Command::SetProject { project } => {
				if project.scenes != self.project.scenes {
					return Err("Use scene catalog commands to change scene entries".into());
				}
				self.require_edit()?;
				if project.gameplay_module != self.project.gameplay_module {
					(self.module_resolver)(&project)?;
				}
				self.commit_project(
					(*self.data).clone(),
					project,
					self.selection.clone(),
					"Configure project",
				)?;
			}
			Command::CreatePlayer => {
				self.require_edit()?;
				let mut next = (*self.data).clone();
				let mut project = (*self.project).clone();
				let key = operations::create_player(&mut next, &mut project)?;
				self.commit_project(next, project, vec![key], "Create Player")?;
			}
			Command::StartPlay => self.start_play()?,
			Command::StopPlay => self.stop_play(),
			Command::SetPaused { paused } => self.set_paused(paused)?,
			Command::SetPlayInput { movement } => self.set_play_input(movement)?,
			Command::SetCharacterInput {
				key,
				movement_xz,
				jump_held,
			} => self.set_play_character_input(key, movement_xz, jump_held)?,
			Command::SetEntityInput { entity, movement } => {
				self.set_entity_input(entity, movement)?
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
			Command::BeginTransaction { label } => self.begin_transaction(label)?,
			Command::CommitTransaction => self.commit_transaction()?,
			Command::CancelTransaction => self.cancel_transaction()?,
			Command::Undo | Command::Redo => {
				self.restore_history(matches!(command, Command::Redo))?
			}
			Command::InstantiateLinkedPrefab {
				source,
				position,
				bindings,
			} => self.instantiate_linked_prefab(source, position, bindings)?,
			Command::UpdateLinkedPrefab {
				root_key,
				source,
				resolution,
				bindings,
			} => self.update_linked_prefab(root_key, source, resolution, bindings)?,
			Command::UpdateLinkedPrefabReviewed {
				root_key,
				source,
				bindings,
				resolutions,
			} => self.update_linked_prefab_reviewed(root_key, source, bindings, resolutions)?,
			Command::RevertLinkedPrefab { root_key, property } => {
				self.revert_linked_prefab(root_key, property)?
			}
			Command::UnpackLinkedPrefab { root_key } => self.unpack_linked_prefab(root_key)?,
			Command::UnpackLinkedPrefabOuter { root_key } => {
				self.unpack_linked_prefab_outer(root_key)?
			}
			Command::RelinkLinkedPrefab { root_key, source } => {
				self.relink_linked_prefab(root_key, source)?
			}
			Command::SetPlayCameraInput {
				workspace_generation: _,
				run_id,
				scene_generation,
				yaw_delta,
				pitch_delta,
				log_distance_delta,
			} => {
				let input = CameraIntent {
					yaw_delta,
					pitch_delta,
					log_distance_delta,
				};
				self.submit_camera_input(&run_id, scene_generation, input)?;
				if !input.is_empty() {
					self.bump();
				}
			}
			mutation => {
				self.require_edit()?;

				if let Command::SetSceneBinding { scene_id, .. } = &mutation
					&& scene_id != &self.active_scene_id
				{
					return Err("Scene binding command must target the active scene".into());
				}

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
		mut next: SceneData,
		project: ProjectData,
		selection: Vec<EntityKey>,
		label: &str,
	) -> Result<(), String> {
		crate::linked_prefab::capture_overrides(&self.data, &mut next)?;
		next.refresh_game_camera_projection()?;
		self.validate_project_documents(&project, &self.active_scene_id, &next)?;
		Self::document_budget(
			&self.active_scene_id,
			&self.data,
			&self.saved,
			&self.drafts,
			Some((&self.active_scene_id, &next)),
			&mut self.document_costs,
		)?;
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
	/// Asset publication changes rendering, not authored scene/project content or history.
	pub fn invalidate_assets(&mut self) -> Result<(), String> {
		self.require_edit()?;
		self.require_no_transaction()?;
		self.ensure_revision_capacity()?;
		self.scene_revision += 1;
		self.bump();
		Ok(())
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
}

fn snapshots_differ<T: PartialEq>(current: &Arc<T>, saved: &Arc<T>) -> bool {
	!Arc::ptr_eq(current, saved) && current != saved
}
