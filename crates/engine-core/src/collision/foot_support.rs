//! Bounded downward support query for animated foot placement.
use super::{CollisionWorld, Shape};
use crate::{EntityKey, world::RuntimeWorld};
use glam::Vec3;

const MAX_FOOT_PROBE_VISITS: usize = 8192;

impl CollisionWorld {
	pub(crate) fn foot_support(
		&self,
		world: &RuntimeWorld,
		owner: &EntityKey,
		sole: Vec3,
		reach: f32,
		remaining: &mut usize,
	) -> Option<f32> {
		if !sole.is_finite() || !reach.is_finite() || reach <= 0.0 {
			return None;
		}
		let capacity = MAX_FOOT_PROBE_VISITS.min(*remaining);
		let mut budget = capacity;
		let result = (|| {
			charge(&mut budget)?;
			let owner_index = world.entity_index(owner)?;
			let interaction = self
				.indices
				.get(owner)
				.map(|&index| self.records[index].interaction)
				.unwrap_or_default();
			let start = sole.y + reach;
			let end = sole.y - reach;
			let mut best: Option<f32> = None;
			for entry in &self.ordered {
				charge(&mut budget)?;
				let collider = &self.records[entry.index];
				let min = collider.bounds.min();
				let max = collider.bounds.max();
				if min[0] > sole.x {
					break;
				}
				if max[0] < sole.x
					|| sole.z < min[2]
					|| sole.z > max[2]
					|| max[1] < end || min[1] > start
					|| !self
					.matrix
					.allows_blocking(interaction, collider.interaction)
				{
					continue;
				}
				if in_family(world, collider.entity_index, owner_index, &mut budget)?
					|| in_family(world, owner_index, collider.entity_index, &mut budget)?
				{
					continue;
				}
				let height = match collider.shape {
					Shape::Box => max[1],
					Shape::Sphere { center, radius } => {
						let dx = sole.x - center.x;
						let dz = sole.z - center.z;
						let discriminant = radius * radius - dx * dx - dz * dz;
						if discriminant < 0.0 {
							continue;
						}
						center.y + discriminant.sqrt()
					}
				};
				if (end..=start).contains(&height) && best.is_none_or(|current| height > current) {
					best = Some(height);
				}
			}
			best
		})();
		*remaining -= capacity - budget;
		result
	}
}

fn charge(budget: &mut usize) -> Option<()> {
	*budget = budget.checked_sub(1)?;
	Some(())
}

fn in_family(
	world: &RuntimeWorld,
	mut index: usize,
	ancestor: usize,
	budget: &mut usize,
) -> Option<bool> {
	loop {
		if index == ancestor {
			return Some(true);
		}
		let Some(parent) = world.data().entities.get(index)?.parent.as_ref() else {
			return Some(false);
		};
		charge(budget)?;
		index = world.entity_index(parent)?;
	}
}
