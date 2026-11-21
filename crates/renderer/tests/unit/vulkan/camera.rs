use super::*;
#[test]
fn orthographic_camera_keeps_parallel_size_and_vulkan_depth_range() {
	let camera = CameraUniform::orthographic(
		[0.0, 0.0, 10.0],
		[0.0; 3],
		[0.0, 1.0, 0.0],
		8.0,
		2.0,
		0.1,
		100.0,
	);
	let matrix = glam::Mat4::from_cols_array(&camera.view_projection);
	let near = matrix * glam::Vec4::new(0.0, 0.0, 9.9, 1.0);
	let far = matrix * glam::Vec4::new(0.0, 0.0, -90.0, 1.0);
	assert!((near.z / near.w).abs() < 1e-5);
	assert!((far.z / far.w - 1.0).abs() < 1e-5);
	let front = matrix * glam::Vec4::new(1.0, 1.0, 0.0, 1.0);
	let back = matrix * glam::Vec4::new(1.0, 1.0, -10.0, 1.0);
	assert!((front.x / front.w - back.x / back.w).abs() < 1e-6);
	assert!((front.y / front.w - back.y / back.w).abs() < 1e-6);
	assert!(
		front.y < 0.0,
		"Vulkan projection flips Y for a positive-height viewport"
	);
}
