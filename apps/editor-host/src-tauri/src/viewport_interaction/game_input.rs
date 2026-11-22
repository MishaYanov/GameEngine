use super::*;

/// Identity established by a successful native grab. Every sample retains it until release.
pub(crate) struct GameCameraCapture {
	workspace: u64,
	epoch: u64,
	pub(super) owner: CameraInputOwner,
}

impl GameCameraCapture {
	pub fn acquire(
		interaction: &ViewportInteraction,
		workspace: &EditorWorkspace,
		native_focused: bool,
		grab: impl FnOnce() -> Result<(), String>,
	) -> Result<Option<Self>, String> {
		if !workspace.backend.session().action_map_uses_pointer_look()
			|| !native_focused
			|| !interaction.keyboard_focused()
			|| !interaction.game_keyboard_active()
			|| interaction.generation != Some(workspace.generation)
		{
			return Ok(None);
		}
		let Some(owner) = workspace.backend.camera_input_owner() else {
			return Ok(None);
		};
		grab()?;
		Ok(Some(Self {
			workspace: workspace.generation,
			epoch: workspace.camera_capture_epoch,
			owner,
		}))
	}

	pub fn current(&self, workspace: &EditorWorkspace) -> bool {
		self.workspace == workspace.generation
			&& self.epoch == workspace.camera_capture_epoch
			&& workspace.backend.owns_camera_input(&self.owner)
	}

	pub fn submit(
		&self,
		workspace: &mut EditorWorkspace,
		intent: CameraIntent,
	) -> Result<(), String> {
		if self.workspace != workspace.generation || self.epoch != workspace.camera_capture_epoch {
			return Err("Game camera capture expired".into());
		}
		workspace.backend.submit_camera_input(
			&self.owner.run_id,
			self.owner.scene_generation,
			intent,
		)
	}
}

impl ViewportInteraction {
	/// Runtime status has its own publication path; it never invalidates an authored snapshot.
	pub fn sync_game_status(&mut self, workspace: &EditorWorkspace) {
		self.sync_orbit_status(workspace);
		self.game_status_revision =
			Some((workspace.generation, workspace.backend.session().revision()));
		let status = workspace.backend.play_status();
		if self.game_status != status {
			self.game_status = status;
			self.changed();
		}
	}

	pub(super) fn sync_orbit_status(&mut self, workspace: &EditorWorkspace) {
		let status = workspace
			.backend
			.session()
			.orbit_state()
			.map(|rig| OrbitStatusDto {
				blocker: rig
					.obstruction
					.as_ref()
					.map(|hit| hit.blocker.as_str().to_owned()),
				recovering: !rig.target_lost && !rig.arm_settled,
				target_lost: rig.target_lost,
			});
		if status != self.orbit_status {
			self.orbit_status = status;
			self.changed();
		}
	}

	pub fn sync_game_status_after_tick(
		&mut self,
		workspace: &EditorWorkspace,
		outcome: &Result<TickOutcome, BackendError>,
	) {
		// Progress, pending scene requests and a pause after rejection can all
		// change the HUD without touching the authored revision.
		let changed = match outcome {
			Ok(outcome) => {
				outcome.progress_changed
					|| outcome.requested_scene.is_some()
					|| !outcome.changes.is_empty()
			}
			Err(_) => true,
		};
		if changed {
			self.sync_game_status(workspace);
		} else if outcome
			.as_ref()
			.is_ok_and(|tick| tick.camera_status_changed)
		{
			self.sync_orbit_status(workspace);
		}
	}

	/// The Scene navigation camera stays in this controller, including during preview.
	pub fn game_camera(&self, workspace: &EditorWorkspace) -> Option<runtime::Camera> {
		workspace.active_camera_preview().cloned().or_else(|| {
			(self.settings.view == ViewportView::Game)
				.then(|| workspace.backend.session().render_camera())
		})
	}

	pub fn preview_active(&self) -> bool {
		self.camera_preview_active
	}

	pub fn keyboard_focused(&self) -> bool {
		self.focused
	}

	pub fn game_keyboard_active(&self) -> bool {
		self.playing && self.settings.view == ViewportView::Game
	}

	pub(super) fn set_keyboard_focus(
		&mut self,
		focused: bool,
		workspace: &mut EditorWorkspace,
	) -> Result<(), String> {
		if self.focused == focused && (focused || !self.captured()) {
			return Ok(());
		}
		self.focused = focused;
		self.changed();
		if !focused {
			// Internal asset drags belong to the WebView and handle app blur there.
			self.navigation = None;
			self.click = None;
			self.clear_play_input(workspace);
			self.cancel_transform(workspace)?;
		}
		Ok(())
	}

	pub fn clear_play_input(&mut self, workspace: &mut EditorWorkspace) {
		self.keys.clear();
		self.play_gamepad.reset();
		workspace.backend.clear_play_input();
	}

	pub(crate) fn route_game_input(
		&mut self,
		workspace: &mut EditorWorkspace,
	) -> Result<(), String> {
		let physical = self.play_gamepad.sample();
		workspace.backend.set_action_controls_from_codes(
			&self.keys,
			physical,
			runtime::input::ControllerBindings::default().resolve(physical),
		)
	}

	#[cfg(windows)]
	pub(crate) fn gamepad_context(&self, workspace: &EditorWorkspace) -> Option<(u64, u64)> {
		let session = workspace.backend.session();
		(self.game_keyboard_active()
			&& self.keyboard_focused()
			&& !self.preview_active()
			&& self.generation == Some(workspace.generation)
			&& session.is_playing()
			&& !session.is_paused())
		.then(|| (workspace.generation, session.revision()))
	}

	#[cfg(windows)]
	pub(crate) fn poll_gamepad(&mut self, now: std::time::Instant) -> bool {
		self.play_gamepad.poll(now, runtime::input::poll_gamepad)
	}

	#[cfg(windows)]
	pub(crate) fn gamepad_deadline(&self) -> Option<std::time::Instant> {
		self.play_gamepad.deadline()
	}

	#[cfg(windows)]
	pub(crate) fn reset_gamepad(&mut self) {
		self.play_gamepad.reset();
	}
}
