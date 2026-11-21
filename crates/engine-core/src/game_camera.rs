//! Authored game camera modes and the fixed-step controller that publishes their runtime pose.
mod follow;
mod obstruction;
mod orbit;

use crate::{Camera, world::RuntimeWorld};

pub use follow::FollowCameraSettings;
pub(crate) use obstruction::obstruction_query;
pub use orbit::{
	CameraTargetSnapshot, MAX_GAME_ASPECT, OrbitArmLimit, OrbitCameraSettings, OrbitRuntimeState,
	OrbitStepInput, OrbitStepResult, orbit_pose, required_probe_radius, resolve_orbit,
};

#[derive(Clone)]
pub struct GameCameraController {
	camera: Camera,
	orbit: Option<orbit::OrbitRig>,
}

impl GameCameraController {
	fn template(world: &RuntimeWorld) -> Camera {
		let authored = &world.data().camera;
		Camera {
			position: authored.position,
			yaw: authored.yaw,
			pitch: authored.pitch,
			vertical_fov: authored.vertical_fov,
			near: authored.near_plane,
			far: authored.far_plane,
		}
	}

	fn fixed_or_follow(world: &RuntimeWorld) -> Result<Self, String> {
		let mut controller = Self {
			camera: Self::template(world),
			orbit: None,
		};
		follow::step(&mut controller.camera, world, 0.0, true)?;
		Ok(controller)
	}

	pub fn camera(&self) -> &Camera {
		&self.camera
	}

	pub fn needs_tick(&self, world: &RuntimeWorld) -> bool {
		match &self.orbit {
			Some(rig) => rig.needs_tick(world),
			None => follow::needs_tick(&self.camera, world),
		}
	}
}
