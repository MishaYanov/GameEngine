use crate::Bounds3;
use glam::{Mat4, Vec3, Vec4};

/// Vulkan clip planes: -w <= x,y <= w and 0 <= z <= w.
pub struct Frustum {
	planes: [Vec4; 6],
}

impl Frustum {
	pub fn from_view_projection(matrix: Mat4) -> Result<Self, String> {
		if !matrix.is_finite() {
			return Err("Frustum matrix must be finite".into());
		}
		let rows = matrix.transpose();
		let [x, y, z, w] = [rows.x_axis, rows.y_axis, rows.z_axis, rows.w_axis];
		let mut planes = [w + x, w - x, w + y, w - y, z, w - z];
		for plane in &mut planes {
			let length = plane.truncate().length();
			if !length.is_finite() || length <= f32::EPSILON {
				return Err("Degenerate frustum plane".into());
			}
			*plane /= length;
		}
		Ok(Self { planes })
	}

	pub fn intersects(&self, bounds: Bounds3) -> bool {
		let min = Vec3::from_array(bounds.min());
		let max = Vec3::from_array(bounds.max());
		self.planes.iter().all(|plane| {
			let normal = plane.truncate();
			let positive = Vec3::new(
				if normal.x >= 0.0 { max.x } else { min.x },
				if normal.y >= 0.0 { max.y } else { min.y },
				if normal.z >= 0.0 { max.z } else { min.z },
			);
			normal.dot(positive) + plane.w >= -1.0e-4
		})
	}
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SceneRenderStats {
	pub total_entities: usize,
	pub visible_entities: usize,
	pub culled_entities: usize,
	pub draw_calls: usize,
	pub triangles: usize,
}
