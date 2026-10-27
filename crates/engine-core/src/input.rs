mod button;
mod key;
mod mouse;

pub use button::InputButton;
pub use key::{Key, NativeKeyCode};
pub use mouse::MouseButton;

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const INPUT_PROFILE_SCHEMA_VERSION: u32 = 1;
pub const MAX_INPUT_PROFILE_BYTES: usize = 4 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputProfile {
	pub schema_version: u32,
	pub movement: MovementBindings,
}

impl Default for InputProfile {
	fn default() -> Self {
		Self {
			schema_version: INPUT_PROFILE_SCHEMA_VERSION,
			movement: MovementBindings::default(),
		}
	}
}

#[derive(Default)]
pub struct InputState {
	held: HashSet<InputButton>,
	pressed: HashSet<InputButton>,
	released: HashSet<InputButton>,
	suppressed: HashSet<InputButton>,
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
		let mut direction = [0.0_f32; 2];

		if input.is_held(self.positive_x) {
			direction[0] += 1.0;
		}
		if input.is_held(self.negative_x) {
			direction[0] -= 1.0;
		}
		if input.is_held(self.positive_y) {
			direction[1] += 1.0;
		}
		if input.is_held(self.negative_y) {
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

impl InputState {
	pub fn press(&mut self, button: impl Into<InputButton>) {
		let key = button.into();
		if self.suppressed.contains(&key) {
			return;
		}
		if self.held.insert(key) {
			self.pressed.insert(key);
		}
	}

	pub fn release(&mut self, button: impl Into<InputButton>) {
		let key = button.into();
		self.suppressed.remove(&key);
		if self.held.remove(&key) {
			self.released.insert(key);
		}
	}

	pub fn is_held(&self, button: impl Into<InputButton>) -> bool {
		self.held.contains(&button.into())
	}

	pub fn just_pressed(&self, button: impl Into<InputButton>) -> bool {
		self.pressed.contains(&button.into())
	}

	pub fn just_released(&self, button: impl Into<InputButton>) -> bool {
		self.released.contains(&button.into())
	}

	pub fn end_frame(&mut self) {
		self.pressed.clear();
		self.released.clear();
	}
	/// Clear motion/edges at a mode boundary without rearming held shortcuts.
	pub fn suppress_held(&mut self) {
		self.suppressed.extend(self.held.drain());
		self.pressed.clear();
		self.released.clear();
	}
	pub fn reset(&mut self) {
		self.held.clear();
		self.pressed.clear();
		self.released.clear();
		self.suppressed.clear();
	}
}

impl InputProfile {
	pub fn validate(&self) -> Result<(), String> {
		if self.schema_version != INPUT_PROFILE_SCHEMA_VERSION {
			return Err(format!(
				"Unsupported input profile schema version: {}",
				self.schema_version
			));
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
