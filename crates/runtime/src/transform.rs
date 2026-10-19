#[derive(Debug)]
pub struct Transform {
    pub position: [f32; 3],
    /// Euler angles in radians, matching ModelPushConstants::from_3d.
    pub rotation: [f32; 3],
    pub scale: [f32; 3],
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            position: [0.0; 3],
            rotation: [0.0; 3],
            scale: [1.0; 3],
        }
    }
}
