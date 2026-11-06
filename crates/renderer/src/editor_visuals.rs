//! Editor-only presentation data. These primitives never enter the game scene.
use glam::{DVec2, DVec4, Mat4, Vec3};

pub const MAX_EDITOR_LINES: usize = 16_384;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EditorGrid {
	pub origin: [f32; 3],
	pub normal: [f32; 3],
	pub tangent: [f32; 3],
	/// Display spacing; independent from any authoring snap increment.
	pub spacing: f32,
	pub opacity: f32,
	pub fade_distance: f32,
}

impl Default for EditorGrid {
	fn default() -> Self {
		Self {
			origin: [0.0; 3],
			normal: [0.0, 1.0, 0.0],
			tangent: [1.0, 0.0, 0.0],
			spacing: 1.0,
			opacity: 0.4,
			fade_distance: 1000.0,
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EditorLine {
	pub start: [f32; 3],
	pub end: [f32; 3],
	pub color: [f32; 4],
	/// Physical-pixel thickness, independent of camera distance.
	pub width: f32,
	pub depth_test: bool,
}

impl EditorLine {
	pub fn new(start: [f32; 3], end: [f32; 3], color: [f32; 4]) -> Self {
		Self {
			start,
			end,
			color,
			width: 1.5,
			depth_test: false,
		}
	}
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct EditorVisuals {
	pub grid: Option<EditorGrid>,
	pub lines: Vec<EditorLine>,
}

impl EditorVisuals {
	pub fn validate(&self) -> Result<(), String> {
		if self.lines.len() > MAX_EDITOR_LINES {
			return Err(format!("Editor visuals exceed {MAX_EDITOR_LINES} lines"));
		}
		if let Some(grid) = self.grid {
			let normal = Vec3::from_array(grid.normal);
			let tangent = Vec3::from_array(grid.tangent);
			if !Vec3::from_array(grid.origin).is_finite()
				|| !normal.is_finite()
				|| !tangent.is_finite()
				|| !normal.length_squared().is_finite()
				|| !tangent.length_squared().is_finite()
				|| normal.length_squared() < 1e-12
				|| tangent.length_squared() < 1e-12
				|| normal
					.normalize()
					.cross(tangent.normalize())
					.length_squared()
					< 1e-8 || !grid.spacing.is_finite()
				|| !(1e-6..=1e6).contains(&grid.spacing)
				|| !grid.opacity.is_finite()
				|| !(0.0..=1.0).contains(&grid.opacity)
				|| !grid.fade_distance.is_finite()
				|| !(0.01..=1e9).contains(&grid.fade_distance)
			{
				return Err("Editor grid requires a finite plane, positive spacing and bounded opacity/distance".into());
			}
		}
		if self.lines.iter().any(|line| {
			!Vec3::from_array(line.start).is_finite()
				|| !Vec3::from_array(line.end).is_finite()
				|| line
					.color
					.iter()
					.any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
				|| !line.width.is_finite()
				|| !(0.1..=32.0).contains(&line.width)
		}) {
			return Err("Editor lines require finite endpoints, normalized colors and widths from 0.1 to 32 pixels".into());
		}
		Ok(())
	}

	pub(crate) fn is_empty(&self) -> bool {
		self.grid.is_none() && self.lines.is_empty()
	}
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct EditorVertex {
	pub position: [f32; 4],
	pub color: [f32; 4],
}

/// Clip world segments before expanding to quads. This avoids wide-line device
/// features, keeps handles legible at any distance, and bounds near-plane spikes.
pub(crate) fn append_line(
	output: &mut Vec<EditorVertex>,
	line: &EditorLine,
	view_projection: Mat4,
	extent: [u32; 2],
) {
	if extent.contains(&0) || line.color[3] == 0.0 {
		return;
	}
	let mut a = (view_projection * Vec3::from_array(line.start).extend(1.0)).as_dvec4();
	let mut b = (view_projection * Vec3::from_array(line.end).extend(1.0)).as_dvec4();
	if !a.is_finite() || !b.is_finite() {
		return;
	}
	for distance in [|v: DVec4| v.z, |v: DVec4| v.w - v.z, |v: DVec4| v.w - 1e-6] {
		let da = distance(a);
		let db = distance(b);
		if da < 0.0 && db < 0.0 {
			return;
		}
		if da < 0.0 {
			a = a.lerp(b, da / (da - db));
		} else if db < 0.0 {
			b = b.lerp(a, db / (db - da));
		}
	}
	let size = DVec2::new(extent[0] as f64, extent[1] as f64);
	let margin = DVec2::ONE + DVec2::splat(line.width as f64 * 2.0) / size;
	for plane in [
		DVec4::new(1.0, 0.0, 0.0, margin.x),
		DVec4::new(-1.0, 0.0, 0.0, margin.x),
		DVec4::new(0.0, 1.0, 0.0, margin.y),
		DVec4::new(0.0, -1.0, 0.0, margin.y),
	] {
		let da = plane.dot(a);
		let db = plane.dot(b);
		if da < 0.0 && db < 0.0 {
			return;
		}
		if da < 0.0 {
			a = a.lerp(b, da / (da - db));
		} else if db < 0.0 {
			b = b.lerp(a, db / (db - da));
		}
	}
	a.z = a.z.clamp(0.0, a.w);
	b.z = b.z.clamp(0.0, b.w);
	let screen_a = (a.truncate().truncate() / a.w) * size;
	let screen_b = (b.truncate().truncate() / b.w) * size;
	let delta = screen_b - screen_a;
	if !delta.is_finite() || delta.length_squared() < 1e-8 {
		return;
	}
	let normal = DVec2::new(-delta.y, delta.x).normalize() * line.width as f64 / size;
	let offset_a = DVec4::new(normal.x * a.w, normal.y * a.w, 0.0, 0.0);
	let offset_b = DVec4::new(normal.x * b.w, normal.y * b.w, 0.0, 0.0);
	let positions = [
		a - offset_a,
		a + offset_a,
		b + offset_b,
		a - offset_a,
		b + offset_b,
		b - offset_b,
	]
	.map(|position| position.as_vec4());
	if positions.iter().any(|position| !position.is_finite()) {
		return;
	}
	for position in positions {
		output.push(EditorVertex {
			position: position.to_array(),
			color: line.color,
		});
	}
}

#[cfg(test)]
mod tests {
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
		assert!(output.iter().all(
			|vertex| vertex.position.iter().all(|v| v.is_finite()) && vertex.position[2] >= 0.0
		));
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
}
