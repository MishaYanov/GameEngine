use super::WorldRegions;
use crate::{
	Bounds3,
	collision::CollisionWorld,
	project::{ProjectData, RegionBounds},
	scene_catalog::SceneId,
	world::RuntimeWorld,
};
use glam::{DVec3, Vec3};

pub(crate) struct RegionMovementGuard {
	barriers: Vec<RegionBounds>,
}

pub(crate) struct GuardedMotion {
	pub delta: Vec3,
	blocked: [bool; 3],
}

impl GuardedMotion {
	pub fn blocked(&self) -> bool {
		self.blocked.iter().any(|&blocked| blocked)
	}

	pub fn blocks_vertical_motion(&self) -> bool {
		self.blocked[1]
	}

	pub fn stop_velocity(&self, velocity: &mut Vec3) {
		for (axis, blocked) in self.blocked.into_iter().enumerate() {
			if blocked {
				velocity[axis] = 0.0;
			}
		}
	}
}

impl RegionMovementGuard {
	pub(in crate::simulation_session) fn new(
		project: &ProjectData,
		scene: &SceneId,
		regions: &WorldRegions,
	) -> Self {
		let barriers = project
			.regions_for(scene)
			.filter(|region| !regions.resident.contains_key(region.scene.as_str()))
			.map(|region| region.bounds)
			.collect();
		Self { barriers }
	}

	pub fn legacy_body(
		&self,
		world: &RuntimeWorld,
		collisions: &CollisionWorld,
		index: usize,
		remaining: &mut usize,
	) -> Result<Option<Bounds3>, String> {
		if self.barriers.is_empty() {
			return Ok(None);
		}
		let indices = world.subtree_indices(index)?;
		*remaining = remaining
			.checked_sub(indices.len())
			.ok_or("Region movement work budget exceeded")?;
		let combined = indices
			.into_iter()
			.filter_map(|index| {
				collisions
					.body_bounds(&world.data().entities[index].key)
					.ok()
			})
			.reduce(Bounds3::union);
		if combined.is_some() {
			return Ok(combined);
		}
		let position = world.world_matrices()[index].w_axis.truncate().to_array();
		Bounds3::new(position, position).map(Some)
	}

	/// Limit the swept body against unavailable geometry. Slide tangentially
	/// along the boundary, just as for an ordinary static collider.
	pub fn clamp(&self, body: Bounds3, delta: Vec3) -> GuardedMotion {
		let mut result = GuardedMotion {
			delta,
			blocked: [false; 3],
		};
		if self.barriers.is_empty() || delta == Vec3::ZERO || !delta.is_finite() {
			return result;
		}
		let low = Vec3::from_array(body.min()).as_dvec3();
		let high = Vec3::from_array(body.max()).as_dvec3();
		let mut applied = DVec3::ZERO;
		let mut remaining = delta.as_dvec3();
		for _ in 0..3 {
			let mut first = None;
			for barrier in &self.barriers {
				match entrance(low + applied, high + applied, remaining, *barrier) {
					Entry::Clear => {}
					Entry::Hold => {
						return GuardedMotion {
							delta: Vec3::ZERO,
							blocked: [true; 3],
						};
					}
					Entry::Contact { fraction, axis } => {
						if first.is_none_or(|(current, previous_axis)| {
							fraction < current || fraction == current && axis < previous_axis
						}) {
							first = Some((fraction, axis));
						}
					}
				}
			}
			let Some((fraction, axis)) = first else {
				applied += remaining;
				break;
			};
			// Keep the final f32 transform on the loaded side even when its
			// ulp is larger than the collision skin at the local origin.
			let scale = (low[axis] + applied[axis])
				.abs()
				.max((high[axis] + applied[axis]).abs());
			let skin = 0.001 + scale * f64::from(f32::EPSILON);
			let travel = (fraction - skin / remaining[axis].abs()).max(0.0);
			applied += remaining * travel;
			remaining *= 1.0 - travel;
			remaining[axis] = 0.0;
			result.blocked[axis] = true;
			if remaining == DVec3::ZERO {
				break;
			}
		}
		result.delta = applied.as_vec3();
		result
	}
}

enum Entry {
	Clear,
	Hold,
	Contact { fraction: f64, axis: usize },
}

fn entrance(low: DVec3, high: DVec3, delta: DVec3, region: RegionBounds) -> Entry {
	let min = Vec3::from_array(region.min).as_dvec3();
	let max = Vec3::from_array(region.max).as_dvec3();
	let center = (low + high) * 0.5;
	let lower = min - high;
	let upper = max - low;

	if (0..3).all(|axis| lower[axis] < 0.0 && upper[axis] > 0.0) {
		// A spawn in an unready cell has no valid collision support yet.
		if (0..3).all(|axis| min[axis] < center[axis] && center[axis] < max[axis]) {
			return Entry::Hold;
		}
		// A body can overlap a seam while its center is on the ready side.
		// Permit retreat and tangential travel, but avoid deeper overlap.
		let mut barrier_axis = None;
		for axis in 0..3 {
			if (center[axis] <= min[axis] && delta[axis] <= 0.0)
				|| (center[axis] >= max[axis] && delta[axis] >= 0.0)
			{
				return Entry::Clear;
			}
			if center[axis] <= min[axis] || center[axis] >= max[axis] {
				barrier_axis.get_or_insert(axis);
			}
		}
		return barrier_axis.map_or(Entry::Hold, |axis| Entry::Contact {
			fraction: 0.0,
			axis,
		});
	}

	let mut near = 0.0_f64;
	let mut far = 1.0_f64;
	let mut axis_at_entry = None;
	for axis in 0..3 {
		let motion = delta[axis];
		if motion == 0.0 {
			if lower[axis] >= 0.0 || upper[axis] <= 0.0 {
				return Entry::Clear;
			}
			continue;
		}
		let a = lower[axis] / motion;
		let b = upper[axis] / motion;
		let entry = a.min(b);
		if entry >= near {
			near = entry;
			axis_at_entry = Some(axis);
		}
		far = far.min(a.max(b));
		if near >= far {
			return Entry::Clear;
		}
	}
	axis_at_entry.map_or(Entry::Clear, |axis| Entry::Contact {
		fraction: near,
		axis,
	})
}
