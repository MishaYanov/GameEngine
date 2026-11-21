use super::{OrbitArmLimit, OrbitCameraSettings};
use crate::{
	collision::{CameraExclusions, CameraProbe, CollisionWorld},
	runtime_lifecycle::{RuntimeEntityHandle, RuntimeLifecycle},
	world::RuntimeWorld,
};

/// Each arm cast sees the colliders and topology of the same candidate world.
/// The caller owns the cache and budget for the entire admission or tick.
pub(crate) fn obstruction_query<'a>(
	world: &'a RuntimeWorld,
	lifecycle: &'a RuntimeLifecycle,
	collisions: &'a CollisionWorld,
	exclusions: &'a mut Option<CameraExclusions>,
	remaining: &'a mut usize,
) -> impl FnMut(
	&RuntimeEntityHandle,
	&OrbitCameraSettings,
	[f32; 3],
	[f32; 3],
	f32,
) -> Result<OrbitArmLimit, String>
+ 'a {
	move |target, settings, pivot, forward, distance| {
		let cache = match &mut *exclusions {
			Some(cache) if cache.matches(world, target, lifecycle.topology_revision) => cache,
			slot => slot.insert(CameraExclusions::build(
				world, lifecycle, target, remaining,
			)?),
		};
		let limit = collisions.cast_camera_probe(
			world,
			CameraProbe {
				pivot,
				forward,
				distance,
				radius: settings.probe_radius,
				obstruction_mask: settings.obstruction_mask,
				target,
				topology_revision: lifecycle.topology_revision,
			},
			cache,
			remaining,
		)?;
		Ok(OrbitArmLimit {
			distance: limit.distance,
			hit: limit.hit,
		})
	}
}
