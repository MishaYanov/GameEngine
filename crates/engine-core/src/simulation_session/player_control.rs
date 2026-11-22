use super::*;
use crate::{
	InputState, Key,
	components::{InputActionsRef, PlayerMovementSpace},
	input::{GameKeyboardInput, HeldGameActions, InputActionMap, PhysicalGamepadSample},
};

#[derive(Clone)]
pub(super) struct PlayerControllerIntent {
	pub movement: [f32; 2],
	pub jump_held: bool,
	pub jump_pressed: bool,
}

pub type InputActionsResolver =
	Arc<dyn Fn(&SceneData) -> Result<Option<InputActionMap>, String> + Send + Sync>;

pub(super) fn apply_local_player(
	world: &RuntimeWorld,
	characters: &mut crate::character_controller::CharacterRuntime,
	inputs: &InputFrame,
	camera: &crate::Camera,
) -> Result<(), String> {
	if !inputs.controller_owned {
		return Ok(());
	}
	let Some(index) = characters
		.local_input_owner(world)
		.and_then(|key| world.entity_index(key))
	else {
		return Ok(());
	};
	let actor = &world.data().entities[index];
	let controller = actor.player_controller.as_ref().expect("validated Player");
	let motor = actor
		.character_controller
		.as_ref()
		.expect("validated Character");
	let intent = controller_intent(
		inputs,
		&motor.settings.jump_action,
		controller.settings.space,
		camera.yaw,
	);
	characters.set_controller_intent(
		&actor.key,
		intent.movement,
		intent.jump_held,
		intent.jump_pressed,
	)
}

fn controller_intent(
	input: &InputFrame,
	jump_action: &str,
	space: PlayerMovementSpace,
	yaw: f32,
) -> PlayerControllerIntent {
	let [side, forward] = input.controller_movement;
	let movement = match space {
		PlayerMovementSpace::World => [side, -forward],
		PlayerMovementSpace::CameraRelative => {
			let (sine, cosine) = yaw.sin_cos();
			[
				side * cosine + forward * sine,
				side * sine - forward * cosine,
			]
		}
	};
	PlayerControllerIntent {
		movement,
		jump_held: input.action_held(jump_action),
		jump_pressed: input.action_pressed(jump_action),
	}
}

pub(super) fn step_camera_intent(
	mut delta: CameraIntent,
	look_rate: [f32; 2],
	seconds: f32,
) -> CameraIntent {
	let angle = f64::from(seconds) * 2.2;
	let max = std::f64::consts::TAU;
	delta.yaw_delta = (delta.yaw_delta + f64::from(look_rate[0]) * angle).clamp(-max, max);
	delta.pitch_delta = (delta.pitch_delta + f64::from(look_rate[1]) * angle).clamp(-max, max);
	delta
}

impl SimulationSession {
	pub fn set_input_actions_resolver(
		&mut self,
		resolver: &InputActionsResolver,
	) -> Result<(), String> {
		let candidate = resolver(self.world.data())?;
		if candidate.is_none() && self.has_configured_input_asset() {
			return Err("The local Player Controller's Input Actions asset is unavailable".into());
		}
		if let Some(map) = &candidate {
			map.validate()?;
		}
		self.input_action_map = candidate;
		Ok(())
	}

	fn has_configured_input_asset(&self) -> bool {
		let configured = self
			.world
			.data()
			.local_player()
			.and_then(|actor| actor.player_controller.as_ref());
		configured.is_some_and(|component| {
			matches!(component.settings.input, InputActionsRef::Asset { .. })
		})
	}

	pub fn require_input_actions_ready(&self) -> Result<(), String> {
		if self.has_configured_input_asset() && self.input_action_map.is_none() {
			return Err("The local Player Controller's Input Actions asset is unavailable".into());
		}
		Ok(())
	}

	pub fn input_action_map(&self) -> Option<&InputActionMap> {
		self.input_action_map.as_ref()
	}

	pub fn authored_action_binds_key(&self, key: Key) -> Option<bool> {
		self.input_action_map
			.as_ref()
			.map(|map| map.contains_key(key))
	}

	pub fn uses_pointer_look(&self) -> bool {
		self.input_action_map
			.as_ref()
			.is_none_or(InputActionMap::uses_pointer_look)
	}

	pub fn sample_action_keys(
		&mut self,
		keyboard: &InputState,
		physical: PhysicalGamepadSample,
	) -> Result<bool, String> {
		let Some(map) = &self.input_action_map else {
			return Ok(false);
		};
		let frame = map.resolve(keyboard, physical, [0.0; 2]);
		self.sample_input_actions(frame)?;
		Ok(true)
	}

	pub fn sample_action_codes(
		&mut self,
		keyboard: &HashSet<String>,
		physical: PhysicalGamepadSample,
	) -> Result<bool, String> {
		let Some(map) = &self.input_action_map else {
			return Ok(false);
		};
		let frame = map.resolve_codes(keyboard, physical, [0.0; 2]);
		self.sample_input_actions(frame)?;
		Ok(true)
	}

	fn sample_input_actions(
		&mut self,
		frame: crate::input::ResolvedInputActions,
	) -> Result<(), String> {
		if self.paused {
			return Ok(());
		}
		let movement = frame.vectors.get("move").copied().unwrap_or_default();
		let look = frame.look_rates.get("look").copied().unwrap_or_default();
		self.set_game_keyboard_input(
			&MovementTarget::AllControlled,
			GameKeyboardInput {
				movement,
				actions: HeldGameActions {
					ids: frame.buttons.into_iter().collect(),
					animation_parameters: frame.animation_parameters.into_iter().collect(),
				},
			},
		)?;
		self.inputs.controller_look = if self.orbit_state().is_some_and(|rig| !rig.target_lost) {
			look
		} else {
			[0.0; 2]
		};
		Ok(())
	}
}
