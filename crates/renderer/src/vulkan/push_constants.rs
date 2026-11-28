use std::{mem::size_of, slice};

use glam::{EulerRot, Mat4, Quat, Vec3};

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ModelPushConstants {
	pub model: [f32; 16],
	pub base_color_factor: [f32; 4],
	// x bits: 1 unlit, 2 screen-space, 4 vertex color, 8 depth-free overlay,
	// 16 instance vertex input, 32 wireframe edge, 64 vertex marker, 128 wireframe surface,
	// 256 alpha test, 512 material alpha cutoff, 1024 tangent-space normal map.
	// y/z hold skin palette base/count. w holds the cutoff bits or marker viewport extent.
	pub flags: [u32; 4],
	/// Linear emissive RGB and normal map scale in the fourth component.
	pub emissive_factor: [f32; 4],
}

impl ModelPushConstants {
	pub const OFFSET: u32 = 0;

	pub(crate) const ALPHA_TEST: u32 = 256;
	pub(crate) const MATERIAL_ALPHA_CUTOFF: u32 = 512;
	pub(crate) const NORMAL_MAP: u32 = 1024;

	pub const SIZE: u32 = size_of::<Self>() as u32;

	pub const fn identity() -> Self {
		Self {
			model: [
				1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
			],
			base_color_factor: [1.0; 4],
			flags: [0; 4],
			emissive_factor: [0.0; 4],
		}
	}

	pub fn from_3d(translation: [f32; 3], rotation_radians: [f32; 3], scale: [f32; 3]) -> Self {
		let translation = Vec3::from_array(translation);

		let scale = Vec3::from_array(scale);

		let rotation = Quat::from_euler(
			EulerRot::XYZ,
			rotation_radians[0],
			rotation_radians[1],
			rotation_radians[2],
		);

		let model = Mat4::from_scale_rotation_translation(scale, rotation, translation);

		Self {
			model: model.to_cols_array(),
			base_color_factor: [1.0; 4],
			flags: [0; 4],
			emissive_factor: [0.0; 4],
		}
	}

	pub fn as_bytes(&self) -> &[u8] {
		unsafe { slice::from_raw_parts((self as *const Self).cast::<u8>(), size_of::<Self>()) }
	}

	pub fn degrees_to_radians(degrees: f32) -> f32 {
		degrees.to_radians()
	}
}

#[cfg(test)]
#[path = "../../tests/unit/vulkan/push_constants.rs"]
mod tests;
