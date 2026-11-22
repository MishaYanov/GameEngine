//! Authored game camera modes and the fixed-step controller that publishes their runtime pose.
mod follow;
mod object;
mod obstruction;
mod orbit;

use crate::{Camera, EntityKey, scene_data::CameraData, world::RuntimeWorld};

pub use follow::FollowCameraSettings;
pub use object::{CameraComponent, CameraRig, CameraSettings, PerspectiveLens};
pub(crate) use obstruction::obstruction_query;
pub use orbit::{
	CameraTargetSnapshot, MAX_GAME_ASPECT, OrbitArmLimit, OrbitCameraSettings, OrbitRuntimeState,
	OrbitStepInput, OrbitStepResult, orbit_pose, required_probe_radius, resolve_orbit,
};

#[derive(Clone)]
pub struct GameCameraController {
	camera: Camera,
	follow: Option<FollowCameraSettings>,
	orbit: Option<orbit::OrbitRig>,
}

impl GameCameraController {
	fn fixed_or_follow(world: &RuntimeWorld, authored: &CameraData) -> Result<Self, String> {
		let mut controller = Self {
			camera: authored.to_camera(),
			follow: authored.follow.clone(),
			orbit: None,
		};
		follow::step(
			&mut controller.camera,
			controller.follow.as_ref(),
			world,
			0.0,
			true,
		)?;
		Ok(controller)
	}

	pub(crate) fn hold_pose(camera: Camera) -> Self {
		Self {
			camera,
			follow: None,
			orbit: None,
		}
	}

	pub(crate) fn is_following(&self) -> bool {
		self.follow.is_some()
	}

	pub(crate) fn tracks_entity(&self, entity: &EntityKey) -> bool {
		self.follow
			.as_ref()
			.is_some_and(|rig| &rig.target == entity)
			|| self
				.orbit_state()
				.is_some_and(|rig| &rig.target.key == entity)
	}

	pub fn camera(&self) -> &Camera {
		&self.camera
	}

	pub fn needs_tick(&self, world: &RuntimeWorld) -> bool {
		match &self.orbit {
			Some(rig) => rig.needs_tick(world),
			None => follow::needs_tick(&self.camera, self.follow.as_ref(), world),
		}
	}
}
