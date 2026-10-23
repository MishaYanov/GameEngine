use glam::{Mat4, Vec3};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds3 {
	min: [f32; 3],
	max: [f32; 3],
}

impl Bounds3 {
	pub fn new(min: [f32; 3], max: [f32; 3]) -> Result<Self, String> {
		if !min.iter().chain(max.iter()).all(|value| value.is_finite())
			|| (0..3).any(|axis| min[axis] > max[axis])
		{
			return Err("Bounds must be finite and ordered on every axis".to_string());
		}
		Ok(Self { min, max })
	}

	pub fn from_points(points: impl IntoIterator<Item=[f32; 3]>) -> Result<Option<Self>, String> {
		let mut bounds: Option<Self> = None;
		for point in points {
			let point_bounds = Self::new(point, point)?;
			bounds = Some(bounds.map_or(point_bounds, |bounds| bounds.union(point_bounds)));
		}
		Ok(bounds)
	}

	pub fn min(&self) -> [f32; 3] {
		self.min
	}
	pub fn max(&self) -> [f32; 3] {
		self.max
	}

	pub fn union(self, other: Self) -> Self {
		Self {
			min: Vec3::from_array(self.min)
				.min(Vec3::from_array(other.min))
				.to_array(),
			max: Vec3::from_array(self.max)
				.max(Vec3::from_array(other.max))
				.to_array(),
		}
	}

	pub fn transformed(self, matrix: Mat4) -> Result<Self, String> {
		let corners = (0..8).map(|corner| {
			let point = Vec3::new(
				if corner & 1 == 0 {
					self.min[0]
				} else {
					self.max[0]
				},
				if corner & 2 == 0 {
					self.min[1]
				} else {
					self.max[1]
				},
				if corner & 4 == 0 {
					self.min[2]
				} else {
					self.max[2]
				},
			);
			matrix.transform_point3(point).to_array()
		});
		Self::from_points(corners)?.ok_or_else(|| "Bounds have no corners".to_string())
	}

	// Direction is deliberately not normalized: after transforming a world ray
	// into model space, t must continue to measure world-space distance.
	pub fn ray_distance(self, origin: Vec3, direction: Vec3, max_distance: f32) -> Option<f32> {
		if !origin.is_finite()
			|| !direction.is_finite()
			|| direction == Vec3::ZERO
			|| !max_distance.is_finite()
			|| max_distance < 0.0
		{
			return None;
		}
		let mut near = 0.0_f32;
		let mut far = max_distance;
		for axis in 0..3 {
			if direction[axis] == 0.0 {
				if origin[axis] < self.min[axis] || origin[axis] > self.max[axis] {
					return None;
				}
				continue;
			}
			let a = (self.min[axis] - origin[axis]) / direction[axis];
			let b = (self.max[axis] - origin[axis]) / direction[axis];
			near = near.max(a.min(b));
			far = far.min(a.max(b));
			if near > far {
				return None;
			}
		}
		Some(near)
	}
}
