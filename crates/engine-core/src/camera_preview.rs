//! CPU-only orbit preview for a frozen scene and a candidate camera definition.
use std::sync::Arc;

use crate::{
	Camera, SceneData,
	camera_input::CameraIntent,
	collision::{CameraExclusions, CollisionWorld},
	game_camera::{
		GameCameraController, OrbitCameraSettings, OrbitRuntimeState, obstruction_query,
	},
	runtime_lifecycle::RuntimeLifecycle,
	simulation_session::SimulationSession,
	world::RuntimeWorld,
};

struct FrozenScene {
	world: RuntimeWorld,
	lifecycle: RuntimeLifecycle,
	collisions: CollisionWorld,
}

/// Preview copies share the frozen scene and advance their camera independently.
#[derive(Clone)]
pub struct FrozenOrbitPreview {
	scene: Arc<FrozenScene>,
	camera: GameCameraController,
	exclusions: Option<CameraExclusions>,
	fixed_seconds: f32,
}

impl FrozenOrbitPreview {
	/// Admit the draft rig and check its first pose against the frozen scene.
	pub fn from_document(
		document: Arc<SceneData>,
		settings: OrbitCameraSettings,
		template: Camera,
		fixed_seconds: f32,
	) -> Result<Self, String> {
		if !fixed_seconds.is_finite() || fixed_seconds <= 0.0 || fixed_seconds > 1.0 {
			return Err("Preview needs a finite fixed interval in (0, 1] seconds".into());
		}
		let world = RuntimeWorld::new(document)?;
		let lifecycle = RuntimeLifecycle::new(&world);
		let handle = lifecycle
			.handle(&settings.target)
			.ok_or("Preview camera target is missing")?;
		let target = world.camera_target_snapshot(&handle)?;
		let collisions = CollisionWorld::new(&world)?;
		let mut remaining = SimulationSession::MAX_COLLISION_CANDIDATES_PER_TICK;
		let mut exclusions = None;
		let camera = {
			let mut query = obstruction_query(
				&world,
				&lifecycle,
				&collisions,
				&mut exclusions,
				&mut remaining,
			);
			GameCameraController::admit_orbit(
				template,
				&settings,
				&target,
				0,
				|pivot, forward, distance| query(&handle, &settings, pivot, forward, distance),
			)?
		};
		Ok(Self {
			scene: Arc::new(FrozenScene {
				world,
				lifecycle,
				collisions,
			}),
			camera,
			exclusions,
			fixed_seconds,
		})
	}

	/// Return one accepted fixed step. A rejected sample preserves this preview for retry.
	pub fn advance(&self, intent: CameraIntent) -> Result<Self, String> {
		intent.validate()?;
		let tick = self
			.orbit_state()
			.tick
			.checked_add(1)
			.filter(|tick| *tick <= 9_007_199_254_740_991)
			.ok_or("Preview tick exceeds the safe JSON integer range")?;
		let mut next = self.clone();
		let mut remaining = SimulationSession::MAX_COLLISION_CANDIDATES_PER_TICK;
		let FrozenScene {
			world,
			lifecycle,
			collisions,
		} = self.scene.as_ref();
		next.camera.step(
			world,
			lifecycle,
			self.fixed_seconds,
			intent,
			tick,
			obstruction_query(
				world,
				lifecycle,
				collisions,
				&mut next.exclusions,
				&mut remaining,
			),
		)?;
		Ok(next)
	}

	pub fn pose(&self) -> &Camera {
		self.camera.camera()
	}

	pub fn orbit_state(&self) -> &OrbitRuntimeState {
		self.camera
			.orbit_state()
			.expect("preview always owns an admitted orbit rig")
	}
}
