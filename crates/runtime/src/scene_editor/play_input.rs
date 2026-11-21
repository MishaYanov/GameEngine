//! Gameplay and camera input routing for the active Play session.

use super::{MovementTarget, SceneEditor};
use crate::{
	EntityKey, InputState, Key,
	editor::command::EditorCommandDto as Command,
	input::{GamepadSample, InputProfile, MovementBindings},
};
use engine_core::camera_input::{CameraInputOwner, CameraIntent};

impl SceneEditor {
	pub fn set_play_input(&mut self, movement: [f32; 3]) -> Result<(), String> {
		self.execute_core(Command::SetPlayInput { movement })?;
		Ok(())
	}
	/// Capture every physical transition, including a down/up pair before a frame.
	pub fn sample_play_keyboard(&mut self, input: &InputState) -> Result<(), String> {
		self.sample_play_controls(input, GamepadSample::default())
	}
	pub fn sample_play_controls(
		&mut self,
		input: &InputState,
		gamepad: GamepadSample,
	) -> Result<(), String> {
		if !self.is_playing() {
			return Ok(());
		}
		let profile = self.play_input_profile().cloned().unwrap_or_default();
		let controls = profile.resolve_controls(input, gamepad);
		self.session
			.as_mut()
			.expect("playing session")
			.set_game_keyboard_input(&self.play_input_target, controls)
	}
	pub fn camera_input_owner(&self) -> Option<CameraInputOwner> {
		self.session.as_ref()?.camera_input_owner()
	}

	pub fn owns_camera_input(&self, owner: &CameraInputOwner) -> bool {
		self.session
			.as_ref()
			.is_some_and(|session| session.owns_camera_input(owner))
	}

	pub fn submit_camera_input(
		&mut self,
		owner: &CameraInputOwner,
		intent: CameraIntent,
	) -> Result<(), String> {
		self.session
			.as_mut()
			.ok_or("Play mode is not active")?
			.submit_camera_input(&owner.run_id, owner.scene_generation, intent)
	}

	pub fn clear_play_keyboard(&mut self) {
		self.clear_play_input();
	}

	pub fn clear_play_input(&mut self) {
		if let Some(session) = &mut self.session {
			session.clear_play_input();
		}
	}

	pub fn set_play_movement_bindings(
		&mut self,
		bindings: MovementBindings,
		input: &mut InputState,
	) -> bool {
		let mut profile = self.play_input_profile().cloned().unwrap_or_default();
		profile.movement = bindings;
		self.set_play_input_profile(profile, input)
	}

	pub fn set_play_input_profile(
		&mut self,
		profile: InputProfile,
		input: &mut InputState,
	) -> bool {
		let defaults = InputProfile::default();
		let changed = self.play_input_profile().unwrap_or(&defaults) != &profile;
		self.play_input_override = Some(profile);
		if !changed {
			return false;
		}

		if let Some(session) = self.session.as_mut() {
			session.clear_play_input();
		}

		input.suppress_held();

		true
	}

	pub fn set_entity_play_input(
		&mut self,
		entity: &EntityKey,
		movement: [f32; 3],
	) -> Result<(), String> {
		self.execute_core(Command::SetEntityInput {
			entity: entity.as_str().to_owned(),
			movement,
		})?;
		Ok(())
	}

	pub fn play_movement_bindings(&self) -> &MovementBindings {
		self.play_input_profile()
			.map(|profile| &profile.movement)
			.unwrap_or(&MovementBindings::WASD)
	}

	pub fn play_input_binds_key(&self, key: Key) -> bool {
		self.play_input_profile().map_or_else(
			|| MovementBindings::WASD.contains_key(key),
			|profile| profile.contains_key(key),
		)
	}

	pub(super) fn play_input_profile(&self) -> Option<&InputProfile> {
		self.play_input_override
			.as_ref()
			.or_else(|| self.project()?.input_profile.as_ref())
	}

	pub fn play_input_target(&self) -> &MovementTarget {
		&self.play_input_target
	}

	pub fn set_play_input_target(
		&mut self,
		target: MovementTarget,
		input: &mut InputState,
	) -> Result<bool, String> {
		let session = self
			.session
			.as_mut()
			.ok_or("Native authoring session is not initialized")?;

		let targets = session
			.play_controlled_targets()
			.ok_or("Play mode is not active")?;

		if let MovementTarget::Entity(entity) = &target
			&& !targets.contains(entity)
		{
			return Err("Input target has no configured controlled movement behavior".into());
		}

		if self.play_input_target == target {
			return Ok(false);
		}

		session.clear_play_input();
		input.suppress_held();
		self.play_input_target = target;

		Ok(true)
	}

	pub fn play_movement_held(&self, input: &InputState) -> bool {
		self.is_playing()
			&& !self.simulation_paused()
			&& self
				.play_controlled_targets()
				.is_some_and(|targets| !targets.is_empty())
			&& self.play_movement_bindings().any_held(input)
	}
}
