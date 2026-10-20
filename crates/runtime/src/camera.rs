use std::f32::consts::TAU;

#[derive(Debug)]
pub struct Camera {
    pub position: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub vertical_fov: f32,
    pub near: f32,
    pub far: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            position: [0.0, 0.0, 3.0],
            yaw: 0.0,
            pitch: 0.0,
            vertical_fov: 60.0_f32.to_radians(),
            near: 0.1,
            far: 100.0,
        }
    }
}

impl Camera {
    pub fn forward(&self) -> [f32; 3] {
        let cos_pitch = self.pitch.cos();

        [
            self.yaw.sin() * cos_pitch,
            self.pitch.sin(),
            -self.yaw.cos() * cos_pitch,
        ]
    }

    pub fn target(&self) -> [f32; 3] {
        let fw = self.forward();

        [
            self.position[0] + fw[0],
            self.position[1] + fw[1],
            self.position[2] + fw[2],
        ]
    }

    pub fn right(&self) -> [f32; 3] {
        [self.yaw.cos(), 0.0, self.yaw.sin()]
    }

    pub fn rotate(&mut self, yaw_delta: f32, pitch_delta: f32) {
        self.yaw += yaw_delta;
        self.yaw = self.yaw.rem_euclid(TAU);

        let pitch_limit = 89.0_f32.to_radians();
        self.pitch = (self.pitch + pitch_delta).clamp(-pitch_limit, pitch_limit);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::{FRAC_PI_2, PI};

    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 1e-5,
            "expected {expected}, got {actual}"
        );
    }

    fn assert_vector(actual: [f32; 3], expected: [f32; 3]) {
        for (actual, expected) in actual.into_iter().zip(expected) {
            assert_close(actual, expected);
        }
    }

    fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
        a.into_iter().zip(b).map(|(a, b)| a * b).sum()
    }

    #[test]
    fn default_camera_faces_negative_z_with_a_valid_projection() {
        let camera = Camera::default();
        assert_vector(camera.position, [0.0, 0.0, 3.0]);
        assert_vector(camera.forward(), [0.0, 0.0, -1.0]);
        assert_vector(camera.right(), [1.0, 0.0, 0.0]);
        assert_vector(camera.target(), [0.0, 0.0, 2.0]);
        assert_close(camera.vertical_fov, PI / 3.0);
        assert!(camera.near > 0.0 && camera.far > camera.near);
    }

    #[test]
    fn yaw_rotates_forward_and_right_through_cardinal_directions() {
        for (yaw, forward, right) in [
            (0.0, [0.0, 0.0, -1.0], [1.0, 0.0, 0.0]),
            (FRAC_PI_2, [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            (PI, [0.0, 0.0, 1.0], [-1.0, 0.0, 0.0]),
            (3.0 * FRAC_PI_2, [-1.0, 0.0, 0.0], [0.0, 0.0, -1.0]),
        ] {
            let camera = Camera {
                yaw,
                ..Camera::default()
            };
            assert_vector(camera.forward(), forward);
            assert_vector(camera.right(), right);
        }
    }

    #[test]
    fn positive_and_negative_pitch_look_up_and_down() {
        for pitch in [PI / 6.0, -PI / 6.0] {
            let camera = Camera {
                pitch,
                ..Camera::default()
            };
            assert_close(camera.forward()[1], if pitch > 0.0 { 0.5 } else { -0.5 });
            assert_close(camera.forward()[2], -3.0_f32.sqrt() / 2.0);
            assert_vector(camera.right(), [1.0, 0.0, 0.0]);
        }
    }

    #[test]
    fn direction_vectors_stay_unit_length_orthogonal_and_right_stays_horizontal() {
        for yaw in [0.0, 0.37, PI, TAU - 0.01] {
            for pitch in [
                -89.0_f32.to_radians(),
                -0.4,
                0.0,
                0.7,
                89.0_f32.to_radians(),
            ] {
                let camera = Camera {
                    yaw,
                    pitch,
                    ..Camera::default()
                };
                let forward = camera.forward();
                let right = camera.right();
                assert_close(dot(forward, forward), 1.0);
                assert_close(dot(right, right), 1.0);
                assert_close(dot(forward, right), 0.0);
                assert_close(right[1], 0.0);
            }
        }
    }

    #[test]
    fn target_moves_with_position_without_changing_the_view_direction() {
        let mut camera = Camera {
            yaw: FRAC_PI_2,
            position: [2.0, -3.0, 4.0],
            ..Camera::default()
        };
        assert_vector(camera.target(), [3.0, -3.0, 4.0]);
        let forward = camera.forward();
        camera.position = [-5.0, 6.0, -7.0];
        assert_vector(camera.target(), [-4.0, 6.0, -7.0]);
        assert_vector(camera.forward(), forward);
    }

    #[test]
    fn rotation_accumulates_deltas_without_changing_position_or_projection() {
        let mut camera = Camera::default();
        let position = camera.position;
        let projection = (camera.vertical_fov, camera.near, camera.far);
        camera.rotate(0.3, 0.2);
        camera.rotate(0.4, -0.1);
        assert_close(camera.yaw, 0.7);
        assert_close(camera.pitch, 0.1);
        assert_vector(camera.position, position);
        assert_eq!((camera.vertical_fov, camera.near, camera.far), projection);
    }

    #[test]
    fn yaw_wraps_positive_negative_and_multiple_full_turns() {
        for (delta, expected) in [
            (TAU, 0.0),
            (-TAU, 0.0),
            (TAU * 3.0 + FRAC_PI_2, FRAC_PI_2),
            (-TAU * 3.0 - FRAC_PI_2, TAU - FRAC_PI_2),
        ] {
            let mut camera = Camera::default();
            camera.rotate(delta, 0.0);
            assert!((0.0..TAU).contains(&camera.yaw));
            assert_close(camera.yaw, expected);
        }
        let mut camera = Camera {
            yaw: TAU - 0.1,
            ..Camera::default()
        };
        camera.rotate(0.2, 0.0);
        assert_close(camera.yaw, 0.1);
    }

    #[test]
    fn pitch_clamps_at_both_limits_and_can_move_back_from_them() {
        let limit = 89.0_f32.to_radians();
        for sign in [-1.0, 1.0] {
            let mut camera = Camera::default();
            camera.rotate(0.0, sign * PI);
            assert_close(camera.pitch, sign * limit);
            camera.rotate(0.0, sign * 0.5);
            assert_close(camera.pitch, sign * limit);
            assert!(
                camera.forward()[2] < 0.0,
                "clamping must avoid the vertical singularity"
            );
            camera.rotate(0.0, -sign * 0.25);
            assert_close(camera.pitch, sign * (limit - 0.25));
        }
    }
}
