use glam::{
	Vec3,
	camera::rh::{proj::vulkan::perspective, view::look_at_mat4},
};

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CameraUniform {
	pub view_projection: [f32; 16],
}

impl CameraUniform {
	pub fn from_view_projection(matrix: glam::Mat4) -> Self {
		Self {
			view_projection: matrix.to_cols_array(),
		}
	}

	pub fn orthographic(
		position: [f32; 3],
		target: [f32; 3],
		up: [f32; 3],
		vertical_height: f32,
		aspect_ratio: f32,
		near: f32,
		far: f32,
	) -> Self {
		let half_height = vertical_height * 0.5;
		let half_width = half_height * aspect_ratio;
		let projection = glam::camera::rh::proj::vulkan::orthographic(
			-half_width,
			half_width,
			-half_height,
			half_height,
			near,
			far,
		);
		let view = look_at_mat4(
			Vec3::from_array(position),
			Vec3::from_array(target),
			Vec3::from_array(up).normalize(),
		);
		Self::from_view_projection(projection * view)
	}

	pub fn perspective(
		position: [f32; 3],
		target: [f32; 3],
		up: [f32; 3],

		vertical_fov_radians: f32,
		aspect_ratio: f32,

		near: f32,
		far: f32,
	) -> Self {
		let position = Vec3::from_array(position);

		let target = Vec3::from_array(target);

		let up = Vec3::from_array(up).normalize();

		let view = look_at_mat4(position, target, up);

		let projection = perspective(vertical_fov_radians, aspect_ratio, near, far);

		let view_projection = projection * view;

		Self {
			view_projection: view_projection.to_cols_array(),
		}
	}
}
