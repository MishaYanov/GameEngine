//! Authoritative Player host for authored replacement routes.

use super::*;
use runtime::{game_session::SceneTransitionAttempt, scene_prefetch::ConfirmedScene};

pub(super) struct WaitingScene {
	pub attempt: SceneTransitionAttempt,
	pub waiting_frame_drawn: bool,
	timing: SceneLoadTiming,
}

impl GameApp {
	pub(super) fn drive_authored_scene_transition(&mut self) -> Result<bool, String> {
		if self.scene_transition.is_none()
			&& self.input.just_pressed(Key::F8)
			&& let Some(failed) = self.failed_scene_transition.take()
		{
			if let Err(error) = self.editor.retry_scene_transition(&failed) {
				self.failed_scene_transition = Some(failed);
				return Err(error);
			}
		}
		let pending = self
			.editor
			.pending_scene_transition()
			.filter(|attempt| attempt.route_id().is_some())
			.cloned();
		let Some(attempt) = pending else {
			if self.scene_transition.is_some() {
				self.invalidate_scene_prefetch();
			}
			self.prime_approaching_route();
			return Ok(false);
		};
		if self
			.scene_transition
			.as_ref()
			.is_none_or(|current| current.attempt.id() != attempt.id())
		{
			self.failed_scene_transition = None;
			self.set_mouse_capture(false);
			self.scene_transition = Some(WaitingScene {
				attempt: attempt.clone(),
				waiting_frame_drawn: false,
				timing: SceneLoadTiming::new("player", SceneLoadScope::Play, Instant::now()),
			});
		}
		if self.input.just_pressed(Key::F7) {
			self.editor
				.finish_unsuccessful_scene_transition(&attempt, false)?;
			self.invalidate_scene_prefetch();
			self.input.suppress_held();
			self.clock.reset();
			return Ok(true);
		}
		let result = self.read_pending_scene(&attempt);
		let loaded = match result {
			Ok(Some(loaded)) => loaded,
			Ok(None) => return Ok(true),
			Err(error) => return self.fail_authored_scene_transition(attempt, error),
		};
		let mut waiting = self
			.scene_transition
			.take()
			.expect("pending route has timing");
		let result = self.install_confirmed_scene(&attempt, loaded, &mut waiting.timing);
		match result {
			Ok(()) => {
				self.scene_load_timing = Some(waiting.timing);
				Ok(true)
			}
			Err(error) => {
				waiting.timing.finish(SceneLoadOutcome::Failed);
				self.fail_authored_scene_transition(attempt, error)
			}
		}
	}

	fn read_pending_scene(
		&self,
		attempt: &SceneTransitionAttempt,
	) -> Result<Option<ConfirmedScene>, String> {
		let project = self
			.editor
			.project()
			.ok_or("Project configuration is unavailable")?;
		let worker = self
			.scene_prefetch
			.as_ref()
			.ok_or("Scene loading worker is unavailable")?;
		let path = project.scene_path(attempt.scene_id())?;
		if !worker.confirm(attempt.scene_id(), path) {
			return Ok(None);
		}
		if self
			.scene_transition
			.as_ref()
			.is_none_or(|waiting| !waiting.waiting_frame_drawn)
		{
			return Ok(None);
		}
		worker.take_confirmed(attempt.scene_id()).transpose()
	}

	fn install_confirmed_scene(
		&mut self,
		attempt: &SceneTransitionAttempt,
		loaded: ConfirmedScene,
		timing: &mut SceneLoadTiming,
	) -> Result<(), String> {
		scene_prefetch::record_load(
			timing,
			attempt.scene_id(),
			loaded.outcome,
			loaded.retained_estimate_bytes,
		);
		let prepared = self
			.editor
			.prepare_scene_transition(attempt, Arc::new(loaded.document.data))?;
		let document = prepared.world_data();
		self.adopt_prepared_scene(&document, prepared, loaded.path, timing)
	}

	fn fail_authored_scene_transition(
		&mut self,
		attempt: SceneTransitionAttempt,
		error: String,
	) -> Result<bool, String> {
		if let Some(waiting) = self.scene_transition.take() {
			waiting.timing.finish(SceneLoadOutcome::Failed);
		}
		self.editor
			.finish_unsuccessful_scene_transition(&attempt, true)?;
		self.invalidate_scene_prefetch();
		self.failed_scene_transition = Some(attempt);
		self.input.suppress_held();
		self.clock.reset();
		Err(error)
	}

	fn prime_approaching_route(&mut self) {
		let (Some(prefetch), Some(project)) = (&mut self.scene_prefetch, self.editor.project())
		else {
			return;
		};
		if prefetch
			.speculative_route()
			.is_some_and(|id| self.editor.should_retain_scene_preload(id))
		{
			return;
		}
		prefetch.set_approach(self.editor.approach_scene_route(), project);
	}
}
