/// Portable snapshot of the left stick and the supported gamepad buttons.
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

fn signed_axis(raw: i16) -> f64 {
	let extent = if raw < 0 { 32768.0 } else { 32767.0 };
	f64::from(raw) / extent
}

#[cfg(test)]
#[path = "../../tests/unit/gamepad_input.rs"]
mod gamepad_input;
