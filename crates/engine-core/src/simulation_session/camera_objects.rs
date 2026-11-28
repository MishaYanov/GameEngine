use super::*;
use crate::{
	runtime_lifecycle::{RuntimeEntityHandle, RuntimeLifecycle},
	scene_data::CameraData,
};

pub(super) struct CameraObstructionStep<'a> {
	pub(super) exclusions: &'a mut Option<CameraExclusions>,
	pub(super) collisions: &'a CollisionWorld,
	pub(super) budget: &'a mut usize,
}

#[derive(Clone)]
pub(super) struct CameraObjects {
	default: Option<RuntimeEntityHandle>,
	override_camera: Option<RuntimeEntityHandle>,
	pub(super) sequence_camera: Option<RuntimeEntityHandle>,
	pub(super) active: Option<RuntimeEntityHandle>,
	unavailable: bool,
}

impl CameraObjects {
	pub(super) fn new(world: &RuntimeWorld, lifecycle: &RuntimeLifecycle) -> Self {
		let default = world
			.data()
			.default_game_camera
			.as_ref()
			.and_then(|key| lifecycle.handle(key));
		Self {
			active: default.clone(),
			default,
			override_camera: None,
			sequence_camera: None,
			unavailable: false,
		}
	}

	pub(super) fn projection(
		handle: &RuntimeEntityHandle,
		world: &RuntimeWorld,
		lifecycle: &RuntimeLifecycle,
	) -> Option<CameraData> {
		lifecycle.validate_handle(handle).ok()?;
		let index = world.entity_index(&handle.key)?;
		let camera = world.data().entities[index].camera.as_ref()?;
		if !camera.enabled {
			return None;
		}
		camera
			.settings
			.from_world_matrix(world.world_matrices()[index])
			.ok()
	}

	pub(super) fn stage_override(
		&mut self,
		target: Option<RuntimeEntityHandle>,
		world: &RuntimeWorld,
		lifecycle: &RuntimeLifecycle,
	) -> Result<(), String> {
		if let Some(handle) = &target
			&& Self::projection(handle, world, lifecycle).is_none()
		{
			return Err(
				"Runtime camera switch requires a live enabled Camera in this world".into(),
			);
		}
		self.override_camera = target;
		Ok(())
	}

	pub(super) fn advance(
		&mut self,
		world: &RuntimeWorld,
		lifecycle: &RuntimeLifecycle,
		camera: &mut GameCameraController,
		tick: u64,
		obstructions: CameraObstructionStep<'_>,
	) -> Result<bool, String> {
		let mut override_projection = self.override_camera.as_ref().and_then(|handle| {
			Self::projection(handle, world, lifecycle).map(|scene| (handle.clone(), scene))
		});
		if override_projection.is_none() {
			self.override_camera = None;
		}
		let sequence_projection = self.sequence_camera.as_ref().and_then(|handle| {
			Self::projection(handle, world, lifecycle).map(|scene| (handle.clone(), scene))
		});
		let resolved = sequence_projection
			.or_else(|| override_projection.take())
			.or_else(|| {
				let handle = self.default.as_ref()?;
				Self::projection(handle, world, lifecycle).map(|scene| (handle.clone(), scene))
			});
		let next = resolved.as_ref().map(|(handle, _)| handle);
		let unavailable = self.default.is_some() && next.is_none();
		let changed = self.active.as_ref() != next || self.unavailable != unavailable;
		if changed {
			let replacement = if unavailable {
				GameCameraController::hold_pose(camera.camera().clone())
			} else {
				let authored = resolved
					.as_ref()
					.map(|(_, scene)| scene)
					.unwrap_or(&world.data().camera);
				*obstructions.exclusions = None;
				GameCameraController::from_authored(
					world,
					lifecycle,
					authored,
					tick,
					obstruction_query(
						world,
						lifecycle,
						obstructions.collisions,
						obstructions.exclusions,
						obstructions.budget,
					),
				)?
			};
			*camera = replacement;
			self.active = next.cloned();
			self.unavailable = unavailable;
		} else if let Some((_, scene)) = resolved
			&& scene.follow.is_none()
			&& scene.orbit.is_none()
			&& !camera.is_following()
			&& camera.orbit_state().is_none()
		{
			*camera = GameCameraController::hold_pose(scene.to_camera());
		}
		Ok(changed)
	}
}

impl SimulationSession {
	/// The active object belongs to this run's world. A legacy camera has no object handle.
	pub fn active_camera_object(&self) -> Option<&RuntimeEntityHandle> {
		self.camera_objects.active.as_ref()
	}

	pub fn owns_game_camera_view(&self) -> bool {
		self.camera_objects.active.is_some()
			|| self.camera_objects.unavailable
			|| self.camera.is_following()
			|| self.camera.orbit_state().is_some()
	}

	pub fn camera_object_diagnostic(&self) -> Option<&'static str> {
		self.camera_objects
			.unavailable
			.then_some("Game Camera is unavailable; retaining the last valid view")
	}

	pub fn set_runtime_camera(
		&mut self,
		target: Option<RuntimeEntityHandle>,
	) -> Result<(), String> {
		let mut objects = self.camera_objects.clone();
		let owner_changed = objects.override_camera != target;
		objects.stage_override(target, &self.world, &self.lifecycle)?;
		let mut camera = self.camera.clone();
		let mut exclusions = self.camera_exclusions.clone();
		let mut budget = Self::MAX_COLLISION_CANDIDATES_PER_TICK;
		let changed = objects.advance(
			&self.world,
			&self.lifecycle,
			&mut camera,
			self.tick,
			CameraObstructionStep {
				exclusions: &mut exclusions,
				collisions: &self.collisions,
				budget: &mut budget,
			},
		)?;
		if changed || owner_changed {
			self.camera_input.clear();
		}
		self.camera_objects = objects;
		self.camera = camera;
		self.camera_exclusions = exclusions;
		Ok(())
	}
}
