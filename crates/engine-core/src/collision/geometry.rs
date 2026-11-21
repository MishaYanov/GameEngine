use crate::{
	Bounds3, EntityKey,
	collider::ColliderInteraction,
	scene_data::{ColliderData, SceneData},
};
use glam::{Mat4, Vec3};

#[derive(Clone, Copy)]
pub(super) enum Shape {
	Box,
	Sphere { center: Vec3, radius: f32 },
}

pub(super) struct WorldCollider<'a> {
	pub(super) entity: &'a EntityKey,
	pub(super) entity_index: usize,
	pub(super) bounds: Bounds3,
	pub(super) shape: Shape,
	pub(super) interaction: ColliderInteraction,
}

pub(super) fn world_colliders<'a>(
	data: &'a SceneData,
	matrices: &[Mat4],
) -> Result<Vec<WorldCollider<'a>>, String> {
	let mut colliders = Vec::new();
	for (index, (entity, matrix)) in data.entities.iter().zip(matrices).enumerate() {
		if !entity.collision_enabled {
			continue;
		}
		let Some(collider) = &entity.collider else {
			continue;
		};
		let (bounds, shape) = collider_geometry(collider, *matrix)?;
		colliders.push(WorldCollider {
			entity: &entity.key,
			entity_index: index,
			bounds,
			shape,
			interaction: collider.interaction(),
		});
	}
	Ok(colliders)
}

pub(super) fn collider_geometry(
	collider: &ColliderData,
	matrix: Mat4,
) -> Result<(Bounds3, Shape), String> {
	collider.validate()?;
	let geometry = match collider {
		ColliderData::Box {
			half_extents,
			offset,
			..
		} => {
			let offset = Vec3::from_array(*offset);
			let half = Vec3::from_array(*half_extents);
			let bounds = Bounds3::new((offset - half).to_array(), (offset + half).to_array())?
				.transformed(matrix)?;
			(bounds, Shape::Box)
		}
		ColliderData::Sphere { radius, offset, .. } => {
			let center = matrix.transform_point3(Vec3::from_array(*offset));
			let radius = (*radius as f64 * conservative_scale(matrix)) as f32;
			if !radius.is_finite() || !center.is_finite() {
				return Err("Sphere collider produces nonfinite world geometry".into());
			}
			let half = Vec3::splat(radius);
			(
				Bounds3::new((center - half).to_array(), (center + half).to_array())?,
				Shape::Sphere { center, radius },
			)
		}
	};
	Ok(geometry)
}

pub(super) fn conservative_scale(matrix: Mat4) -> f64 {
	let axes = [
		matrix.x_axis.truncate(),
		matrix.y_axis.truncate(),
		matrix.z_axis.truncate(),
	];
	// The largest absolute row sum of A^T A bounds its largest eigenvalue.
	// Orthogonal TRS columns reduce to the exact maximum axis scale; hierarchy
	// shear remains safely enclosed instead of underestimating the sphere.
	(0..3)
		.map(|row| {
			(0..3)
				.map(|column| {
					(0..3)
						.map(|axis| axes[row][axis] as f64 * axes[column][axis] as f64)
						.sum::<f64>()
						.abs()
				})
				.sum::<f64>()
		})
		.fold(0.0, f64::max)
		.sqrt()
}

pub(super) fn bounds_overlap(a: Bounds3, b: Bounds3) -> bool {
	(0..3).all(|axis| a.min()[axis] <= b.max()[axis] && a.max()[axis] >= b.min()[axis])
}

pub(super) fn contact_geometry(
	a: &WorldCollider<'_>,
	b: &WorldCollider<'_>,
) -> Option<(Vec3, f32)> {
	if !a.interaction.detects(b.interaction) {
		return None;
	}
	match (a.shape, b.shape) {
		(Shape::Box, Shape::Box) => box_box(a.bounds, b.bounds),
		(
			Shape::Sphere {
				center: a,
				radius: ar,
			},
			Shape::Sphere {
				center: b,
				radius: br,
			},
		) => {
			let (normal, distance) = direction_and_distance(a, b);
			let penetration = ar as f64 + br as f64 - distance;
			(penetration >= 0.0).then_some((normal, penetration as f32))
		}
		(Shape::Sphere { center, radius }, Shape::Box) => sphere_box(center, radius, b.bounds),
		(Shape::Box, Shape::Sphere { center, radius }) => {
			sphere_box(center, radius, a.bounds).map(|(normal, penetration)| (-normal, penetration))
		}
	}
}

fn box_box(a: Bounds3, b: Bounds3) -> Option<(Vec3, f32)> {
	if !bounds_overlap(a, b) {
		return None;
	}
	let mut penetration = f32::INFINITY;
	let mut normal = Vec3::X;
	for axis in 0..3 {
		let negative = a.max()[axis] - b.min()[axis];
		let positive = b.max()[axis] - a.min()[axis];
		let depth = negative.min(positive);
		if depth < penetration {
			penetration = depth;
			normal = Vec3::ZERO;
			normal[axis] = if negative <= positive { 1.0 } else { -1.0 };
		}
	}
	Some((normal, penetration))
}

fn sphere_box(center: Vec3, radius: f32, bounds: Bounds3) -> Option<(Vec3, f32)> {
	let closest = center.clamp(
		Vec3::from_array(bounds.min()),
		Vec3::from_array(bounds.max()),
	);
	let (normal, distance) = direction_and_distance(center, closest);
	if distance > 0.0 {
		let penetration = radius as f64 - distance;
		return (penetration >= 0.0).then_some((normal, penetration as f32));
	}
	// Containment needs distance to an escape face, not zero penetration.
	let mut depth = f32::INFINITY;
	let mut normal = Vec3::X;
	for axis in 0..3 {
		let negative = center[axis] - bounds.min()[axis];
		let positive = bounds.max()[axis] - center[axis];
		if negative.min(positive) < depth {
			depth = negative.min(positive);
			normal = Vec3::ZERO;
			normal[axis] = if negative <= positive { 1.0 } else { -1.0 };
		}
	}
	Some((normal, radius + depth))
}

fn direction_and_distance(from: Vec3, to: Vec3) -> (Vec3, f64) {
	let delta = [
		to.x as f64 - from.x as f64,
		to.y as f64 - from.y as f64,
		to.z as f64 - from.z as f64,
	];
	let distance = delta
		.into_iter()
		.map(|value| value * value)
		.sum::<f64>()
		.sqrt();
	let normal = if distance > 0.0 {
		Vec3::from_array(delta.map(|value| (value / distance) as f32))
	} else {
		Vec3::X
	};
	(normal, distance)
}

pub(super) fn translated<'a>(
	collider: &WorldCollider<'a>,
	delta: Vec3,
) -> Result<WorldCollider<'a>, String> {
	let bounds = Bounds3::new(
		(Vec3::from_array(collider.bounds.min()) + delta).to_array(),
		(Vec3::from_array(collider.bounds.max()) + delta).to_array(),
	)?;
	let shape = match collider.shape {
		Shape::Box => Shape::Box,
		Shape::Sphere { center, radius } => Shape::Sphere {
			center: center + delta,
			radius,
		},
	};
	Ok(WorldCollider {
		entity: collider.entity,
		entity_index: collider.entity_index,
		bounds,
		shape,
		interaction: collider.interaction,
	})
}

pub(super) fn sweep_hit(
	start: &WorldCollider<'_>,
	obstacle: &WorldCollider<'_>,
	delta: Vec3,
) -> Result<Option<(f64, Vec3)>, String> {
	if start.interaction.sensor
		|| obstacle.interaction.sensor
		|| !start.interaction.detects(obstacle.interaction)
	{
		return Ok(None);
	}
	if let Some((normal, penetration)) = contact_geometry(start, obstacle) {
		if !normal.is_finite() || !penetration.is_finite() {
			return Err("Collision contact exceeds finite limits".into());
		}
		return Ok((motion_dot(delta, normal) > 0.0).then_some((0.0, normal)));
	}
	if bounds_overlap(start.bounds, obstacle.bounds) {
		let normal = match (start.shape, obstacle.shape) {
			(Shape::Sphere { center: a, .. }, Shape::Sphere { center: b, .. }) => {
				direction_and_distance(a, b).0
			}
			(Shape::Sphere { center, .. }, Shape::Box) => {
				direction_and_distance(
					center,
					center.clamp(
						Vec3::from_array(obstacle.bounds.min()),
						Vec3::from_array(obstacle.bounds.max()),
					),
				)
				.0
			}
			(Shape::Box, Shape::Sphere { center, .. }) => {
				direction_and_distance(
					center.clamp(
						Vec3::from_array(start.bounds.min()),
						Vec3::from_array(start.bounds.max()),
					),
					center,
				)
				.0
			}
			_ => Vec3::ZERO,
		};
		return Ok((motion_dot(delta, normal) > 0.0).then_some((0.0, normal)));
	}
	// Sweep the enclosing world bounds. Curved contacts remain conservative.
	let min = Vec3::from_array(start.bounds.min()).as_dvec3();
	let max = Vec3::from_array(start.bounds.max()).as_dvec3();
	let center = (min + max) * 0.5;
	let half = (max - min) * 0.5;
	let low = Vec3::from_array(obstacle.bounds.min()).as_dvec3() - half;
	let high = Vec3::from_array(obstacle.bounds.max()).as_dvec3() + half;
	let mut entry = f64::NEG_INFINITY;
	let mut exit = 1.0_f64;
	let mut normal = Vec3::ZERO;
	for axis in 0..3 {
		let speed = f64::from(delta[axis]);
		if speed == 0.0 {
			if center[axis] < low[axis] || center[axis] > high[axis] {
				return Ok(None);
			}
			continue;
		}
		let first = (low[axis] - center[axis]) / speed;
		let second = (high[axis] - center[axis]) / speed;
		let near = first.min(second);
		if near > entry {
			entry = near;
			normal = Vec3::ZERO;
			normal[axis] = delta[axis].signum();
		}
		exit = exit.min(first.max(second));
		if entry > exit {
			return Ok(None);
		}
	}
	Ok((entry <= 1.0 && exit >= 0.0).then_some((entry.max(0.0), normal)))
}

fn motion_dot(delta: Vec3, direction: Vec3) -> f64 {
	(0..3)
		.map(|axis| delta[axis] as f64 * direction[axis] as f64)
		.sum()
}
