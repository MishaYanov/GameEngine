use serde::{Deserialize, Serialize};

/// Portable snapshot of the left stick and the resolved logical actions.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GamepadSample {
	pub left_x: i16,
	pub left_y: i16,
	pub jump: bool,
	pub interact: bool,
}

impl GamepadSample {
	/// Maps the stick to a unit disc with a 20 percent radial dead zone.
	pub fn movement(self) -> [f32; 2] {
		const DEAD_ZONE: f64 = 0.20;
		let x = signed_axis(self.left_x);
		let y = signed_axis(self.left_y);
		let length = x.hypot(y);
		if length <= DEAD_ZONE {
			return [0.0; 2];
		}

		let strength = (length.min(1.0) - DEAD_ZONE) / (1.0 - DEAD_ZONE);
		[
			(x / length * strength) as f32,
			(y / length * strength) as f32,
		]
	}

	/// Reports whether movement and both buttons are at rest.
	pub fn is_neutral(self) -> bool {
		!self.jump && !self.interact && self.movement() == [0.0; 2]
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FaceButton {
	A,
	B,
	X,
	Y,
}

impl FaceButton {
	pub const ALL: [Self; 4] = [Self::A, Self::B, Self::X, Self::Y];
}

/// Physical controls reserved for the standalone Player menu.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MenuButtons {
	pub open: bool,
	pub up: bool,
	pub down: bool,
	pub left: bool,
	pub right: bool,
}

/// Face button order is A, B, X, Y. Rearming checks all buttons and the stick.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PhysicalGamepadSample {
	pub left_x: i16,
	pub left_y: i16,
	pub right_x: i16,
	pub right_y: i16,
	pub face_buttons: [bool; 4],
	pub menu: MenuButtons,
}

impl PhysicalGamepadSample {
	pub fn movement(self) -> [f32; 2] {
		self.stick().movement()
	}

	pub fn right_stick(self) -> [f32; 2] {
		GamepadSample {
			left_x: self.right_x,
			left_y: self.right_y,
			..GamepadSample::default()
		}
		.movement()
	}

	pub fn is_neutral(self) -> bool {
		self.face_buttons == [false; 4]
			&& self.menu == MenuButtons::default()
			&& self.movement() == [0.0; 2]
			&& self.right_stick() == [0.0; 2]
	}

	fn stick(self) -> GamepadSample {
		GamepadSample {
			left_x: self.left_x,
			left_y: self.left_y,
			..GamepadSample::default()
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControllerBindings {
	pub jump: FaceButton,
	pub interact: FaceButton,
}

impl Default for ControllerBindings {
	fn default() -> Self {
		Self {
			jump: FaceButton::A,
			interact: FaceButton::X,
		}
	}
}

impl ControllerBindings {
	pub fn validate(self) -> Result<Self, &'static str> {
		if self.jump == self.interact {
			return Err("Choose different buttons for Jump and Interact");
		}
		Ok(self)
	}

	pub fn resolve(self, input: PhysicalGamepadSample) -> GamepadSample {
		GamepadSample {
			jump: input.face_buttons[self.jump as usize],
			interact: input.face_buttons[self.interact as usize],
			..input.stick()
		}
	}
}

fn signed_axis(raw: i16) -> f64 {
	let extent = if raw < 0 { 32768.0 } else { 32767.0 };
	f64::from(raw) / extent
}

#[cfg(test)]
#[path = "../../tests/unit/gamepad_input.rs"]
mod gamepad_input;
