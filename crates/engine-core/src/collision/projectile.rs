//! A swept point query for lightweight gameplay projectile Props.

use super::{CollisionWorld, Shape, consume_candidates};
use crate::{Bounds3, EntityKey, runtime_lifecycle::RuntimeLifecycle, world::RuntimeWorld};
use glam::Vec3;

pub(crate) struct ProjectileContact {
	pub target: EntityKey,
	pub position: [f32; 3],
	pub normal: [f32; 3],
}

impl CollisionWorld {
	pub(crate) fn projectile_contact(
		&self,
		world: &RuntimeWorld,
		lifecycle: &RuntimeLifecycle,
		projectile: &crate::projectile::ProjectileInstance,
		origin: Vec3,
		delta: Vec3,
		remaining: &mut usize,
	) -> Result<Option<ProjectileContact>, String> {
		let end = origin + delta;
		if !origin.is_finite() || !end.is_finite() || delta == Vec3::ZERO {
			return Err("Projectile motion needs a finite, nonzero segment".into());
		}
		if projectile.settings.collision_mask == 0 {
			return Ok(None);
		}
		let instance = lifecycle.instance_for(&projectile.entity);
		let mut interaction = self
			.indices
			.get(&projectile.entity)
			.map_or_else(crate::collider::ColliderInteraction::default, |&index| {
				self.records[index].interaction
			});
		interaction.sensor = false;
		interaction.mask = projectile.settings.collision_mask;
		let max_x = origin.x.max(end.x);
		let mut best: Option<(f32, &EntityKey, Vec3)> = None;
		for entry in &self.ordered {
			consume_candidates(remaining, 1)?;
			let collider = &self.records[entry.index];
			if collider.bounds.min()[0] > max_x {
				break;
			}
			if !self
				.matrix
				.allows_blocking(interaction, collider.interaction)
			{
				continue;
			}
			let Some(box_time) = collider.bounds.ray_distance(origin, delta, 1.0) else {
				continue;
			};
			if box_time == 0.0
				&& (0..3).any(|axis| {
				(origin[axis] == collider.bounds.min()[axis] && delta[axis] < 0.0)
					|| (origin[axis] == collider.bounds.max()[axis] && delta[axis] > 0.0)
			}) {
				continue;
			}
			if lifecycle.instance_for(&collider.key) == instance
				|| is_launch_group(world, collider.entity_index, projectile, remaining)?
			{
				continue;
			}
			let (fraction, normal) = match collider.shape {
				Shape::Box => (
					box_time,
					box_normal(collider.bounds, origin + delta * box_time),
				),
				Shape::Sphere { center, radius } => {
					let Some(contact) = sphere_contact(center, radius, origin, delta) else {
						continue;
					};
					contact
				}
			};
			if best.is_none_or(|(time, key, _)| {
				fraction < time || fraction == time && &collider.key < key
			}) {
				best = Some((fraction, &collider.key, normal));
			}
		}
		Ok(best.map(|(fraction, target, normal)| ProjectileContact {
			target: target.clone(),
			position: (origin + delta * fraction).to_array(),
			normal: normal.to_array(),
		}))
	}
}

fn is_launch_group(
	world: &RuntimeWorld,
	mut index: usize,
	projectile: &crate::projectile::ProjectileInstance,
	remaining: &mut usize,
) -> Result<bool, String> {
	let Some(launcher) = &projectile.launcher else {
		return Ok(false);
	};
	loop {
		consume_candidates(remaining, 1)?;
		let entity = &world.data().entities[index];
		if entity.key == launcher.key {
			return Ok(true);
		}
		let Some(parent) = &entity.parent else {
			return Ok(false);
		};
		index = world
			.entity_index(parent)
			.ok_or("Projectile collider parent is unavailable")?;
	}
}

fn box_normal(bounds: Bounds3, point: Vec3) -> Vec3 {
	let mut nearest = f64::INFINITY;
	let mut normal = Vec3::ZERO;
	for axis in 0..3 {
		for (boundary, sign) in [(bounds.min()[axis], -1.), (bounds.max()[axis], 1.)] {
			let distance = (f64::from(point[axis]) - f64::from(boundary)).abs();
			if distance < nearest {
				nearest = distance;
				normal = Vec3::ZERO;
				normal[axis] = sign;
			}
		}
	}
	normal
}

fn sphere_contact(center: Vec3, radius: f32, origin: Vec3, delta: Vec3) -> Option<(f32, Vec3)> {
	let relative = origin.as_dvec3() - center.as_dvec3();
	let ray = delta.as_dvec3();
	let c = relative.length_squared() - f64::from(radius).powi(2);
	let b = relative.dot(ray);
	let fraction = if c <= 0.0 {
		if c == 0.0 && b > 0.0 {
			return None;
		}
		0.0
	} else {
		let a = ray.length_squared();
		let discriminant = b * b - a * c;
		if b >= 0.0 || discriminant < 0.0 {
			return None;
		}
		c / (-b + discriminant.sqrt())
	};
	(0.0..=1.0).contains(&fraction).then(|| {
		let normal = (relative + ray * fraction)
			.try_normalize()
			.unwrap_or_else(|| -ray.normalize());
		(fraction as f32, normal.as_vec3())
	})
}
