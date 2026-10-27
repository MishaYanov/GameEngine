use crate::{Camera, InputState, input::MovementBindings};

#[derive(Debug)]
pub struct CameraController {
	pub move_speed: f32,
	pub mouse_sensitivity: f32,
}

impl Default for CameraController {
	fn default() -> Self {
		Self {
			move_speed: 2.0,
			mouse_sensitivity: 0.002,
		}
	}
}

impl CameraController {
	pub fn update(&self, camera: &mut Camera, input: &InputState, delta_seconds: f32) {
		let direction = MovementBindings::ARROWS.direction(input);

		let forward = camera.forward();
		let right = camera.right();
		let distance = self.move_speed * delta_seconds;

		for axis in 0..3 {
			camera.position[axis] +=
				(right[axis] * direction[0] + forward[axis] * direction[1]) * distance;
		}
	}

	pub fn mouse_motion(&self, camera: &mut Camera, dx: f64, dy: f64) {
		camera.rotate(
			dx as f32 * self.mouse_sensitivity,
			-dy as f32 * self.mouse_sensitivity,
		);
	}
}
