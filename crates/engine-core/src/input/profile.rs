use super::{GamepadSample, InputState, Key};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const INPUT_PROFILE_SCHEMA_VERSION: u32 = 1;
pub const MAX_INPUT_PROFILE_BYTES: usize = 4 * 1024;
pub const MAX_GAME_INPUT_ACTIONS: usize = 24;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputProfile {
	pub schema_version: u32,
	pub movement: MovementBindings,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub actions: Vec<GameInputBinding>,
}

impl Default for InputProfile {
	fn default() -> Self {
		Self {
			schema_version: INPUT_PROFILE_SCHEMA_VERSION,
			movement: MovementBindings::default(),
			actions: Vec::new(),
		}
	}
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameInputBinding {
	pub id: String,
	pub key: Key,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub animation_parameter: Option<String>,
}

/// A resolved gameplay sample shared by the editor and Player.
pub struct GameKeyboardInput {
	pub(crate) movement: [f32; 2],
	pub(crate) actions: HeldGameActions,
}

#[derive(Clone, Default)]
pub(crate) struct HeldGameActions {
	pub(crate) ids: HashSet<String>,
	pub(crate) animation_parameters: HashSet<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MovementBindings {
	pub negative_x: Key,
	pub positive_x: Key,
	pub negative_y: Key,
	pub positive_y: Key,
}

impl Default for MovementBindings {
	fn default() -> Self {
		Self::WASD
	}
}

impl MovementBindings {
	pub const WASD: Self = Self {
		negative_x: Key::A,
		positive_x: Key::D,
		negative_y: Key::S,
		positive_y: Key::W,
	};

	pub const ARROWS: Self = Self {
		negative_x: Key::ArrowLeft,
		positive_x: Key::ArrowRight,
		negative_y: Key::ArrowDown,
		positive_y: Key::ArrowUp,
	};

	pub fn contains_key(&self, key: Key) -> bool {
		[
			self.negative_x,
			self.positive_x,
			self.negative_y,
			self.positive_y,
		]
		.contains(&key)
	}

	pub fn any_held(&self, input: &InputState) -> bool {
		[
			self.negative_x,
			self.positive_x,
			self.negative_y,
			self.positive_y,
		]
		.into_iter()
		.any(|key| input.is_held(key))
	}

	pub fn direction(&self, input: &InputState) -> [f32; 2] {
		self.direction_from_held(|key| input.is_held(key))
	}

	pub(super) fn direction_from_held(&self, mut is_held: impl FnMut(Key) -> bool) -> [f32; 2] {
		let mut direction = [0.0_f32; 2];

		if is_held(self.positive_x) {
			direction[0] += 1.0;
		}
		if is_held(self.negative_x) {
			direction[0] -= 1.0;
		}
		if is_held(self.positive_y) {
			direction[1] += 1.0;
		}
		if is_held(self.negative_y) {
			direction[1] -= 1.0;
		}

		let length = direction[0].hypot(direction[1]);
		if length > 0.0 {
			direction[0] /= length;
			direction[1] /= length;
		}

		direction
	}
}

impl InputProfile {
	pub fn contains_key(&self, key: Key) -> bool {
		self.movement.contains_key(key) || self.actions.iter().any(|binding| binding.key == key)
	}

	pub fn resolve_keyboard(&self, input: &InputState) -> GameKeyboardInput {
		self.resolve_controls(input, GamepadSample::default())
	}

	/// Resolves movement and logical actions from a single keyboard and controller owner.
	pub fn resolve_controls(
		&self,
		input: &InputState,
		gamepad: GamepadSample,
	) -> GameKeyboardInput {
		self.resolve(|key| input.is_held(key), gamepad)
	}

	pub(crate) fn resolve_keyboard_codes(&self, held_codes: &HashSet<String>) -> GameKeyboardInput {
		self.resolve_controls_from_codes(held_codes, GamepadSample::default())
	}

	pub(crate) fn resolve_controls_from_codes(
		&self,
		held_codes: &HashSet<String>,
		gamepad: GamepadSample,
	) -> GameKeyboardInput {
		self.resolve(|key| held_codes.contains(&key.physical_code()), gamepad)
	}

	fn resolve(
		&self,
		mut is_held: impl FnMut(Key) -> bool,
		gamepad: GamepadSample,
	) -> GameKeyboardInput {
		let keyboard = self.movement.direction_from_held(&mut is_held);
		let movement = if keyboard == [0.0; 2] {
			gamepad.movement()
		} else {
			keyboard
		};
		let mut actions = HeldGameActions::default();
		for binding in &self.actions {
			let controller_held = match binding.id.as_str() {
				"jump" => gamepad.jump,
				"interact" => gamepad.interact,
				_ => false,
			};
			if is_held(binding.key) || controller_held {
				actions.ids.insert(binding.id.clone());
				if let Some(parameter) = &binding.animation_parameter {
					actions.animation_parameters.insert(parameter.clone());
				}
			}
		}
		GameKeyboardInput { movement, actions }
	}

	pub fn validate(&self) -> Result<(), String> {
		if self.schema_version != INPUT_PROFILE_SCHEMA_VERSION {
			return Err(format!(
				"Unsupported input profile schema version: {}",
				self.schema_version
			));
		}
		if self.actions.len() > MAX_GAME_INPUT_ACTIONS {
			return Err(format!(
				"An input profile may contain at most {MAX_GAME_INPUT_ACTIONS} actions"
			));
		}
		let mut ids = HashSet::with_capacity(self.actions.len());
		for binding in &self.actions {
			if !valid_game_action_id(&binding.id) {
				return Err("Action IDs must use 1–32 lowercase letters, digits, or underscores and begin with a letter".into());
			}
			if !ids.insert(&binding.id) {
				return Err(format!("Duplicate game action ID {:?}", binding.id));
			}
			if binding.key == Key::Escape {
				return Err("Escape is reserved for leaving game input".into());
			}
			if let Some(parameter) = &binding.animation_parameter {
				let mut bytes = parameter.bytes();
				if parameter.len() > 32
					|| !bytes
						.next()
						.is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
					|| !bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
				{
					return Err("Animation parameters must use 1–32 ASCII letters, digits, or underscores and begin with a letter or underscore".into());
				}
			}
		}

		Ok(())
	}

	pub fn from_json(json: &str) -> Result<Self, String> {
		if json.len() > MAX_INPUT_PROFILE_BYTES {
			return Err(format!(
				"Input profile exceeds {MAX_INPUT_PROFILE_BYTES} bytes"
			));
		}

		let profile: Self = serde_json::from_str(json)
			.map_err(|error| format!("Invalid input profile JSON: {error}"))?;

		profile.validate()?;
		Ok(profile)
	}

	pub fn to_json(&self) -> Result<String, String> {
		self.validate()?;

		let json = serde_json::to_string_pretty(self)
			.map_err(|error| format!("Cannot serialize input profile: {error}"))?;

		if json.len() > MAX_INPUT_PROFILE_BYTES {
			return Err(format!(
				"Input profile exceeds {MAX_INPUT_PROFILE_BYTES} bytes"
			));
		}

		Ok(json)
	}
}

/// Validates the action ID grammar shared by authored profiles and local overrides.
pub fn valid_game_action_id(value: &str) -> bool {
	let mut bytes = value.bytes();
	value.len() <= 32
		&& bytes.next().is_some_and(|byte| byte.is_ascii_lowercase())
		&& bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}
