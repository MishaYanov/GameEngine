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
		// Renderer adapters consume this matrix; authoring and simulation own
		// the XYZ Euler convention independently of any graphics backend.
		glam::Mat4::from_scale_rotation_translation(
			glam::Vec3::from_array(self.scale),
			glam::Quat::from_euler(
				glam::EulerRot::XYZ,
				self.rotation[0],
				self.rotation[1],
				self.rotation[2],
			),
			glam::Vec3::from_array(self.position),
		)
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
