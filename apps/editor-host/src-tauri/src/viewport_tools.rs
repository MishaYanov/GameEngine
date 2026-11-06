//! Window-independent editor navigation and spatial tool math.
//!
//! The host owns capture and transactions. Nothing here mutates a scene, camera
//! document, selection, revision, history, file, or GPU resource.
use bridge::{
	editor::{TransformDto, TransformEditDto},
	viewport::{
		ViewportOrientation, ViewportPlane, ViewportProjection, ViewportSettingsDto, ViewportTool,
	},
};
use glam::{Mat4, Quat, Vec2, Vec3};
use runtime::Bounds3;

const EPSILON: f32 = 1.0e-6;
const MAX_DISTANCE: f32 = 1.0e7;
const MIN_DISTANCE: f32 = 0.001;

#[derive(Debug, Clone)]
pub struct EditorCamera {
	pub pivot: Vec3,
	pub yaw: f32,
	pub pitch: f32,
	pub distance: f32,
	pub ortho_height: f32,
	pub vertical_fov: f32,
	pub near: f32,
	pub far: f32,
}

impl Default for EditorCamera {
	fn default() -> Self {
		Self {
			pivot: Vec3::ZERO,
			yaw: -std::f32::consts::FRAC_PI_4,
			pitch: -0.5,
			distance: 15.0,
			ortho_height: 15.0,
			vertical_fov: 60.0_f32.to_radians(),
			near: 0.01,
			far: 10_000.0,
		}
	}
}

impl EditorCamera {
	/// Preserve the authored pose once, then navigate a separate editor value.
	pub fn from_camera(camera: &runtime::Camera) -> Self {
		if camera.view_projection(1.0).is_err() {
			return Self::default();
		}
		let distance = 10.0;
		Self {
			pivot: Vec3::from_array(camera.position)
				+ Vec3::from_array(camera.forward()) * distance,
			yaw: camera.yaw,
			pitch: camera.pitch.clamp(-pole_limit(), pole_limit()),
			distance,
			ortho_height: 2.0 * distance * (camera.vertical_fov * 0.5).tan(),
			vertical_fov: camera.vertical_fov,
			near: camera.near,
			far: camera.far,
		}
	}

	pub fn forward(&self) -> Vec3 {
		Vec3::new(
			self.yaw.sin() * self.pitch.cos(),
			self.pitch.sin(),
			-self.yaw.cos() * self.pitch.cos(),
		)
	}
	pub fn right(&self) -> Vec3 {
		Vec3::new(self.yaw.cos(), 0.0, self.yaw.sin())
	}
	pub fn up(&self) -> Vec3 {
		self.right().cross(self.forward()).normalize_or_zero()
	}
	pub fn position(&self) -> Vec3 {
		self.pivot - self.forward() * self.distance
	}

	pub fn view_projection(
		&self,
		extent: [u32; 2],
		projection: ViewportProjection,
	) -> Result<Mat4, String> {
		validate_extent(extent)?;
		if !self.pivot.is_finite()
			|| ![
				self.yaw,
				self.pitch,
				self.distance,
				self.ortho_height,
				self.vertical_fov,
				self.near,
				self.far,
			]
			.iter()
			.all(|v| v.is_finite())
			|| self.distance <= 0.0
			|| self.ortho_height <= 0.0
			|| self.vertical_fov <= 0.0
			|| self.vertical_fov >= std::f32::consts::PI
			|| self.near <= 0.0
			|| self.far <= self.near
		{
			return Err("Editor camera values must be finite, positive, and ordered".into());
		}
		let position = self.position();
		if !position.is_finite() || (self.pivot - position).length_squared() < EPSILON * EPSILON {
			return Err("Editor camera position exceeds useful floating-point precision".into());
		}
		let aspect = extent[0] as f32 / extent[1] as f32;
		let near = self.near.min(self.distance * 0.1).max(0.00001);
		let far = self.far.max(self.distance * 20.0).min(1.0e9);
		let view = glam::camera::rh::view::look_at_mat4(position, self.pivot, self.up());
		let projection = match projection {
			ViewportProjection::Perspective => {
				glam::camera::rh::proj::vulkan::perspective(self.vertical_fov, aspect, near, far)
			}
			ViewportProjection::Orthographic => {
				let half_height = self.ortho_height * 0.5;
				let half_width = half_height * aspect;
				glam::camera::rh::proj::vulkan::orthographic(
					-half_width,
					half_width,
					-half_height,
					half_height,
					near,
					far,
				)
			}
		};
		let matrix = projection * view;
		if !matrix.is_finite() || matrix.determinant() == 0.0 || !matrix.inverse().is_finite() {
			return Err("Editor camera matrix is not invertible".into());
		}
		Ok(matrix)
	}

	pub fn project(
		&self,
		point: Vec3,
		extent: [u32; 2],
		projection: ViewportProjection,
	) -> Option<Vec2> {
		project(
			self.view_projection(extent, projection).ok()?,
			extent,
			point,
		)
	}

	pub fn ray(
		&self,
		pixel: [f64; 2],
		extent: [u32; 2],
		projection: ViewportProjection,
	) -> Result<Ray, String> {
		validate_pixel(pixel, extent)?;
		let inverse = self.view_projection(extent, projection)?.inverse();
		let ndc = Vec2::new(
			pixel[0] as f32 / extent[0] as f32 * 2.0 - 1.0,
			pixel[1] as f32 / extent[1] as f32 * 2.0 - 1.0,
		);
		let unproject = |depth| -> Result<Vec3, String> {
			let p = inverse * ndc.extend(depth).extend(1.0);
			if !p.is_finite() || p.w.abs() < EPSILON {
				return Err("Editor ray could not be unprojected".into());
			}
			Ok(p.truncate() / p.w)
		};
		let near = unproject(0.0)?;
		// A near-one depth avoids precision loss at an extremely distant far plane.
		let far = unproject(0.999)?;
		let direction = (far - near)
			.try_normalize()
			.ok_or("Editor ray is degenerate")?;
		Ok(Ray {
			origin: near,
			direction,
			max_distance: self.far.max(self.distance * 20.0).min(MAX_DISTANCE),
		})
	}

	/// Pointer deltas are logical pixels, normalized exactly once by the caller.
	pub fn orbit(&mut self, delta: Vec2) {
		if !delta.is_finite() {
			return;
		}
		self.yaw =
			(self.yaw - delta.x.clamp(-2000.0, 2000.0) * 0.006).rem_euclid(std::f32::consts::TAU);
		self.pitch = (self.pitch + delta.y.clamp(-2000.0, 2000.0) * 0.006)
			.clamp(-pole_limit(), pole_limit());
	}

	/// Pan uses physical pixels and the matching physical extent.
	pub fn pan(&mut self, delta: Vec2, extent: [u32; 2], projection: ViewportProjection) {
		if !delta.is_finite() || extent.contains(&0) {
			return;
		}
		let units = self.units_per_pixel(self.pivot, extent, projection);
		let next = self.pivot + self.right() * (-delta.x * units) + self.up() * (delta.y * units);
		if next.is_finite() {
			self.pivot = next;
		}
	}

	/// The same multiplicative factor serves wheel input and explicit +/- controls.
	pub fn zoom(&mut self, factor: f32) {
		if !factor.is_finite() || factor <= 0.0 {
			return;
		}
		self.distance = (self.distance * factor).clamp(MIN_DISTANCE, MAX_DISTANCE);
		self.ortho_height = (self.ortho_height * factor).clamp(MIN_DISTANCE, MAX_DISTANCE);
	}

	/// Movement components are camera-right, world-up, and camera-forward.
	pub fn fly(&mut self, movement: Vec3, seconds: f32, speed: f32) {
		if !movement.is_finite()
			|| !seconds.is_finite()
			|| !speed.is_finite()
			|| seconds <= 0.0
			|| speed <= 0.0
		{
			return;
		}
		let direction =
			self.right() * movement.x + Vec3::Y * movement.y + self.forward() * movement.z;
		let next = self.pivot
			+ direction.clamp_length_max(1.0) * seconds.min(0.1) * speed.min(MAX_DISTANCE);
		if next.is_finite() {
			self.pivot = next;
		}
	}

	pub fn frame(&mut self, bounds: Bounds3, aspect: f32) {
		let min = Vec3::from_array(bounds.min());
		let max = Vec3::from_array(bounds.max());
		let radius = ((max - min).length() * 0.5).max(0.1);
		self.pivot = (min + max) * 0.5;
		let vertical = (self.vertical_fov * 0.5).tan();
		let horizontal = vertical * aspect.max(0.01);
		self.distance =
			(radius / vertical.min(horizontal) * 1.35).clamp(MIN_DISTANCE, MAX_DISTANCE);
		self.ortho_height =
			(2.0 * radius * 1.35 / aspect.min(1.0).max(0.01)).clamp(MIN_DISTANCE, MAX_DISTANCE);
	}

	pub fn orient(&mut self, orientation: ViewportOrientation) {
		use ViewportOrientation::*;
		match orientation {
			Free => {
				self.yaw = -std::f32::consts::FRAC_PI_4;
				self.pitch = -0.5;
			}
			Top => {
				self.yaw = 0.0;
				self.pitch = -pole_limit();
			}
			Bottom => {
				self.yaw = 0.0;
				self.pitch = pole_limit();
			}
			Front => {
				self.yaw = 0.0;
				self.pitch = 0.0;
			}
			Back => {
				self.yaw = std::f32::consts::PI;
				self.pitch = 0.0;
			}
			Right => {
				self.yaw = -std::f32::consts::FRAC_PI_2;
				self.pitch = 0.0;
			}
			Left => {
				self.yaw = std::f32::consts::FRAC_PI_2;
				self.pitch = 0.0;
			}
		}
	}

	pub fn units_per_pixel(
		&self,
		point: Vec3,
		extent: [u32; 2],
		projection: ViewportProjection,
	) -> f32 {
		let height = match projection {
			ViewportProjection::Orthographic => self.ortho_height,
			ViewportProjection::Perspective => {
				(point - self.position()).dot(self.forward()).max(self.near)
					* 2.0 * (self.vertical_fov * 0.5).tan()
			}
		};
		(height / extent[1].max(1) as f32).max(1.0e-7)
	}

	pub fn gizmo_size(
		&self,
		pivot: Vec3,
		extent: [u32; 2],
		projection: ViewportProjection,
		physical_pixels: f32,
	) -> f32 {
		(self.units_per_pixel(pivot, extent, projection) * physical_pixels.max(1.0))
			.clamp(0.0001, MAX_DISTANCE)
	}
}

fn pole_limit() -> f32 {
	std::f32::consts::FRAC_PI_2 - 0.0001
}
fn validate_extent(extent: [u32; 2]) -> Result<(), String> {
	if extent.iter().any(|v| *v == 0 || *v > 16384) {
		return Err("Viewport extent must be between 1 and 16384 pixels".into());
	}
	Ok(())
}
fn validate_pixel(pixel: [f64; 2], extent: [u32; 2]) -> Result<(), String> {
	validate_extent(extent)?;
	if !pixel.iter().all(|v| v.is_finite())
		|| pixel[0] < 0.0
		|| pixel[1] < 0.0
		|| pixel[0] > extent[0] as f64
		|| pixel[1] > extent[1] as f64
	{
		return Err("Cursor must be finite and inside the viewport".into());
	}
	Ok(())
}
pub fn project(matrix: Mat4, extent: [u32; 2], point: Vec3) -> Option<Vec2> {
	if extent.contains(&0) {
		return None;
	}
	let clip = matrix * point.extend(1.0);
	if !clip.is_finite() || clip.w <= EPSILON || clip.z < 0.0 || clip.z > clip.w {
		return None;
	}
	let ndc = clip.truncate() / clip.w;
	Some(Vec2::new(
		(ndc.x + 1.0) * 0.5 * extent[0] as f32,
		(ndc.y + 1.0) * 0.5 * extent[1] as f32,
	))
}

#[derive(Debug, Clone, Copy)]
pub struct Ray {
	pub origin: Vec3,
	pub direction: Vec3,
	pub max_distance: f32,
}
impl Ray {
	pub fn plane(self, origin: Vec3, normal: Vec3) -> Option<Vec3> {
		if !self.origin.is_finite()
			|| !self.direction.is_finite()
			|| !self.max_distance.is_finite()
			|| self.max_distance < 0.0
			|| !origin.is_finite()
			|| !normal.is_finite()
		{
			return None;
		}
		let denominator = self.direction.dot(normal);
		if !denominator.is_finite() || denominator.abs() < EPSILON {
			return None;
		}
		let distance = (origin - self.origin).dot(normal) / denominator;
		if !distance.is_finite() || distance < 0.0 || distance > self.max_distance {
			return None;
		}
		Some(self.origin + self.direction * distance)
	}
}

#[derive(Debug, Clone)]
pub struct PickTarget {
	pub key: String,
	pub bounds: Bounds3,
}
#[derive(Debug, Clone)]
pub struct PickHit {
	pub key: String,
	pub distance: f32,
	pub position: Vec3,
	pub normal: Vec3,
}
pub fn pick_nearest(ray: Ray, targets: &[PickTarget]) -> Option<PickHit> {
	let mut nearest = ray.max_distance;
	let mut hit = None;
	for target in targets {
		if let Some(distance) = target
			.bounds
			.ray_distance(ray.origin, ray.direction, nearest)
		{
			if hit.is_some() && distance >= nearest {
				continue;
			}
			let position = ray.origin + ray.direction * distance;
			let min = Vec3::from_array(target.bounds.min());
			let max = Vec3::from_array(target.bounds.max());
			let mut normal = -ray.direction;
			let mut closest = f32::INFINITY;
			for axis in 0..3 {
				for (edge, sign) in [(min[axis], -1.0), (max[axis], 1.0)] {
					let separation = (position[axis] - edge).abs();
					if separation < closest {
						closest = separation;
						normal = Vec3::ZERO;
						normal[axis] = sign;
					}
				}
			}
			nearest = distance;
			hit = Some(PickHit {
				key: target.key.clone(),
				distance,
				position,
				normal,
			});
		}
	}
	hit
}

pub fn plane_basis(plane: ViewportPlane) -> (Vec3, Vec3, Vec3) {
	match plane {
		ViewportPlane::Xz => (Vec3::Y, Vec3::X, -Vec3::Z),
		ViewportPlane::Xy => (Vec3::Z, Vec3::X, Vec3::Y),
		ViewportPlane::Yz => (Vec3::X, Vec3::Y, Vec3::Z),
	}
}

/// A bounds surface is a placement approximation, not a triangle-accurate mesh hit.
pub fn placement_point(
	ray: Ray,
	targets: &[PickTarget],
	plane: ViewportPlane,
	snap: Option<f32>,
) -> Option<Vec3> {
	let (mut point, normal) = if let Some(hit) = pick_nearest(ray, targets) {
		(hit.position, hit.normal)
	} else {
		let normal = plane_basis(plane).0;
		(ray.plane(Vec3::ZERO, normal)?, normal)
	};
	if let Some(spacing) = snap.filter(|value| value.is_finite() && *value > 0.0) {
		let locked = normal.abs().max_element();
		for axis in 0..3 {
			if normal[axis].abs() < locked - 0.001 {
				point[axis] = (point[axis] / spacing).round() * spacing;
			}
		}
	}
	point.is_finite().then_some(point)
}

/// Visual LOD is separate from tool snapping and never changes the settings DTO.
pub fn adaptive_grid_spacing(base: f32, units_per_pixel: f32) -> f32 {
	if !base.is_finite() || base <= 0.0 || !units_per_pixel.is_finite() || units_per_pixel <= 0.0 {
		return 1.0;
	}
	let desired = units_per_pixel * 32.0;
	let exponent = ((desired / base).log10().floor()).clamp(-6.0, 6.0);
	let decade = base * 10.0_f32.powf(exponent);
	let multiplier = [1.0, 2.0, 5.0, 10.0]
		.into_iter()
		.find(|factor| decade * *factor >= desired)
		.unwrap_or(10.0);
	(decade * multiplier).clamp(1.0e-6, MAX_DISTANCE)
}

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

#[cfg(test)]
#[path = "viewport_tools_tests.rs"]
mod tests;
