use super::camera::validate_pixel;
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handle {
	Axis(usize),
	Plane(usize, usize),
	Screen,
	Rotate(usize),
	UniformScale,
}

#[derive(Debug, Clone, Copy)]
pub struct GizmoSegment {
	pub a: Vec3,
	pub b: Vec3,
	pub axis: usize,
	pub handle: Handle,
}

#[derive(Debug, Clone, Copy)]
pub struct Gizmo {
	pub pivot: Vec3,
	pub axes: [Vec3; 3],
	pub size: f32,
}
impl Gizmo {
	pub fn new(pivot: Vec3, basis: Quat, world_size: f32) -> Self {
		let basis = if basis.is_finite() && basis.length_squared() > EPSILON {
			basis.normalize()
		} else {
			Quat::IDENTITY
		};
		let size = if world_size.is_finite() {
			world_size.clamp(0.0001, MAX_DISTANCE)
		} else {
			1.0
		};
		Self {
			pivot,
			axes: [basis * Vec3::X, basis * Vec3::Y, basis * Vec3::Z],
			size,
		}
	}

	pub fn segments(&self, tool: ViewportTool) -> Vec<GizmoSegment> {
		let mut segments = Vec::new();
		if tool == ViewportTool::Hand {
			return segments;
		}
		for axis in 0..3 {
			if tool == ViewportTool::Rotate {
				let (a, b) = (self.axes[(axis + 1) % 3], self.axes[(axis + 2) % 3]);
				for step in 0..64 {
					let angle = std::f32::consts::TAU * step as f32 / 64.0;
					let next = std::f32::consts::TAU * (step + 1) as f32 / 64.0;
					segments.push(GizmoSegment {
						a: self.pivot + self.size * (a * angle.cos() + b * angle.sin()),
						b: self.pivot + self.size * (a * next.cos() + b * next.sin()),
						axis,
						handle: Handle::Rotate(axis),
					});
				}
			} else {
				let end = self.pivot + self.axes[axis] * self.size;
				segments.push(GizmoSegment {
					a: self.pivot + self.axes[axis] * self.size * 0.15,
					b: end,
					axis,
					handle: Handle::Axis(axis),
				});
				let a = self.axes[(axis + 1) % 3] * self.size * 0.045;
				let b = self.axes[(axis + 2) % 3] * self.size * 0.045;
				if tool == ViewportTool::Scale {
					for (start, finish) in [
						(end - a - b, end + a - b),
						(end + a - b, end + a + b),
						(end + a + b, end - a + b),
						(end - a + b, end - a - b),
					] {
						segments.push(GizmoSegment {
							a: start,
							b: finish,
							axis,
							handle: Handle::Axis(axis),
						});
					}
				} else {
					let base = end - self.axes[axis] * self.size * 0.14;
					for side in [a, -a, b, -b] {
						segments.push(GizmoSegment {
							a: end,
							b: base + side,
							axis,
							handle: Handle::Axis(axis),
						});
					}
				}
			}
		}
		if tool == ViewportTool::Move {
			for (a, b) in [(0, 1), (1, 2), (2, 0)] {
				let corners = self.plane_corners(a, b);
				for index in 0..4 {
					segments.push(GizmoSegment {
						a: corners[index],
						b: corners[(index + 1) % 4],
						axis: a,
						handle: Handle::Plane(a, b),
					});
				}
			}
		}
		segments
	}

	fn plane_corners(&self, a: usize, b: usize) -> [Vec3; 4] {
		[(0.23, 0.23), (0.43, 0.23), (0.43, 0.43), (0.23, 0.43)]
			.map(|(x, y)| self.pivot + self.size * (self.axes[a] * x + self.axes[b] * y))
	}

	pub fn hit_test(
		&self,
		camera: &EditorCamera,
		extent: [u32; 2],
		projection: ViewportProjection,
		tool: ViewportTool,
		pixel: [f64; 2],
		physical_radius: f32,
	) -> Option<Handle> {
		if tool == ViewportTool::Hand || validate_pixel(pixel, extent).is_err() {
			return None;
		}
		let matrix = camera.view_projection(extent, projection).ok()?;
		let cursor = Vec2::new(pixel[0] as f32, pixel[1] as f32);
		let center = project(matrix, extent, self.pivot)?;
		let radius = physical_radius.clamp(2.0, 64.0);
		if center.distance(cursor) < radius * 0.75 && tool != ViewportTool::Rotate {
			return Some(if tool == ViewportTool::Scale {
				Handle::UniformScale
			} else {
				Handle::Screen
			});
		}
		if tool == ViewportTool::Move {
			for (a, b) in [(0, 1), (1, 2), (2, 0)] {
				let projected = self
					.plane_corners(a, b)
					.map(|point| project(matrix, extent, point));
				if let [Some(p0), Some(p1), Some(p2), Some(p3)] = projected {
					if point_in_quad(cursor, [p0, p1, p2, p3]) {
						return Some(Handle::Plane(a, b));
					}
				}
			}
		}
		let mut best = radius;
		let mut hit = None;
		for segment in self.segments(tool) {
			let (Some(a), Some(b)) = (
				project(matrix, extent, segment.a),
				project(matrix, extent, segment.b),
			) else {
				continue;
			};
			let distance = segment_distance(cursor, a, b);
			if distance < best {
				best = distance;
				hit = Some(segment.handle);
			}
		}
		hit
	}
}

fn segment_distance(point: Vec2, a: Vec2, b: Vec2) -> f32 {
	let delta = b - a;
	if delta.length_squared() < 0.01 {
		return point.distance(a);
	}
	point.distance(a + delta * ((point - a).dot(delta) / delta.length_squared()).clamp(0.0, 1.0))
}
fn point_in_quad(point: Vec2, points: [Vec2; 4]) -> bool {
	let mut positive = false;
	let mut negative = false;
	let mut area = 0.0;
	for index in 0..4 {
		let a = points[index];
		let b = points[(index + 1) % 4];
		let cross = (b - a).perp_dot(point - a);
		positive |= cross > 0.01;
		negative |= cross < -0.01;
		area += a.perp_dot(b);
	}
	area.abs() > 1.0 && !(positive && negative)
}

#[derive(Debug, Clone)]
pub struct TransformTarget {
	pub key: String,
	pub world: Mat4,
}

/// Every preview derives from captured matrices, never from a previous preview.
pub struct GizmoDrag {
	camera: EditorCamera,
	extent: [u32; 2],
	start: Vec2,
	gizmo: Gizmo,
	tool: ViewportTool,
	handle: Handle,
	settings: ViewportSettingsDto,
	targets: Vec<TransformTarget>,
	inverse_view_projection: Mat4,
	screen_axis: Option<Vec2>,
	plane_normal: Vec3,
	start_plane: Option<Vec3>,
	last_angle: f32,
	accumulated_angle: f32,
	has_preview: bool,
}
impl GizmoDrag {
	#[allow(clippy::too_many_arguments)]
	pub fn begin(
		camera: &EditorCamera,
		extent: [u32; 2],
		projection: ViewportProjection,
		pixel: [f64; 2],
		gizmo: Gizmo,
		tool: ViewportTool,
		handle: Handle,
		settings: &ViewportSettingsDto,
		targets: Vec<TransformTarget>,
	) -> Result<Self, String> {
		validate_pixel(pixel, extent)?;
		if targets.is_empty() || !gizmo.pivot.is_finite() || !gizmo.size.is_finite() {
			return Err("A handle needs finite selected transforms".into());
		}
		for target in &targets {
			if !target.world.is_finite() {
				return Err("Selected transforms must be finite".into());
			}
			if tool != ViewportTool::Move {
				runtime::scene_data::TransformData::from_matrix(target.world)?;
			}
		}
		let valid = match (tool, handle) {
			(ViewportTool::Move, Handle::Axis(axis))
			| (ViewportTool::Scale, Handle::Axis(axis))
			| (ViewportTool::Rotate, Handle::Rotate(axis)) => axis < 3,
			(ViewportTool::Move, Handle::Plane(a, b)) => a < 3 && b < 3 && a != b,
			(ViewportTool::Move, Handle::Screen) | (ViewportTool::Scale, Handle::UniformScale) => {
				true
			}
			_ => false,
		};
		if !valid {
			return Err("The handle does not match the active tool".into());
		}
		let axis = match handle {
			Handle::Axis(axis) | Handle::Rotate(axis) => Some(axis),
			_ => None,
		};
		let screen_axis = axis.and_then(|axis| {
			let start = camera.project(gizmo.pivot, extent, projection)?;
			let end = camera.project(
				gizmo.pivot + gizmo.axes[axis] * gizmo.size,
				extent,
				projection,
			)?;
			(end.distance_squared(start) > 1.0).then_some(end - start)
		});
		if matches!(handle, Handle::Axis(_)) && screen_axis.is_none() {
			return Err(
				"This axis points into the camera; rotate the view or use a plane handle".into(),
			);
		}
		let plane_normal = match handle {
			Handle::Plane(a, b) => gizmo.axes[a].cross(gizmo.axes[b]).normalize_or_zero(),
			Handle::Rotate(axis) => gizmo.axes[axis],
			_ => camera.forward(),
		};
		let start_plane = camera
			.ray(pixel, extent, projection)?
			.plane(gizmo.pivot, plane_normal);
		if matches!(
			handle,
			Handle::Plane(_, _) | Handle::Screen | Handle::Rotate(_)
		) && start_plane.is_none()
		{
			return Err("The selected handle plane is edge-on to the camera".into());
		}
		if matches!(handle, Handle::Rotate(_))
			&& start_plane.is_some_and(|point| point.distance_squared(gizmo.pivot) < EPSILON)
		{
			return Err("Start the rotation away from the handle center".into());
		}
		Ok(Self {
			camera: camera.clone(),
			extent,
			start: Vec2::new(pixel[0] as f32, pixel[1] as f32),
			gizmo,
			tool,
			handle,
			settings: settings.clone(),
			targets,
			inverse_view_projection: camera.view_projection(extent, projection)?.inverse(),
			screen_axis,
			plane_normal,
			start_plane,
			last_angle: 0.0,
			accumulated_angle: 0.0,
			has_preview: false,
		})
	}

	pub fn tool(&self) -> ViewportTool {
		self.tool
	}

	/// Total world-space displacement from the press point. Apply this to captured
	/// LOCAL positions through each parent's inverse to preserve valid local TRS
	/// under nonuniform or sheared parent-derived world matrices.
	pub fn translation_delta(&self, pixel: [f64; 2]) -> Result<Vec3, String> {
		if self.tool != ViewportTool::Move {
			return Err("Only Move has a translation displacement".into());
		}
		if !pixel
			.iter()
			.all(|value| value.is_finite() && value.abs() < 1.0e8)
		{
			return Err("Handle cursor must be finite and bounded".into());
		}
		let delta = Vec2::new(pixel[0] as f32, pixel[1] as f32) - self.start;
		if delta == Vec2::ZERO {
			return Ok(Vec3::ZERO);
		}
		let mut translation = match self.handle {
			Handle::Axis(axis) => {
				let amount = self.axis_fraction(pixel)? * self.gizmo.size;
				self.gizmo.axes[axis] * snap(amount, self.settings.snap, self.settings.move_snap)
			}
			Handle::Plane(a, b) => {
				let displacement = self.current_plane(pixel)?
					- self.start_plane.ok_or("Plane baseline is missing")?;
				self.gizmo.axes[a]
					* snap(
						displacement.dot(self.gizmo.axes[a]),
						self.settings.snap,
						self.settings.move_snap,
					) + self.gizmo.axes[b]
					* snap(
						displacement.dot(self.gizmo.axes[b]),
						self.settings.snap,
						self.settings.move_snap,
					)
			}
			Handle::Screen => {
				let displacement = self.current_plane(pixel)?
					- self.start_plane.ok_or("Plane baseline is missing")?;
				self.camera.right()
					* snap(
						displacement.dot(self.camera.right()),
						self.settings.snap,
						self.settings.move_snap,
					) + self.camera.up()
					* snap(
						displacement.dot(self.camera.up()),
						self.settings.snap,
						self.settings.move_snap,
					)
			}
			_ => return Err("Invalid Move handle".into()),
		};
		if !translation.is_finite() {
			return Err("Translation exceeds representable values".into());
		}
		if translation.length_squared() < 1.0e-18 {
			translation = Vec3::ZERO;
		}
		Ok(translation)
	}

	/// World TRS preview for ordinary matrices. Move under sheared world matrices
	/// should use `translation_delta` and captured local positions instead.
	/// Pixel positions may leave the aperture while the host owns pointer capture.
	pub fn preview(&mut self, pixel: [f64; 2]) -> Result<Vec<TransformEditDto>, String> {
		if !pixel
			.iter()
			.all(|value| value.is_finite() && value.abs() < 1.0e8)
		{
			return Err("Handle cursor must be finite and bounded".into());
		}
		let cursor = Vec2::new(pixel[0] as f32, pixel[1] as f32);
		let delta = cursor - self.start;
		if delta == Vec2::ZERO {
			if !self.has_preview {
				return Ok(Vec::new());
			}
			return self
				.targets
				.iter()
				.map(|target| world_edit(&target.key, target.world))
				.collect();
		}
		let matrix = match self.tool {
			ViewportTool::Move => Mat4::from_translation(self.translation_delta(pixel)?),
			ViewportTool::Rotate => {
				let current = (self.current_plane(pixel)? - self.gizmo.pivot)
					.try_normalize()
					.ok_or("Rotate away from the handle center")?;
				let start = (self.start_plane.ok_or("Rotation baseline is missing")?
					- self.gizmo.pivot)
					.normalize();
				let angle = self
					.plane_normal
					.dot(start.cross(current))
					.atan2(start.dot(current));
				let difference = (angle - self.last_angle + std::f32::consts::PI)
					.rem_euclid(std::f32::consts::TAU)
					- std::f32::consts::PI;
				self.accumulated_angle += difference;
				self.last_angle = angle;
				let angle = snap(
					self.accumulated_angle,
					self.settings.snap,
					self.settings.rotation_snap_degrees.to_radians(),
				);
				around(
					self.gizmo.pivot,
					Mat4::from_quat(Quat::from_axis_angle(self.plane_normal, angle)),
				)
			}
			ViewportTool::Scale => {
				let mut factors = Vec3::ONE;
				if let Handle::Axis(axis) = self.handle {
					factors[axis] = snap(
						1.0 + self.axis_fraction(pixel)?,
						self.settings.snap,
						self.settings.scale_snap,
					)
					.max(0.001);
				} else {
					let factor = snap(
						1.0 + (delta.x - delta.y) / 120.0,
						self.settings.snap,
						self.settings.scale_snap,
					)
					.max(0.001);
					factors = Vec3::splat(factor);
				}
				let basis = Mat4::from_cols(
					self.gizmo.axes[0].extend(0.0),
					self.gizmo.axes[1].extend(0.0),
					self.gizmo.axes[2].extend(0.0),
					Vec3::ZERO.extend(1.0),
				);
				around(
					self.gizmo.pivot,
					basis * Mat4::from_scale(factors) * basis.inverse(),
				)
			}
			ViewportTool::Hand => return Err("Hand navigation cannot author transforms".into()),
		};
		if !self.has_preview && matrix.abs_diff_eq(Mat4::IDENTITY, 1.0e-7) {
			return Ok(Vec::new());
		}
		self.has_preview = true;
		self.targets
			.iter()
			.map(|target| world_edit(&target.key, matrix * target.world))
			.collect()
	}

	fn axis_fraction(&self, pixel: [f64; 2]) -> Result<f32, String> {
		let screen = self.screen_axis.ok_or("Axis has no screen projection")?;
		let cursor = Vec2::new(pixel[0] as f32, pixel[1] as f32);
		let projected_pixels = (cursor - self.start).dot(screen) / screen.length();
		// Subtracting two absolute f32 screen coordinates introduces ULP-sized
		// noise. Suppress only that numerical uncertainty, not a perceptible
		// movement dead zone or a world-space/scale threshold.
		let pixel_uncertainty = f32::EPSILON
			* cursor
				.abs()
				.max_element()
				.max(self.start.abs().max_element())
				.max(1.0)
			* 4.0;
		Ok(if projected_pixels.abs() <= pixel_uncertainty {
			0.0
		} else {
			projected_pixels / screen.length()
		})
	}

	fn current_plane(&self, pixel: [f64; 2]) -> Result<Vec3, String> {
		// Captured drags may leave the viewport; unprojection remains well-defined.
		let inverse = self.inverse_view_projection;
		let xy = Vec2::new(
			pixel[0] as f32 / self.extent[0] as f32 * 2.0 - 1.0,
			pixel[1] as f32 / self.extent[1] as f32 * 2.0 - 1.0,
		);
		let near = inverse * xy.extend(0.0).extend(1.0);
		let far = inverse * xy.extend(0.999).extend(1.0);
		if near.w.abs() < EPSILON || far.w.abs() < EPSILON {
			return Err("Handle ray cannot be unprojected".into());
		}
		let origin = near.truncate() / near.w;
		let direction = (far.truncate() / far.w - origin)
			.try_normalize()
			.ok_or("Handle ray is degenerate")?;
		Ray {
			origin,
			direction,
			max_distance: MAX_DISTANCE,
		}
		.plane(self.gizmo.pivot, self.plane_normal)
		.ok_or_else(|| "Handle plane is parallel or behind the cursor".into())
	}
}
fn snap(value: f32, enabled: bool, increment: f32) -> f32 {
	if enabled && increment.is_finite() && increment > 0.0 {
		(value / increment).round() * increment
	} else {
		value
	}
}
fn around(pivot: Vec3, matrix: Mat4) -> Mat4 {
	Mat4::from_translation(pivot) * matrix * Mat4::from_translation(-pivot)
}
fn world_edit(key: &str, matrix: Mat4) -> Result<TransformEditDto, String> {
	let value = runtime::scene_data::TransformData::from_matrix(matrix)?;
	Ok(TransformEditDto {
		key: key.into(),
		transform: TransformDto {
			position: value.position,
			rotation: value.rotation,
			scale: value.scale,
		},
	})
}
