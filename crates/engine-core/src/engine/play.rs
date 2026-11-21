use super::*;

impl Engine {
	pub fn camera_replay_state(
		&self,
	) -> Result<Option<crate::camera_replay::CameraReplayProjection>, String> {
		self.session.camera_replay_state()
	}

	pub fn tick(&mut self, elapsed: f64) -> Result<TickOutcome, BackendError> {
		if !elapsed.is_finite() || elapsed < 0.0 {
			return Err(self.error(
				BackendErrorCode::InvalidCommand,
				"Elapsed time must be finite and nonnegative",
			));
		}
		self.ensure_event_capacity()?;
		let outcome = match self.session.tick(elapsed) {
			Ok(outcome) => outcome,
			Err(message) => {
				// Simulation rejects and rolls back an action batch, then pauses.
				// Consumers still need the state transition even on this error.
				self.publish_runtime(BackendEventKind::StateChanged)?;
				return Err(self.error(BackendErrorCode::SimulationRejected, message));
			}
		};
		if outcome.progress_changed {
			self.publish_runtime(BackendEventKind::StateChanged)?;
		}
		if outcome.steps > 0 {
			self.publish_runtime(BackendEventKind::SimulationAdvanced {
				tick: outcome.tick,
				changes: outcome.changes.clone(),
				camera_status_changed: outcome.camera_status_changed,
			})?;
		}
		Ok(outcome)
	}

	pub fn camera_input_owner(&self) -> Option<crate::camera_input::CameraInputOwner> {
		self.session.camera_input_owner()
	}

	pub fn owns_camera_input(&self, owner: &crate::camera_input::CameraInputOwner) -> bool {
		self.session.owns_camera_input(owner)
	}

	/// Native input is ephemeral and does not create authored changes or advance the command revision.
	pub fn submit_camera_input(
		&mut self,
		run_id: &str,
		scene_generation: u64,
		intent: crate::camera_input::CameraIntent,
	) -> Result<(), String> {
		self.session
			.submit_camera_input(run_id, scene_generation, intent)
	}

	pub fn play_status(&self) -> Option<PlayStatusDto> {
		self.session.play_status()
	}

	pub fn pending_runtime_spawns(&self) -> Vec<crate::runtime_lifecycle::RuntimeSpawnRequest> {
		self.session.pending_runtime_spawns()
	}

	pub fn prepare_runtime_spawn(
		&self,
		prefab: &crate::editor::PrefabData,
		fingerprint: &str,
		position: [f32; 3],
		request: Option<u64>,
	) -> Result<crate::runtime_lifecycle::PreparedRuntimeSpawn, String> {
		self.session
			.prepare_runtime_spawn(prefab, fingerprint, position, request)
	}

	pub fn runtime_spawn_document(
		&self,
		prepared: &crate::runtime_lifecycle::PreparedRuntimeSpawn,
	) -> Result<SceneData, String> {
		self.session.runtime_spawn_document(prepared)
	}

	pub fn commit_runtime_spawn(
		&mut self,
		prepared: crate::runtime_lifecycle::PreparedRuntimeSpawn,
	) -> Result<(), BackendError> {
		self.ensure_event_capacity()?;
		let previous_orbit = self.session.orbit_state().cloned();
		let changes = self
			.session
			.commit_runtime_spawn(prepared)
			.map_err(|message| self.error(BackendErrorCode::CommandRejected, message))?;
		self.publish_runtime(BackendEventKind::SimulationAdvanced {
			tick: self.session.simulation_stats().map_or(0, |stats| stats.0),
			changes,
			camera_status_changed: previous_orbit
				.as_ref()
				.zip(self.session.orbit_state())
				.is_some_and(|(previous, next)| !previous.same_status(next)),
		})?;
		Ok(())
	}

	pub fn reject_runtime_spawn(&mut self, id: u64, error: &str) -> Result<(), BackendError> {
		self.ensure_event_capacity()?;
		self.session
			.reject_runtime_spawn(id, error)
			.map_err(|message| self.error(BackendErrorCode::CommandRejected, message))?;
		self.publish_runtime(BackendEventKind::StateChanged)?;
		Ok(())
	}

	pub fn runtime_inspection(
		&self,
		offset: usize,
		limit: usize,
	) -> Result<crate::runtime_lifecycle::RuntimeInspectionDto, String> {
		self.session.runtime_inspection(offset, limit)
	}

	pub fn set_play_visibility_capabilities(
		&mut self,
		available: std::collections::HashSet<crate::EntityKey>,
	) -> Result<(), String> {
		self.session.set_play_visibility_capabilities(available)
	}

	pub fn has_play_visibility_capabilities(&self) -> bool {
		self.session.has_play_visibility_capabilities()
	}

	/// Validates the UI's source run and authoring revision before preparing
	/// or installing a restart that was prepared while rendering was unlocked.
	pub fn validate_restart_play(
		&self,
		run_id: &str,
		expected_revision: u64,
	) -> Result<(), BackendError> {
		if expected_revision != self.session.revision() {
			return Err(self.error(
				BackendErrorCode::Conflict,
				"The editor changed before restart. Try again.",
			));
		}
		let current = self.session.play_run_id().ok_or_else(|| {
			self.error(BackendErrorCode::CommandRejected, "Play mode is not active")
		})?;
		if current != run_id {
			return Err(self.error(
				BackendErrorCode::Conflict,
				"The game run changed before restart. Try again.",
			));
		}
		Ok(())
	}

	pub fn prepare_restart_play(
		&self,
		run_id: &str,
		expected_revision: u64,
	) -> Result<(PreparedScene, Arc<SceneData>), BackendError> {
		self.validate_restart_play(run_id, expected_revision)?;
		let startup = self.session.project().startup_scene_id.clone();
		let document = self
			.session
			.scene_document(&startup)
			.map_err(|message| self.error(BackendErrorCode::CommandRejected, message))?;
		let prepared = self
			.session
			.prepare_new_game(startup, Arc::clone(&document))
			.map_err(|message| self.error(BackendErrorCode::CommandRejected, message))?;
		Ok((prepared, document))
	}

	pub fn pending_play_scene(&self) -> Option<&SceneId> {
		self.session.pending_play_scene()
	}

	pub fn prepare_pending_play_scene(
		&self,
	) -> Result<(PreparedScene, Arc<SceneData>), BackendError> {
		self.session
			.prepare_pending_play_scene()
			.map_err(|message| self.error(BackendErrorCode::CommandRejected, message))
	}

	pub fn commit_play_scene(&mut self, prepared: PreparedScene) -> Result<(), BackendError> {
		self.ensure_event_capacity()?;
		let from = self.session.play_scene_id().cloned().ok_or_else(|| {
			self.error(BackendErrorCode::CommandRejected, "Play mode is not active")
		})?;
		self.session
			.commit_play_scene(prepared)
			.map_err(|message| self.error(BackendErrorCode::CommandRejected, message))?;
		let to = self
			.session
			.play_scene_id()
			.expect("committed Play scene")
			.clone();
		self.publish_runtime(BackendEventKind::RuntimeSceneChanged { from, to })
	}

	pub fn apply_play_progress(
		&mut self,
		actions: &[crate::game_progress::ProgressAction],
	) -> Result<CommandReceipt, BackendError> {
		self.mutate_session(|session| session.apply_play_progress(actions))
	}

	pub fn clear_play_input(&mut self) {
		self.session.clear_play_input();
	}

	/// Update transient gameplay input without adding a document revision or history entry.
	pub fn set_game_keyboard_input_from_codes(
		&mut self,
		held_codes: &HashSet<String>,
	) -> Result<(), String> {
		self.session.set_game_keyboard_input_from_codes(held_codes)
	}

	fn publish_runtime(&mut self, event: BackendEventKind) -> Result<(), BackendError> {
		let scene_id = self
			.session
			.play_scene_id()
			.unwrap_or_else(|| self.session.active_scene_id());
		self.publish_for_scene(scene_id.clone(), event)
	}
}
