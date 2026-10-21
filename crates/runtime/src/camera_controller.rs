use crate::{Camera, InputState, Key};

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
        let mut direction = [0.0_f32; 2];

        if input.is_held(Key::ArrowUp) {
            direction[1] += 1.0;
        }
        if input.is_held(Key::ArrowDown) {
            direction[1] -= 1.0;
        }
        if input.is_held(Key::ArrowLeft) {
            direction[0] -= 1.0;
        }
        if input.is_held(Key::ArrowRight) {
            direction[0] += 1.0;
        }

        let length = (direction[0] * direction[0] + direction[1] * direction[1]).sqrt();
        if length > 0.0 {
            direction[0] /= length;
            direction[1] /= length;
        }

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

#[cfg(test)]
#[path = "tests_camera_controller.rs"]
mod tests;
