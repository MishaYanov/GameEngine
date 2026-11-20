//! Authored follow settings and the shared runtime camera pose.
use crate::{Camera, EntityKey, world::RuntimeWorld};
use glam::Vec3;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FollowCameraSettings {
	pub target: EntityKey,
	pub offset: [f32; 3],
	pub look_offset: [f32; 3],
	/// Exponential response time in seconds. Zero follows immediately.
	pub smoothing_seconds: f32,
}
impl FollowCameraSettings {
	pub fn validate(&self) -> Result<(), String> {
		self.target.validate()?;
		if !self
			.offset
			.iter()
			.chain(self.look_offset.iter())
			.all(|value| value.is_finite())
			|| !self.smoothing_seconds.is_finite()
			|| !(0.0..=10.0).contains(&self.smoothing_seconds)
		{
			return Err(
				"Follow camera offsets must be finite and smoothing must be within 0..=10 seconds"
					.into(),
			);
		}
		let direction = Vec3::from_array(self.look_offset) - Vec3::from_array(self.offset);
		let direction = direction
			.try_normalize()
			.ok_or("Follow camera eye and look offsets must define a finite direction")?;
		if direction.y.abs() > 89.0_f32.to_radians().sin() {
			return Err("Follow camera direction must stay within ±89 degrees of pitch".into());
		}
		Ok(())
	}
}

#[derive(Clone)]
pub struct GameCameraController {
	camera: Camera,
}
impl GameCameraController {
	pub fn new(world: &RuntimeWorld) -> Result<Self, String> {
		let authored = &world.data().camera;
		let mut controller = Self {
			camera: Camera {
				position: authored.position,
				yaw: authored.yaw,
				pitch: authored.pitch,
				vertical_fov: authored.vertical_fov,
				near: authored.near_plane,
				far: authored.far_plane,
			},
		};
		controller.resolve(world, 0.0, true)?;
		Ok(controller)
	}
	pub fn camera(&self) -> &Camera {
		&self.camera
	}
	pub fn needs_tick(&self, world: &RuntimeWorld) -> bool {
		let Some(follow) = &world.data().camera.follow else {
			return false;
		};
		let Some(index) = world.entity_index(&follow.target) else {
			return false;
		};
		let desired =
			world.world_matrices()[index].w_axis.truncate() + Vec3::from_array(follow.offset);
		(Vec3::from_array(self.camera.position) - desired)
			.abs()
			.max_element()
			> camera_tolerance(desired)
	}
	pub fn update(&mut self, world: &RuntimeWorld, elapsed: f32) -> Result<(), String> {
		if !elapsed.is_finite() || elapsed < 0.0 {
			return Err("Camera elapsed time must be finite and nonnegative".into());
		}
		self.resolve(world, elapsed, false)
	}
	fn resolve(&mut self, world: &RuntimeWorld, elapsed: f32, snap: bool) -> Result<(), String> {
		let Some(follow) = &world.data().camera.follow else {
			return Ok(());
		};
		// Runtime removal can temporarily remove the subject. Retain the last valid pose.
		let Some(index) = world.entity_index(&follow.target) else {
			return Ok(());
		};
		let target = world.world_matrices()[index].w_axis.truncate();
		let desired = target + Vec3::from_array(follow.offset);
		let alpha = if snap || follow.smoothing_seconds == 0.0 {
			1.0
		} else {
			-(-elapsed / follow.smoothing_seconds).exp_m1()
		};
		let previous = Vec3::from_array(self.camera.position);
		let mut position = previous.lerp(desired, alpha);
		if (position - desired).abs().max_element() <= camera_tolerance(desired)
			|| elapsed > 0.0 && position == previous
		{
			position = desired;
		}
		// Translate the eye and look point together while smoothing the rig.
		// Subject reversals cannot move the eye through its own look point.
		let direction = (Vec3::from_array(follow.look_offset) - Vec3::from_array(follow.offset))
			.try_normalize()
			.ok_or("Follow camera cannot resolve a viewing direction")?;
		let mut candidate = self.camera.clone();
		candidate.position = position.to_array();
		candidate.yaw = direction.x.atan2(-direction.z);
		candidate.pitch = direction.y.atan2(direction.x.hypot(direction.z));
		candidate.view_projection(1.0)?;
		self.camera = candidate;
		Ok(())
	}
}

fn camera_tolerance(position: Vec3) -> f32 {
	(4.0 * f32::EPSILON * position.abs().max_element()).max(0.0001)
}
