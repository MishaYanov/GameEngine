use super::*;
use glam::Vec4;
#[test]
fn grid_rejects_degenerate_planes_and_unbounded_inputs() {
	let mut visuals = EditorVisuals {
		grid: Some(EditorGrid::default()),
		lines: vec![],
	};
	assert!(visuals.validate().is_ok());
	visuals.grid.as_mut().unwrap().normal = [1.0, 0.0, 0.0];
	assert!(visuals.validate().is_err());
	visuals.grid = Some(EditorGrid {
		opacity: f32::NAN,
		..Default::default()
	});
	assert!(visuals.validate().is_err());
}
#[test]
fn line_quad_has_constant_pixel_width_and_clips_before_near_plane() {
	let line = EditorLine {
		start: [-0.5, 0.0, 0.5],
		end: [0.5, 0.0, 0.5],
		color: [1.0; 4],
		width: 4.0,
		depth_test: false,
	};
	let mut output = vec![];
	append_line(&mut output, &line, Mat4::IDENTITY, [800, 600]);
	assert_eq!(output.len(), 6);
	let width = (output[1].position[1] - output[0].position[1]) * 600.0 / 2.0;
	assert!((width.abs() - 4.0).abs() < 1e-4);
	output.clear();
	append_line(
		&mut output,
		&EditorLine {
			start: [-1.0, 0.0, -1.0],
			..line
		},
		Mat4::IDENTITY,
		[800, 600],
	);
	assert_eq!(output.len(), 6);
	assert!(
		output.iter().all(
			|vertex| vertex.position.iter().all(|v| v.is_finite()) && vertex.position[2] >= 0.0
		)
	);
	output.clear();
	append_line(
		&mut output,
		&EditorLine {
			start: [-1.0, 0.0, -2.0],
			end: [1.0, 0.0, -1.0],
			..line
		},
		Mat4::IDENTITY,
		[800, 600],
	);
	assert!(output.is_empty());
}
#[test]
fn grid_depth_reconstruction_matches_projection_for_both_camera_modes() {
	for projection in [
		glam::camera::rh::proj::vulkan::perspective(1.0, 1.5, 0.1, 500.0),
		glam::camera::rh::proj::vulkan::orthographic(-5.0, 5.0, -4.0, 4.0, 0.1, 500.0),
	] {
		let inverse = projection.inverse();
		let near_h = inverse * Vec4::new(0.2, -0.3, 0.0, 1.0);
		let far_h = inverse * Vec4::new(0.2, -0.3, 1.0, 1.0);
		let near = near_h.truncate() / near_h.w;
		let far = far_h.truncate() / far_h.w;
		for t in [0.0, 0.001, 0.1, 0.5, 1.0] {
			let world = near.lerp(far, t);
			let clip = projection * world.extend(1.0);
			let reconstructed = t * near_h.w / (far_h.w + t * (near_h.w - far_h.w));
			assert!((reconstructed - clip.z / clip.w).abs() < 1e-4);
		}
	}
}

#[test]
fn offscreen_editor_lines_do_not_fill_buffers_or_generate_unbounded_quads() {
	let mut output = Vec::new();
	let line = EditorLine::new([5.0, 0.0, 0.5], [6.0, 0.0, 0.5], [1.0; 4]);
	append_line(&mut output, &line, Mat4::IDENTITY, [800, 600]);
	assert!(output.is_empty());
	append_line(
		&mut output,
		&EditorLine {
			start: [-1e10, 0.0, 0.5],
			end: [1e10, 0.0, 0.5],
			..line
		},
		Mat4::IDENTITY,
		[800, 600],
	);
	assert_eq!(output.len(), 6);
	assert!(output.iter().all(
		|vertex| vertex.position.iter().all(|value| value.is_finite())
			&& vertex.position[0].abs() < 2.0
	));
}
