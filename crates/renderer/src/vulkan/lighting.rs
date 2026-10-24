use glam::Vec3;

/// A renderer light, independent of scene serialization or editor controls.
/// Directional directions point in the direction the light rays travel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Light {
	Directional {
		direction: [f32; 3],
		color: [f32; 3],
		intensity: f32,
	},
	Point {
		position: [f32; 3],
		color: [f32; 3],
		intensity: f32,
		range: f32,
	},
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
struct GpuLight {
	position_kind: [f32; 4],
	direction_range: [f32; 4],
	color_intensity: [f32; 4],
}

/// std140 layout shared with the fragment shader. Construction validates all
/// public input before any renderer state is changed.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct LightingUniform {
	ambient_count: [f32; 4],
	lights: [GpuLight; Self::MAX_LIGHTS],
}

impl LightingUniform {
	pub const MAX_LIGHTS: usize = 16;

	pub fn from_lights(lights: &[Light]) -> Result<Self, String> {
		if lights.len() > Self::MAX_LIGHTS {
			return Err(format!("at most {} lights are supported", Self::MAX_LIGHTS));
		}
		let mut uniform = Self {
			ambient_count: [0.25, 0.25, 0.25, lights.len() as f32],
			lights: [GpuLight::default(); Self::MAX_LIGHTS],
		};
		for (index, light) in lights.iter().enumerate() {
			let (color, intensity) = match *light {
				Light::Directional {
					color, intensity, ..
				}
				| Light::Point {
					color, intensity, ..
				} => (color, intensity),
			};
			if color.iter().any(|v| !v.is_finite() || *v < 0.0)
				|| !intensity.is_finite()
				|| intensity < 0.0
				|| color.iter().any(|v| !(*v * intensity).is_finite())
			{
				return Err(format!(
					"light {index} requires finite nonnegative color and intensity"
				));
			}
			let gpu = match *light {
				Light::Directional { direction, .. } => {
					let direction = Vec3::from_array(direction);
					let Some(direction) = direction.try_normalize() else {
						return Err(format!("light {index} requires a finite nonzero direction"));
					};
					let direction = direction.to_array();
					GpuLight {
						position_kind: [0.0; 4],
						direction_range: [direction[0], direction[1], direction[2], 0.0],
						color_intensity: [color[0], color[1], color[2], intensity],
					}
				}
				Light::Point {
					position, range, ..
				} => {
					if position.iter().any(|v| !v.is_finite()) || !range.is_finite() || range <= 0.0
					{
						return Err(format!(
							"light {index} requires a finite position and positive range"
						));
					}
					GpuLight {
						position_kind: [position[0], position[1], position[2], 1.0],
						direction_range: [0.0, 0.0, 0.0, range],
						color_intensity: [color[0], color[1], color[2], intensity],
					}
				}
			};
			uniform.lights[index] = gpu;
		}
		Ok(uniform)
	}

	pub fn light_count(&self) -> usize {
		self.ambient_count[3] as usize
	}
}

impl Default for LightingUniform {
	fn default() -> Self {
		Self::from_lights(&[Light::Directional {
			direction: [0.4, -0.8, -0.6],
			color: [1.0; 3],
			intensity: 0.75,
		}])
			.expect("constant default lighting is valid")
	}
}
