use std::f32::consts::TAU;

#[derive(Debug, Clone)]
pub struct Camera {
	pub position: [f32; 3],
	pub yaw: f32,
	pub pitch: f32,
	pub vertical_fov: f32,
	pub near: f32,
	pub far: f32,
}

impl Default for Camera {
	fn default() -> Self {
		Self {
			position: [0.0, 0.0, 3.0],
			yaw: 0.0,
			pitch: 0.0,
			vertical_fov: 60.0_f32.to_radians(),
			near: 0.1,
			far: 100.0,
		}
	}
}

impl Camera {
	/// Right-handed projection with Vulkan's 0..1 clip depth and screen Y
	/// convention, shared by CPU picking and renderer adapters.
	pub fn view_projection(&self, aspect: f32) -> Result<glam::Mat4, String> {
		if !aspect.is_finite()
			|| aspect <= 0.0
			|| !self
				.position
				.iter()
				.chain([self.yaw, self.pitch, self.vertical_fov, self.near, self.far].iter())
				.all(|value| value.is_finite())
			|| self.vertical_fov <= 0.0
			|| self.vertical_fov >= std::f32::consts::PI
			|| self.near <= 0.0
			|| self.far <= self.near
		{
			return Err("Camera projection settings must be finite and ordered".into());
		}
		let eye = glam::Vec3::from_array(self.position);
		let target = glam::Vec3::from_array(self.target());
		let direction = (target - eye)
			.try_normalize()
			.ok_or("Camera position cannot represent its viewing direction with f32 precision")?;
		if direction.cross(glam::Vec3::Y).try_normalize().is_none() {
			return Err("Camera viewing direction cannot be parallel to its up axis".into());
		}
		let view = glam::camera::rh::view::look_at_mat4(eye, target, glam::Vec3::Y);
		let projection = glam::camera::rh::proj::vulkan::perspective(
			self.vertical_fov,
			aspect,
			self.near,
			self.far,
		);
		let matrix = projection * view;
		if !matrix.is_finite() || matrix.determinant() == 0.0 || !matrix.inverse().is_finite() {
			return Err("Camera matrix cannot be inverted".into());
		}
		Ok(matrix)
	}
	pub fn forward(&self) -> [f32; 3] {
		let cos_pitch = self.pitch.cos();

		[
			self.yaw.sin() * cos_pitch,
			self.pitch.sin(),
			-self.yaw.cos() * cos_pitch,
		]
	}

	pub fn target(&self) -> [f32; 3] {
		let fw = self.forward();

		[
			self.position[0] + fw[0],
			self.position[1] + fw[1],
			self.position[2] + fw[2],
		]
	}

	pub fn right(&self) -> [f32; 3] {
		[self.yaw.cos(), 0.0, self.yaw.sin()]
	}

	pub fn rotate(&mut self, yaw_delta: f32, pitch_delta: f32) {
		self.yaw += yaw_delta;
		self.yaw = self.yaw.rem_euclid(TAU);

		let pitch_limit = 89.0_f32.to_radians();
		self.pitch = (self.pitch + pitch_delta).clamp(-pitch_limit, pitch_limit);
	}
}
