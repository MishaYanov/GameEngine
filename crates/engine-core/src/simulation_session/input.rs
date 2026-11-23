use super::SimulationSession;
use crate::{
	EntityKey,
	camera_input::CameraIntent,
	input::{GameKeyboardInput, HeldGameActions, MAX_GAME_INPUT_ACTIONS},
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
}
impl InputFrame {
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
		if let Some(input) = &mut self.conditional_input {
			input.clear();
		}
		self.characters.clear_input();
		self.camera_input.clear();
		self.inputs.movement.clear();
		self.inputs.game_actions = HeldGameActions::default();
		self.inputs.action_presses.clear();
		self.inputs.controller_owned = false;
		self.inputs.controller_movement = [0.0; 2];
		self.inputs.controller_look = [0.0; 2];
		self.inputs
			.control_sample
			.clear(self.control_graph.is_some());
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
			return Err("Game action press budget exceeded".into());
		}
		let [x, y] = input.movement;
		// The legacy controlled mover uses XY; upright characters use XZ.
		// Route each target once so the legacy path cannot overwrite character Z.
		match target {
			MovementTarget::AllControlled
				if self.characters.local_input_owner(&self.world).is_some() =>
			{
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
		if self.control_graph.is_some() {
			let releases = &mut self.inputs.control_sample.releases;
			let available = MAX_GAME_INPUT_ACTIONS.saturating_sub(releases.len());
			let mut released: Vec<_> = self
				.inputs
				.game_actions
				.ids
				.difference(&input.actions.ids)
				.filter(|id| !releases.contains(*id))
				.collect();
			released.sort_unstable();
			releases.extend(released.into_iter().take(available).cloned());
		}
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
		if let Some(local) = self.characters.local_input_owner(&self.world).cloned() {
			self.characters
				.set_movement(&local, [movement[0], movement[2]])?;
			return Ok(true);
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
