#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    pub position: [f32; 3],
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

impl Transform {
    pub fn matrix(&self) -> glam::Mat4 {
        // Use exactly the same Euler order and matrix construction as drawing.
        let constants =
            renderer::vulkan::ModelPushConstants::from_3d(self.position, self.rotation, self.scale);
        glam::Mat4::from_cols_array(&constants.model)
    }

    pub fn validate(&self) -> Result<(), String> {
        for (name, values) in [
            ("position", &self.position),
            ("rotation", &self.rotation),
            ("scale", &self.scale),
        ] {
            if !values.iter().all(|value| value.is_finite()) {
                return Err(format!("Transform {name} must contain only finite values"));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
#[path = "tests_transform.rs"]
mod tests;
