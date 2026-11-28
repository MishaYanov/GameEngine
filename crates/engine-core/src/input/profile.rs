use super::{GamepadSample, InputState, Key, MouseButton};
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
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub mouse_actions: Vec<MouseActionBinding>,
}

impl Default for InputProfile {
	fn default() -> Self {
		Self {
			schema_version: INPUT_PROFILE_SCHEMA_VERSION,
			movement: MovementBindings::default(),
			actions: Vec::new(),
			mouse_actions: Vec::new(),
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

/// An alternate physical binding for a named gameplay action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MouseActionBinding {
	pub id: String,
	pub button: MouseButton,
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

	pub fn contains_mouse_button(&self, button: MouseButton) -> bool {
		self.mouse_actions
			.iter()
			.any(|binding| binding.button == button)
	}

	pub fn resolve_keyboard(&self, input: &InputState) -> GameKeyboardInput {
		self.resolve_controls(input, GamepadSample::default())
	}

	/// Resolves movement and logical actions from the active keyboard, mouse, and controller input.
	pub fn resolve_controls(
		&self,
		input: &InputState,
		gamepad: GamepadSample,
	) -> GameKeyboardInput {
		self.resolve(
			|key| input.is_held(key),
			|button| input.is_held(button),
			gamepad,
		)
	}

	pub(crate) fn resolve_keyboard_codes(&self, held_codes: &HashSet<String>) -> GameKeyboardInput {
		self.resolve_controls_from_codes(held_codes, GamepadSample::default())
	}

	pub(crate) fn resolve_controls_from_codes(
		&self,
		held_codes: &HashSet<String>,
		gamepad: GamepadSample,
	) -> GameKeyboardInput {
		self.resolve(
			|key| held_codes.contains(&key.physical_code()),
			|button| {
				button
					.input_code()
					.is_some_and(|code| held_codes.contains(code))
			},
			gamepad,
		)
	}

	fn resolve(
		&self,
		mut is_held: impl FnMut(Key) -> bool,
		mouse_held: impl Fn(MouseButton) -> bool,
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
			if is_held(binding.key)
				|| controller_held
				|| self
				.mouse_actions
				.iter()
				.any(|alias| alias.id == binding.id && mouse_held(alias.button))
			{
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

		let mut mouse_ids = HashSet::new();
		let mut mouse_buttons = HashSet::new();
		for binding in &self.mouse_actions {
			if !ids.contains(&binding.id) {
				return Err(format!(
					"Mouse binding refers to unknown game action {:?}",
					binding.id
				));
			}
			if binding.button.input_code().is_none() {
				return Err("Mouse actions require Left, Right, Middle, Back or Forward".into());
			}
			if !mouse_ids.insert(&binding.id) {
				return Err(format!(
					"Duplicate mouse binding for game action {:?}",
					binding.id
				));
			}
			if !mouse_buttons.insert(binding.button) {
				return Err(format!(
					"Mouse button {:?} is already bound",
					binding.button
				));
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

#[cfg(test)]
mod tests {
	use super::*;

	fn mouse_profile() -> InputProfile {
		InputProfile {
			actions: vec![
				GameInputBinding {
					id: "jump".into(),
					key: Key::Space,
					animation_parameter: Some("Jumping".into()),
				},
				GameInputBinding {
					id: "wait".into(),
					key: Key::Q,
					animation_parameter: None,
				},
			],
			mouse_actions: vec![MouseActionBinding {
				id: "jump".into(),
				button: MouseButton::Left,
			}],
			..InputProfile::default()
		}
	}

	#[test]
	fn native_mouse_and_named_codes_resolve_the_same_action_and_parameter() {
		let profile = mouse_profile();
		let mut input = InputState::default();
		input.press(Key::W);
		input.press(MouseButton::Left);
		input.press(MouseButton::Right);
		let codes = HashSet::from(["KeyW".into(), "MouseLeft".into(), "MouseRight".into()]);
		let actual = profile.resolve_controls(&input, GamepadSample::default());
		let named = profile.resolve_controls_from_codes(&codes, GamepadSample::default());

		assert_eq!(actual.movement, [0.0, 1.0]);
		assert_eq!(actual.movement, named.movement);
		assert_eq!(actual.actions.ids, HashSet::from(["jump".into()]));
		assert_eq!(actual.actions.ids, named.actions.ids);
		assert_eq!(
			actual.actions.animation_parameters,
			HashSet::from(["Jumping".into()])
		);
		assert_eq!(
			actual.actions.animation_parameters,
			named.actions.animation_parameters
		);

		input.press(Key::Space);
		input.release(MouseButton::Left);
		assert!(
			profile
				.resolve_keyboard(&input)
				.actions
				.ids
				.contains("jump")
		);
		input.release(Key::Space);
		assert!(profile.resolve_keyboard(&input).actions.ids.is_empty());
	}

	#[test]
	fn mouse_aliases_round_trip_and_reject_invalid_targets_or_collisions() {
		let profile = mouse_profile();
		assert!(profile.contains_mouse_button(MouseButton::Left));
		assert!(!profile.contains_mouse_button(MouseButton::Right));
		assert_eq!(
			InputProfile::from_json(&profile.to_json().unwrap()).unwrap(),
			profile
		);
		assert!(
			!InputProfile::default()
				.to_json()
				.unwrap()
				.contains("mouse_actions")
		);

		let mut invalid = profile.clone();
		invalid.mouse_actions[0].id = "missing".into();
		assert!(
			invalid
				.validate()
				.unwrap_err()
				.contains("unknown game action")
		);

		let mut invalid = profile.clone();
		invalid.mouse_actions[0].button = MouseButton::Other(7);
		assert!(invalid.validate().unwrap_err().contains("Left, Right"));

		let mut invalid = profile.clone();
		invalid.mouse_actions.push(MouseActionBinding {
			id: "jump".into(),
			button: MouseButton::Right,
		});
		assert!(
			invalid
				.validate()
				.unwrap_err()
				.contains("Duplicate mouse binding")
		);

		let mut invalid = profile;
		invalid.mouse_actions.push(MouseActionBinding {
			id: "wait".into(),
			button: MouseButton::Left,
		});
		assert!(invalid.validate().unwrap_err().contains("already bound"));
	}

	#[test]
	fn standard_mouse_codes_are_distinct_from_physical_keyboard_codes() {
		let keys: HashSet<_> = Key::ALL.iter().copied().map(Key::physical_code).collect();
		for button in [
			MouseButton::Left,
			MouseButton::Right,
			MouseButton::Middle,
			MouseButton::Back,
			MouseButton::Forward,
		] {
			assert!(!keys.contains(button.input_code().unwrap()));
		}
		assert_eq!(MouseButton::Other(7).input_code(), None);
	}
}
