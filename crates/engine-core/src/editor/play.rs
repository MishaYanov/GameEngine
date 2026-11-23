use super::*;
use crate::input::GamepadSample;

impl EditorSession {
	pub(super) fn start_play(&mut self) -> Result<(), String> {
		self.require_edit()?;
		self.require_no_transaction()?;
		let factory = (self.module_resolver)(&self.project)?;
		let mut run = GameSession::with_factory_for_scene(
			Arc::clone(&self.project),
			self.active_scene_id.clone(),
			Arc::clone(&self.data),
			factory,
		)?;
		if let Some(resolver) = &self.animation_resolver {
			run.set_animation_resolver(Arc::clone(resolver))?;
		}
		run.active().require_animation_ready()?;
		if let Some(resolver) = &self.animator_resolver {
			run.set_animator_resolver(Arc::clone(resolver))?;
		}
		run.active().require_animators_ready()?;
		if let Some(resolver) = &self.input_actions_resolver {
			run.set_input_actions_resolver(Arc::clone(resolver))?;
		}
		run.active().require_input_actions_ready()?;
		if let Some(resolver) = &self.control_graph_resolver {
			run.set_control_graph_resolver(Arc::clone(resolver))?;
		}
		run.active().require_control_graph_ready()?;
		self.play = Some(run);
		self.scene_revision += 1;
		self.bump();
		engine_logger::info!(target: "engine_core::play", "Play started: revision={}", self.revision);
		Ok(())
	}

	pub(super) fn stop_play(&mut self) {
		if self.play.take().is_some() {
			self.scene_revision += 1;
			self.bump();
			engine_logger::info!(target: "engine_core::play", "Play stopped: revision={}", self.revision);
		}
	}

	pub(super) fn set_paused(&mut self, paused: bool) -> Result<(), String> {
		let play = self.play.as_mut().ok_or("Play mode is not active")?;
		if play.active().paused() != paused {
			play.active_mut().set_paused(paused);
			self.bump();
			engine_logger::debug!(target: "engine_core::play", "Play pause changed: paused={paused}, revision={}", self.revision);
		}
		Ok(())
	}

	pub(super) fn set_play_input(&mut self, movement: [f32; 3]) -> Result<(), String> {
		let play = self.play.as_mut().ok_or("Play mode is not active")?;
		if play
			.active_mut()
			.set_targeted_input(&MovementTarget::AllControlled, movement)?
		{
			self.bump();
		}
		Ok(())
	}

	pub(super) fn set_play_character_input(
		&mut self,
		key: String,
		movement_xz: [f32; 2],
		jump_held: bool,
	) -> Result<(), String> {
		let key = EntityKey::new(key)?;
		self.play
			.as_mut()
			.ok_or("Play mode is not active")?
			.active_mut()
			.set_character_input(&key, movement_xz, jump_held)?;
		self.bump();
		Ok(())
	}

	pub(super) fn set_entity_input(
		&mut self,
		entity: String,
		movement: [f32; 3],
	) -> Result<(), String> {
		let entity = EntityKey::new(entity)?;
		let play = self.play.as_mut().ok_or("Play mode is not active")?;
		if play
			.active_mut()
			.set_targeted_input(&MovementTarget::Entity(entity), movement)?
		{
			self.bump();
		}
		Ok(())
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

	/// Samples keyboard and controller input together for the current Play owner.
	pub fn set_game_controls_from_codes(
		&mut self,
		held_codes: &HashSet<String>,
		gamepad: GamepadSample,
	) -> Result<(), String> {
		let defaults = InputProfile::default();
		let profile = self.project.input_profile.as_ref().unwrap_or(&defaults);
		let input = profile.resolve_controls_from_codes(held_codes, gamepad);
		self.set_game_keyboard_input(&MovementTarget::AllControlled, input)
	}

	pub fn sample_action_codes(
		&mut self,
		held_codes: &HashSet<String>,
		physical: crate::input::PhysicalGamepadSample,
	) -> Result<bool, String> {
		self.play
			.as_mut()
			.ok_or("Play mode is not active")?
			.active_mut()
			.sample_action_codes(held_codes, physical)
	}

	pub fn sample_action_keys(
		&mut self,
		state: &crate::InputState,
		physical: crate::input::PhysicalGamepadSample,
	) -> Result<bool, String> {
		self.play
			.as_mut()
			.ok_or("Play mode is not active")?
			.active_mut()
			.sample_action_keys(state, physical)
	}

	pub fn has_authored_input_actions(&self) -> bool {
		self.play
			.as_ref()
			.is_some_and(|run| run.active().input_action_map().is_some())
	}

	pub fn play_input_action_map(&self) -> Option<&crate::input::InputActionMap> {
		self.play.as_ref()?.active().input_action_map()
	}

	/// Replace the runtime map after validating a local Player keyboard edit.
	pub fn replace_play_input_action_map(
		&mut self,
		map: crate::input::InputActionMap,
	) -> Result<(), String> {
		let active = self
			.play
			.as_mut()
			.ok_or("Play mode is not active")?
			.active_mut();
		active.install_input_action_map(Some(map))?;
		active.clear_input();
		Ok(())
	}

	pub fn action_map_binds_key(&self, key: crate::Key) -> Option<bool> {
		self.play.as_ref()?.active().authored_action_binds_key(key)
	}

	pub fn action_map_uses_pointer_look(&self) -> bool {
		self.play
			.as_ref()
			.is_none_or(|run| run.active().uses_pointer_look())
	}

	pub fn play_has_game_camera_owner(&self) -> bool {
		self.play
			.as_ref()
			.is_some_and(|game| game.active().owns_game_camera_view())
	}

	pub fn camera_input_owner(&self) -> Option<crate::camera_input::CameraInputOwner> {
		self.play.as_ref()?.camera_input_owner()
	}

	pub fn owns_camera_input(&self, owner: &crate::camera_input::CameraInputOwner) -> bool {
		self.play
			.as_ref()
			.is_some_and(|play| play.owns_camera_input(owner))
	}

	pub fn submit_camera_input(
		&mut self,
		run_id: &str,
		scene_generation: u64,
		intent: CameraIntent,
	) -> Result<(), String> {
		self.play
			.as_mut()
			.ok_or("Play mode is not active")?
			.submit_camera_input(run_id, scene_generation, intent)
	}

	/// Checks the owning run and scene generation without building a status DTO.
	pub fn owns_play_scene(&self, run_id: &str, scene_generation: u64) -> bool {
		self.play.as_ref().is_some_and(|play| {
			play.run_id() == run_id && play.scene_generation() == scene_generation
		})
	}

	pub fn play_status(&self) -> Option<PlayStatusDto> {
		self.play.as_ref().map(GameSession::status)
	}

	pub(crate) fn play_run_id(&self) -> Option<&str> {
		self.play.as_ref().map(GameSession::run_id)
	}

	pub fn play_progress<P: GameProgress>(&self) -> Option<&P> {
		self.play.as_ref()?.progress::<P>()
	}
	pub fn apply_play_progress(&mut self, actions: &[ProgressAction]) -> Result<(), String> {
		self.require_no_transaction()?;
		self.ensure_revision_capacity()?;
		if self
			.play
			.as_mut()
			.ok_or("Play mode is not active")?
			.apply_progress_actions(actions)?
		{
			self.bump();
		}
		Ok(())
	}
	pub fn prepare_new_game(
		&self,
		scene_id: SceneId,
		document: Arc<SceneData>,
	) -> Result<PreparedScene, String> {
		self.require_no_transaction()?;
		self.ensure_revision_capacity()?;
		self.play
			.as_ref()
			.ok_or("Play mode is not active")?
			.prepare_new_game(scene_id, document)
	}

	pub fn prepare_restored_game<P: GameProgress>(
		&self,
		scene_id: SceneId,
		document: Arc<SceneData>,
		progress: P,
	) -> Result<PreparedScene, String> {
		self.require_no_transaction()?;
		self.ensure_revision_capacity()?;
		self.play
			.as_ref()
			.ok_or("Play mode is not active")?
			.prepare_restored_game(scene_id, document, progress)
	}
	pub fn play_scene_id(&self) -> Option<&SceneId> {
		self.play.as_ref().map(|play| play.active().scene_id())
	}

	pub fn pending_play_scene(&self) -> Option<&SceneId> {
		self.play.as_ref()?.pending_scene()
	}

	pub fn prepare_pending_play_scene(&self) -> Result<(PreparedScene, Arc<SceneData>), String> {
		self.require_no_transaction()?;
		self.ensure_revision_capacity()?;
		let play = self.play.as_ref().ok_or("Play mode is not active")?;
		let scene_id = play
			.pending_scene()
			.ok_or("No runtime scene request is pending")?;
		let document = self.scene_document(scene_id)?;
		let prepared = play.prepare_pending_scene(Arc::clone(&document))?;
		Ok((prepared, document))
	}

	pub fn prepare_play_scene(
		&self,
		scene_id: SceneId,
		document: Arc<SceneData>,
	) -> Result<PreparedScene, String> {
		self.require_no_transaction()?;
		self.ensure_revision_capacity()?;

		self.play
			.as_ref()
			.ok_or("Play mode is not active")?
			.prepare_scene(scene_id, document)
	}

	pub fn commit_play_scene(&mut self, prepared: PreparedScene) -> Result<(), String> {
		self.require_no_transaction()?;
		self.ensure_revision_capacity()?;

		self.play
			.as_mut()
			.ok_or("Play mode is not active")?
			.commit_scene(prepared)?;

		self.scene_revision += 1;
		Ok(())
	}
}
