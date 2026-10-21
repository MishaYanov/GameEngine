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
#[path = "tests_camera.rs"]
mod tests;
