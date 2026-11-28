use glam::{Mat4, Vec3};

use super::CameraUniform;

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

/// Height and distance fog parameters in world units and linear RGB.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fog {
	pub color_linear: [f32; 3],
	pub density: f32,
	pub base_height: f32,
	pub height_falloff: f32,
	pub start_distance: f32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
struct GpuFog {
	color_density: [f32; 4],
	settings: [f32; 4],
	camera_origin: [f32; 4],
	camera_forward: [f32; 4],
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
	fog: GpuFog,
}

impl LightingUniform {
	pub const MAX_LIGHTS: usize = 16;

	pub fn set_ambient(&mut self, ambient: [f32; 3]) -> Result<(), String> {
		if !ambient.iter().all(|value| (0.0..=1.0).contains(value)) {
			return Err("Ambient color requires finite linear RGB values from 0 to 1".into());
		}
		self.ambient_count[..3].copy_from_slice(&ambient);
		Ok(())
	}

	pub(crate) fn retain_fog(&mut self, existing: &Self) {
		self.fog = existing.fog;
	}

	pub fn set_fog(&mut self, fog: Option<Fog>) -> Result<(), String> {
		let Some(fog) = fog else {
			self.fog = GpuFog::default();
			return Ok(());
		};
		if !fog
			.color_linear
			.iter()
			.all(|value| (0.0..=1.0).contains(value))
			|| !(0.0..=1.0).contains(&fog.density)
			|| !(0.0..=1.0).contains(&fog.height_falloff)
			|| !fog.base_height.is_finite()
			|| !fog.start_distance.is_finite()
			|| fog.start_distance < 0.0
		{
			return Err("Invalid world-space fog parameters".into());
		}
		self.fog.color_density = [
			fog.color_linear[0],
			fog.color_linear[1],
			fog.color_linear[2],
			fog.density,
		];
		self.fog.settings = [fog.base_height, fog.height_falloff, fog.start_distance, 0.0];
		Ok(())
	}

	pub(crate) fn set_fog_camera(&mut self, camera: &CameraUniform) {
		if self.fog.color_density[3] == 0.0 {
			return;
		}
		let matrix = Mat4::from_cols_array(&camera.view_projection);
		if let Some((origin, forward)) = fog_camera(matrix) {
			self.fog.camera_origin = origin.extend(0.0).to_array();
			self.fog.camera_forward = forward.extend(0.0).to_array();
		} else {
			// A draw with a degenerate camera has no meaningful fog ray.
			self.fog.color_density[3] = 0.0;
		}
	}

	pub fn from_lights(lights: &[Light]) -> Result<Self, String> {
		if lights.len() > Self::MAX_LIGHTS {
			return Err(format!("at most {} lights are supported", Self::MAX_LIGHTS));
		}
		let mut uniform = Self {
			ambient_count: [0.25, 0.25, 0.25, lights.len() as f32],
			lights: [GpuLight::default(); Self::MAX_LIGHTS],
			fog: GpuFog::default(),
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

	pub(crate) fn shadow_light(&self) -> Option<(usize, Vec3)> {
		self.lights[..self.light_count()]
			.iter()
			.enumerate()
			.filter(|(_, light)| light.position_kind[3] == 0.0 && light.color_intensity[3] > 0.0)
			.filter_map(|(index, light)| {
				let strength = Vec3::from_slice(&light.color_intensity[..3]).max_element()
					* light.color_intensity[3];
				(strength > 0.0).then_some((index, light, strength))
			})
			.max_by(|(_, _, a), (_, _, b)| a.total_cmp(b))
			.map(|(index, light, _)| (index, Vec3::from_slice(&light.direction_range[..3])))
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

fn fog_camera(matrix: Mat4) -> Option<(Vec3, Vec3)> {
	let determinant = matrix.determinant();
	if !determinant.is_finite() || determinant == 0.0 {
		return None;
	}
	let inverse = matrix.inverse();
	let perspective = matrix.transpose().w_axis.truncate().length_squared() > f32::EPSILON;
	if perspective {
		// Projecting the eye gives X=Y=W=0, with only clip Z remaining.
		let eye = inverse.z_axis;
		let origin = eye.truncate() / eye.w;
		return origin.is_finite().then_some((origin, Vec3::ZERO));
	}
	// Orthographic rays are parallel and start on the near plane.
	let center = inverse.w_axis;
	let origin = center.truncate() / center.w;
	let forward = inverse.z_axis.truncate().try_normalize()?;
	origin.is_finite().then_some((origin, forward))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn fog_camera_distinguishes_perspective_and_parallel_rays() {
		let eye = [42.0, 7.0, 35.0];
		let target = [40.0, 6.0, 12.0];
		let up = [0.0, 1.0, 0.0];
		let expected = Vec3::from_array(eye);
		let toward = (Vec3::from_array(target) - expected).normalize();
		let perspective = CameraUniform::perspective(eye, target, up, 1.0, 1.6, 0.1, 500.0);
		let (origin, forward) =
			fog_camera(Mat4::from_cols_array(&perspective.view_projection)).unwrap();
		assert!(origin.abs_diff_eq(expected, 0.001));
		assert_eq!(forward, Vec3::ZERO);
		let orthographic = CameraUniform::orthographic(eye, target, up, 10.0, 1.6, 0.1, 500.0);
		let (origin, forward) =
			fog_camera(Mat4::from_cols_array(&orthographic.view_projection)).unwrap();
		assert!(origin.abs_diff_eq(expected + toward * 0.1, 0.001));
		assert!(forward.abs_diff_eq(toward, 0.001));
	}
}
