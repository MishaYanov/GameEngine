use super::SimulationSession;
use crate::{
	EntityKey,
	camera_input::CameraIntent,
	input::{GameKeyboardInput, HeldGameActions, MAX_GAME_INPUT_ACTIONS},
	runtime_lifecycle::RuntimeEntityHandle,
	simulation::FixedStepper,
};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum MovementTarget {
	#[default]
	AllControlled,
	Entity(EntityKey),
}

#[derive(Clone, Default)]
pub struct InputFrame {
	pub(super) movement: HashMap<EntityKey, [f32; 3]>,
	pub(super) game_actions: HeldGameActions,
	pub(super) action_presses: HashSet<String>,
	pub(super) controller_owned: bool,
	pub(super) controller_movement: [f32; 2],
	pub(super) controller_look: [f32; 2],
	pub(super) control_sample: super::control_graph::ControlSample,
	pub(super) local_player: Option<RuntimeEntityHandle>,
	pub(super) possession_managed: bool,
	pub(super) awaiting_neutral: bool,
}
impl InputFrame {
	/// The actor currently receiving this local player's actions.
	pub fn local_player(&self) -> Option<&RuntimeEntityHandle> {
		self.local_player.as_ref()
	}

	pub(super) fn reset_local_controls(
		&mut self,
		owner: Option<RuntimeEntityHandle>,
		possession_managed: bool,
		awaiting_neutral: bool,
		control_graph: bool,
	) {
		*self = Self {
			local_player: owner,
			possession_managed,
			awaiting_neutral,
			..Self::default()
		};
		self.control_sample.clear(control_graph);
	}

	pub(super) fn allow_local_sample(&mut self, is_neutral: bool) -> bool {
		if self.awaiting_neutral && !is_neutral {
			return false;
		}
		self.awaiting_neutral = false;
		true
	}

	pub fn movement(&self, entity: &EntityKey) -> [f32; 3] {
		self.movement.get(entity).copied().unwrap_or([0.0; 3])
	}
	pub fn is_idle(&self) -> bool {
		self.movement.is_empty()
			&& self.game_actions.ids.is_empty()
			&& self.action_presses.is_empty()
			&& self.controller_movement == [0.0; 2]
			&& self.controller_look == [0.0; 2]
	}
	pub fn action_held(&self, id: &str) -> bool {
		self.game_actions.ids.contains(id)
	}
	/// A keyboard transition pending for this fixed step, even if already released.
	pub fn action_pressed(&self, id: &str) -> bool {
		self.action_presses.contains(id)
	}
	/// Multiple held actions may feed the same parameter. Any held source sets it.
	pub fn animation_bool(&self, parameter: &str) -> bool {
		self.game_actions.animation_parameters.contains(parameter)
	}
}

impl SimulationSession {
	pub fn clear_input(&mut self) {
		let awaiting_neutral = self.inputs.awaiting_neutral
			|| self.possession.managed
				&& (!self.inputs.is_idle()
					|| self
						.conditional_input
						.as_ref()
						.is_some_and(super::conditional_input::ConditionalInputs::has_held_sample));
		if let Some(input) = &mut self.conditional_input {
			input.retarget(self.possession.owner.clone());
		}
		self.characters.clear_input();
		self.camera_input.clear();
		self.possession.reset_context();
		self.inputs.reset_local_controls(
			self.possession.owner.clone(),
			self.possession.managed,
			awaiting_neutral,
			self.control_graph.is_some(),
		);
		self.control_graph_step = None;
		self.clock.reset();
	}
	pub fn set_paused(&mut self, paused: bool) {
		if self.paused != paused {
			self.paused = paused;
			self.clear_input();
		}
	}
	pub fn set_input(&mut self, entity: &EntityKey, movement: [f32; 3]) -> Result<bool, String> {
		let movement = normalized_input(movement)?;
		if self.characters.contains(entity) {
			self.characters
				.set_movement(entity, [movement[0], movement[2]])?;
			return Ok(true);
		}
		if !self.controlled_target_set.contains(entity) {
			return Err("Input target has no configured controlled movement behavior".into());
		}

		Ok(Self::apply_input(
			&mut self.inputs,
			&mut self.clock,
			entity,
			movement,
		))
	}

	pub fn set_targeted_input(
		&mut self,
		target: &MovementTarget,
		movement: [f32; 3],
	) -> Result<bool, String> {
		match target {
			MovementTarget::AllControlled => self.set_controlled_input(movement),
			MovementTarget::Entity(entity) => self.set_input(entity, movement),
		}
	}

	pub(crate) fn set_game_keyboard_input(
		&mut self,
		target: &MovementTarget,
		input: GameKeyboardInput,
	) -> Result<(), String> {
		if self.paused {
			return Ok(());
		}
		if matches!(target, MovementTarget::AllControlled)
			&& (!self
				.inputs
				.allow_local_sample(input.movement == [0.0; 2] && input.actions.ids.is_empty())
				|| self.inputs.possession_managed && self.inputs.local_player().is_none())
		{
			return Ok(());
		}
		let available = MAX_GAME_INPUT_ACTIONS - self.inputs.action_presses.len();
		let pressed: Vec<_> = input
			.actions
			.ids
			.difference(&self.inputs.game_actions.ids)
			.filter(|id| !self.inputs.action_presses.contains(*id))
			.take(available + 1)
			.cloned()
			.collect();
		if pressed.len() > available {
			self.clear_input();
			return Err("Game action press budget exceeded; local input cleared".into());
		}
		let mut released = Vec::new();
		if self.control_graph.is_some() || self.conditional_input.is_some() {
			let releases = &self.inputs.control_sample.releases;
			let available = MAX_GAME_INPUT_ACTIONS.saturating_sub(releases.len());
			released.extend(
				self.inputs
					.game_actions
					.ids
					.difference(&input.actions.ids)
					.filter(|id| !releases.contains(*id))
					.take(available + 1)
					.cloned(),
			);
			if released.len() > available {
				self.clear_input();
				return Err("Game action release budget exceeded; local input cleared".into());
			}
		}
		let [x, y] = input.movement;
		// The legacy controlled mover uses XY; upright characters use XZ.
		// Route each target once so the legacy path cannot overwrite character Z.
		match target {
			MovementTarget::AllControlled if self.inputs.local_player().is_some() => {
				self.inputs.controller_owned = true;
				self.inputs.controller_movement = input.movement;
			}
			MovementTarget::AllControlled => {
				self.characters.sample_keyboard(
					&self.world,
					None,
					input.movement,
					&input.actions,
				)?;
				for entity in &self.controlled_targets {
					Self::apply_input(&mut self.inputs, &mut self.clock, entity, [x, y, 0.0]);
				}
			}
			MovementTarget::Entity(entity) if self.characters.contains(entity) => {
				self.characters.sample_keyboard(
					&self.world,
					Some(entity),
					input.movement,
					&input.actions,
				)?;
			}
			MovementTarget::Entity(entity) => {
				self.set_input(entity, [x, y, 0.0])?;
			}
		}
		self.inputs.action_presses.extend(pressed);
		self.inputs.control_sample.releases.extend(released);
		self.inputs.game_actions = input.actions;
		Ok(())
	}

	fn apply_input(
		inputs: &mut InputFrame,
		clock: &mut FixedStepper,
		entity: &EntityKey,
		movement: [f32; 3],
	) -> bool {
		match inputs.movement.get_mut(entity) {
			Some(old) if *old == movement => return false,
			Some(old) if movement != [0.0; 3] => {
				*old = movement;
				return true;
			}
			Some(_) => {
				inputs.movement.remove(entity);
			}
			None if movement == [0.0; 3] => return false,
			None => {
				inputs.movement.insert(entity.clone(), movement);
			}
		}
		clock.reset();
		true
	}

	pub fn set_controlled_input(&mut self, movement: [f32; 3]) -> Result<bool, String> {
		let movement = normalized_input(movement)?;
		if !self.inputs.allow_local_sample(movement == [0.0; 3]) {
			return Ok(false);
		}
		if let Some(local) = self.inputs.local_player() {
			self.characters
				.set_movement(&local.key, [movement[0], movement[2]])?;
			return Ok(true);
		}
		if self.inputs.possession_managed {
			return Ok(false);
		}
		let character_keys = self
			.characters
			.keys()
			.filter(|key| {
				self.world.entity_index(key).is_some_and(|index| {
					self.world.data().entities[index]
						.player_controller
						.is_none()
				})
			})
			.cloned()
			.collect::<Vec<_>>();
		for key in &character_keys {
			self.characters
				.set_movement(key, [movement[0], movement[2]])?;
		}

		if self.controlled_targets.is_empty() && character_keys.is_empty() && movement != [0.0; 3] {
			return Err(
				"Configure a controlled movement behavior before supplying movement".into(),
			);
		}

		let mut changed = false;
		for entity in &self.controlled_targets {
			changed |= Self::apply_input(&mut self.inputs, &mut self.clock, entity, movement);
		}

		Ok(changed)
	}

	pub fn pending_camera_input(&self) -> CameraIntent {
		self.camera_input.snapshot()
	}

	pub(crate) fn submit_camera_input(&mut self, intent: CameraIntent) -> Result<(), String> {
		if self.paused {
			return Err("Resume Play before submitting camera input".into());
		}
		if !self.orbit_state().is_some_and(|rig| !rig.target_lost) {
			return Err("Camera input requires an orbit camera with a live target".into());
		}
		self.camera_input.submit(intent)
	}
}

fn normalized_input(movement: [f32; 3]) -> Result<[f32; 3], String> {
	if !movement.iter().all(|v| v.is_finite()) {
		return Err("Movement input must be finite".into());
	}
	let length = movement
		.iter()
		.map(|v| (*v as f64).powi(2))
		.sum::<f64>()
		.sqrt()
		.max(1.0);
	Ok(movement.map(|v| (v as f64 / length) as f32))
}
