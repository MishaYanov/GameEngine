use super::*;
use crate::vulkan::test_driver::{device, reset, state};
use crate::{EditorGrid, EditorLine};
fn pass(device: &VulkanDevice) -> EditorPass {
	let pipeline = || Pipeline::for_test(device.raw());
	EditorPass {
		grid_pipeline: pipeline(),
		depth_lines: pipeline(),
		overlay_lines: pipeline(),
		color_format: vk::Format::B8G8R8A8_SRGB,
		depth_format: vk::Format::D32_SFLOAT,
		vertices: Vec::new(),
		vertex_buffer: None,
		capacity: 0,
		depth_count: 0,
		grid: None,
	}
}
#[test]
fn repeated_editor_frames_reuse_mapped_storage_and_keep_depth_groups_separate() {
	reset();
	let device = device();
	let mut pass = pass(&device);
	let camera = CameraUniform::from_view_projection(Mat4::IDENTITY);
	let line = EditorLine::new([-0.5, 0.0, 0.5], [0.5, 0.0, 0.5], [1.0; 4]);
	let visuals = EditorVisuals {
		grid: Some(EditorGrid::default()),
		lines: vec![
			line,
			EditorLine {
				depth_test: true,
				..line
			},
		],
	};
	for _ in 0..3 {
		pass.prepare(&device, &visuals, &camera, [800, 600])
			.unwrap();
	}
	assert_eq!(pass.depth_count, 6);
	assert_eq!(pass.vertices.len(), 12);
	assert_eq!(pass.draw_counts(), (3, 5));
	state(|state| {
		assert_eq!(state.calls["create_buffer"], 1);
		assert_eq!(state.calls["allocate_memory"], 1);
		assert_eq!(state.calls["map_memory"], 1);
	});
	let capacity = pass.capacity;
	pass.prepare(&device, &EditorVisuals::default(), &camera, [800, 600])
		.unwrap();
	assert_eq!(pass.draw_counts(), (0, 0));
	assert_eq!(pass.capacity, capacity);
}

#[test]
fn failed_editor_buffer_growth_can_be_retried_without_poisoning_capacity() {
	reset();
	let device = device();
	let mut pass = pass(&device);
	let camera = CameraUniform::from_view_projection(Mat4::IDENTITY);
	let visuals = EditorVisuals {
		grid: None,
		lines: vec![EditorLine::new([-0.5, 0.0, 0.5], [0.5, 0.0, 0.5], [1.0; 4])],
	};
	state(|state| state.fail = Some(("create_buffer", 1)));
	assert!(
		pass.prepare(&device, &visuals, &camera, [800, 600])
			.is_err()
	);
	assert_eq!(pass.capacity, 0);
	assert!(pass.vertex_buffer.is_none());
	state(|state| state.fail = None);
	pass.prepare(&device, &visuals, &camera, [800, 600])
		.unwrap();
	assert_eq!(pass.draw_counts(), (1, 2));
}
