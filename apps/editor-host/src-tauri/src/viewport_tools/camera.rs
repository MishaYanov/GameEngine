use super::*;

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
		let angle = delta.clamp(Vec2::splat(-2000.0), Vec2::splat(2000.0)) * 0.006;
		self.orbit_angles(Vec2::new(-angle.x, angle.y));
	}

	pub fn orbit_angles(&mut self, delta: Vec2) {
		self.yaw = (self.yaw + delta.x).rem_euclid(std::f32::consts::TAU);
		self.pitch = (self.pitch + delta.y).clamp(-pole_limit(), pole_limit());
	}

	/// Fly mode turns around the eye instead of swinging around the scene pivot.
	pub fn look(&mut self, delta: Vec2) {
		let position = self.position();
		self.orbit(-delta);
		self.pivot = position + self.forward() * self.distance;
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

fn pole_limit() -> f32 {
	std::f32::consts::FRAC_PI_2 - 0.0001
}
fn validate_extent(extent: [u32; 2]) -> Result<(), String> {
	if extent.iter().any(|v| *v == 0 || *v > 16384) {
		return Err("Viewport extent must be between 1 and 16384 pixels".into());
	}
	Ok(())
}
pub(super) fn validate_pixel(pixel: [f64; 2], extent: [u32; 2]) -> Result<(), String> {
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
