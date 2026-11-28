use std::sync::Arc;

use renderer::vulkan::EditorPickGeometry;

use super::*;

#[derive(Debug, Clone, Copy)]
pub struct Ray {
	pub origin: Vec3,
	pub direction: Vec3,
	pub max_distance: f32,
}
impl Ray {
	pub fn plane(self, origin: Vec3, normal: Vec3) -> Option<Vec3> {
		if !self.origin.is_finite()
			|| !self.direction.is_finite()
			|| !self.max_distance.is_finite()
			|| self.max_distance < 0.0
			|| !origin.is_finite()
			|| !normal.is_finite()
		{
			return None;
		}
		let denominator = self.direction.dot(normal);
		if !denominator.is_finite() || denominator.abs() < EPSILON {
			return None;
		}
		let distance = (origin - self.origin).dot(normal) / denominator;
		if !distance.is_finite() || distance < 0.0 || distance > self.max_distance {
			return None;
		}
		Some(self.origin + self.direction * distance)
	}
}

pub struct PickTarget {
	pub key: String,
	pub bounds: Bounds3,
	surface: Option<PickSurface>,
}

struct PickSurface {
	local_from_world: Mat4,
	parts: Arc<[Arc<EditorPickGeometry>]>,
}

impl PickTarget {
	pub fn new(key: String, bounds: Bounds3) -> Self {
		Self {
			key,
			bounds,
			surface: None,
		}
	}

	pub fn with_surface(
		mut self,
		local_from_world: Mat4,
		parts: Arc<[Arc<EditorPickGeometry>]>,
	) -> Self {
		self.surface = Some(PickSurface {
			local_from_world,
			parts,
		});
		self
	}

	fn bounds_distance(&self, ray: Ray) -> Option<f32> {
		self.bounds.ray_distance(ray.origin, ray.direction, ray.max_distance)
	}

	fn surface_distance(&self, ray: Ray) -> Option<f32> {
		let entry = self.bounds_distance(ray)?;
		let Some(surface) = &self.surface else {
			return Some(entry);
		};
		let origin = surface.local_from_world.transform_point3(ray.origin);
		let direction = surface.local_from_world.transform_vector3(ray.direction);
		let mut closest = None;
		let mut limit = ray.max_distance;
		for part in surface.parts.iter() {
			if let Some(distance) = part.ray_distance(origin, direction, limit) {
				closest = Some(distance);
				limit = distance;
			}
		}
		closest
	}
}

pub fn pick_nearest(ray: Ray, targets: &[PickTarget]) -> Option<&str> {
	closest_target(ray, targets, PickTarget::surface_distance)
		.map(|(target, _)| target.key.as_str())
}

fn closest_target(
	mut ray: Ray,
	targets: &[PickTarget],
	distance_to: impl Fn(&PickTarget, Ray) -> Option<f32>,
) -> Option<(&PickTarget, f32)> {
	let mut nearest = None;
	for target in targets {
		if let Some(distance) = distance_to(target, ray) {
			if nearest.is_some() && distance >= ray.max_distance {
				continue;
			}
			nearest = Some((target, distance));
			ray.max_distance = distance;
		}
	}
	nearest
}

fn hit_surface(ray: Ray, bounds: Bounds3, distance: f32) -> (Vec3, Vec3) {
	let position = ray.origin + ray.direction * distance;
	let min = Vec3::from_array(bounds.min());
	let max = Vec3::from_array(bounds.max());
	let mut normal = -ray.direction;
	let mut closest = f32::INFINITY;
	for axis in 0..3 {
		for (edge, sign) in [(min[axis], -1.0), (max[axis], 1.0)] {
			let separation = (position[axis] - edge).abs();
			if separation < closest {
				closest = separation;
				normal = Vec3::ZERO;
				normal[axis] = sign;
			}
		}
	}
	(position, normal)
}

pub fn plane_basis(plane: ViewportPlane) -> (Vec3, Vec3, Vec3) {
	match plane {
		ViewportPlane::Xz => (Vec3::Y, Vec3::X, -Vec3::Z),
		ViewportPlane::Xy => (Vec3::Z, Vec3::X, Vec3::Y),
		ViewportPlane::Yz => (Vec3::X, Vec3::Y, Vec3::Z),
	}
}

/// A bounds surface is a placement approximation, not a triangle-accurate mesh hit.
pub fn placement_point(
	ray: Ray,
	targets: &[PickTarget],
	plane: ViewportPlane,
	snap: Option<f32>,
) -> Option<Vec3> {
	let (mut point, normal) = if let Some((target, distance)) =
		closest_target(ray, targets, PickTarget::bounds_distance)
	{
		hit_surface(ray, target.bounds, distance)
	} else {
		let normal = plane_basis(plane).0;
		(ray.plane(Vec3::ZERO, normal)?, normal)
	};
	if let Some(spacing) = snap.filter(|value| value.is_finite() && *value > 0.0) {
		let locked = normal.abs().max_element();
		for axis in 0..3 {
			if normal[axis].abs() < locked - 0.001 {
				point[axis] = (point[axis] / spacing).round() * spacing;
			}
		}
	}
	point.is_finite().then_some(point)
}

/// Visual LOD is separate from tool snapping and never changes the settings DTO.
pub fn adaptive_grid_spacing(base: f32, units_per_pixel: f32) -> f32 {
	if !base.is_finite() || base <= 0.0 || !units_per_pixel.is_finite() || units_per_pixel <= 0.0 {
		return 1.0;
	}
	let desired = units_per_pixel * 32.0;
	let exponent = ((desired / base).log10().floor()).clamp(-6.0, 6.0);
	let decade = base * 10.0_f32.powf(exponent);
	let multiplier = [1.0, 2.0, 5.0, 10.0]
		.into_iter()
		.find(|factor| decade * *factor >= desired)
		.unwrap_or(10.0);
	(decade * multiplier).clamp(1.0e-6, MAX_DISTANCE)
}
