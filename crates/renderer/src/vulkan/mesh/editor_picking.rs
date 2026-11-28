//! CPU surface picking for static meshes uploaded by the editor.

use std::sync::Arc;

use glam::{DVec3, Vec3};

use super::{EditorTopologyBudget, editor_geometry::TopologyReservation};
use crate::vulkan::Vertex;

const MAX_TRIANGLES: usize = 512 * 1024;
const LEAF_TRIANGLES: usize = 8;

#[derive(Debug)]
struct Node {
	bounds: Bounds,
	contents: Contents,
}

#[derive(Debug)]
enum Contents {
	Leaf { start: u32, end: u32 },
	Branch { left: u32, right: u32 },
}

#[derive(Clone, Copy, Debug)]
struct Bounds {
	min: Vec3,
	max: Vec3,
}

impl Bounds {
	const EMPTY: Self = Self {
		min: Vec3::splat(f32::INFINITY),
		max: Vec3::splat(f32::NEG_INFINITY),
	};

	fn include(&mut self, point: Vec3) {
		self.min = self.min.min(point);
		self.max = self.max.max(point);
	}

	fn entry(self, ray: Ray, limit: f64) -> Option<f64> {
		let mut entry = 0.0_f64;
		let mut exit = limit;
		for axis in 0..3 {
			let origin = ray.origin[axis];
			let direction = ray.direction[axis];
			let min = f64::from(self.min[axis]);
			let max = f64::from(self.max[axis]);
			if direction == 0.0 {
				if origin < min || origin > max {
					return None;
				}
				continue;
			}
			let near = (min - origin) / direction;
			let far = (max - origin) / direction;
			entry = entry.max(near.min(far));
			exit = exit.min(near.max(far));
			if entry > exit {
				return None;
			}
		}
		Some(entry)
	}
}

#[derive(Clone, Copy)]
struct Ray {
	origin: DVec3,
	direction: DVec3,
}

impl Ray {
	fn triangle(self, [a, b, c]: [Vec3; 3], limit: f64) -> Option<f64> {
		let a = a.as_dvec3();
		let edge_ab = b.as_dvec3() - a;
		let edge_ac = c.as_dvec3() - a;
		let perpendicular = self.direction.cross(edge_ac);
		let determinant = edge_ab.dot(perpendicular);
		if determinant == 0.0 {
			return None;
		}
		let offset = self.origin - a;
		let u = offset.dot(perpendicular) / determinant;
		if !(0.0..=1.0).contains(&u) {
			return None;
		}
		let transverse = offset.cross(edge_ab);
		let v = self.direction.dot(transverse) / determinant;
		if !(0.0..=1.0 - u).contains(&v) {
			return None;
		}
		let distance = edge_ac.dot(transverse) / determinant;
		(0.0..=limit).contains(&distance).then_some(distance)
	}
}

/// Shared CPU representation of a static editor mesh for nearest-triangle picking.
#[derive(Debug)]
pub struct EditorPickGeometry {
	positions: Vec<Vec3>,
	triangles: Vec<[u32; 3]>,
	nodes: Vec<Node>,
	_reservation: TopologyReservation,
}

impl EditorPickGeometry {
	pub(super) fn retain(
		budget: &EditorTopologyBudget,
		vertices: &[Vertex],
		indices: &[u32],
	) -> Option<Arc<Self>> {
		let faces = indices.chunks_exact(3);
		let triangle_count = faces.len();
		if triangle_count == 0 || triangle_count > MAX_TRIANGLES || !faces.remainder().is_empty() {
			return None;
		}

		// Median splits have fewer than twice as many nodes as leaves. Reserve enough for
		// the smallest leaves produced by repeatedly halving a range larger than eight.
		let node_capacity = 2 * triangle_count.div_ceil(LEAF_TRIANGLES / 2);
		let bytes = vertices
			.len()
			.checked_mul(size_of::<Vec3>())?
			.checked_add(size_of_val(indices) + node_capacity * size_of::<Node>())?;
		let mut positions = Vec::new();
		let mut triangles = Vec::new();
		let mut nodes = Vec::new();
		let reservation = budget.reserve_picking(bytes)?;
		positions.try_reserve_exact(vertices.len()).ok()?;
		triangles.try_reserve_exact(triangle_count).ok()?;
		nodes.try_reserve_exact(node_capacity).ok()?;
		for vertex in vertices {
			let position = Vec3::from_array(vertex.position);
			if !position.is_finite() {
				return None;
			}
			positions.push(position);
		}
		for face in faces {
			if face.iter().any(|&index| index as usize >= positions.len()) {
				return None;
			}
			triangles.push([face[0], face[1], face[2]]);
		}
		Self::build(&positions, &mut triangles, 0, &mut nodes);
		Some(Arc::new(Self {
			positions,
			triangles,
			nodes,
			_reservation: reservation,
		}))
	}

	/// Find the nearest two-sided triangle on the ray segment in mesh-local space.
	/// The returned value is a parameter along the given direction. Leave the
	/// direction unnormalized after an inverse transform to preserve world distance.
	pub fn ray_distance(&self, origin: Vec3, direction: Vec3, max_distance: f32) -> Option<f32> {
		if !origin.is_finite()
			|| !direction.is_finite()
			|| direction == Vec3::ZERO
			|| !max_distance.is_finite()
			|| max_distance < 0.0
		{
			return None;
		}
		let ray = Ray {
			origin: origin.as_dvec3(),
			direction: direction.as_dvec3(),
		};
		let limit = f64::from(max_distance);
		self.nodes.first()?.bounds.entry(ray, limit)?;
		self.hit_node(0, ray, limit).map(|distance| distance as f32)
	}

	fn build(
		positions: &[Vec3],
		triangles: &mut [[u32; 3]],
		start: usize,
		nodes: &mut Vec<Node>,
	) -> usize {
		let mut bounds = Bounds::EMPTY;
		let mut centers = Bounds::EMPTY;
		for triangle in triangles.iter() {
			let points = points(positions, *triangle);
			for point in points {
				bounds.include(point);
			}
			centers.include(centroid(points));
		}
		let index = nodes.len();
		nodes.push(Node {
			bounds,
			contents: Contents::Leaf {
				start: start as u32,
				end: (start + triangles.len()) as u32,
			},
		});
		if triangles.len() > LEAF_TRIANGLES {
			let span = centers.max - centers.min;
			let axis = if span.x >= span.y && span.x >= span.z {
				0
			} else if span.y >= span.z {
				1
			} else {
				2
			};
			let midpoint = triangles.len() / 2;
			triangles.select_nth_unstable_by(midpoint, |a, b| {
				centroid(points(positions, *a))[axis]
					.total_cmp(&centroid(points(positions, *b))[axis])
			});
			let (left_triangles, right_triangles) = triangles.split_at_mut(midpoint);
			let left = Self::build(positions, left_triangles, start, nodes);
			let right = Self::build(positions, right_triangles, start + midpoint, nodes);
			nodes[index].contents = Contents::Branch {
				left: left as u32,
				right: right as u32,
			};
		}
		index
	}

	fn hit_node(&self, index: usize, ray: Ray, limit: f64) -> Option<f64> {
		match self.nodes[index].contents {
			Contents::Leaf { start, end } => {
				let mut closest = None;
				for &triangle in &self.triangles[start as usize..end as usize] {
					if let Some(distance) =
						ray.triangle(points(&self.positions, triangle), closest.unwrap_or(limit))
					{
						closest = Some(distance);
					}
				}
				closest
			}
			Contents::Branch { left, right } => {
				let (left, right) = (left as usize, right as usize);
				let left_entry = self.nodes[left].bounds.entry(ray, limit);
				let right_entry = self.nodes[right].bounds.entry(ray, limit);
				match (left_entry, right_entry) {
					(None, None) => None,
					(Some(_), None) => self.hit_node(left, ray, limit),
					(None, Some(_)) => self.hit_node(right, ray, limit),
					(Some(a), Some(b)) => {
						let (first, second, second_entry) = if a <= b {
							(left, right, b)
						} else {
							(right, left, a)
						};
						let closest = self.hit_node(first, ray, limit);
						let limit = closest.unwrap_or(limit);
						if second_entry > limit {
							return closest;
						}
						self.hit_node(second, ray, limit).or(closest)
					}
				}
			}
		}
	}
}

fn points(positions: &[Vec3], indices: [u32; 3]) -> [Vec3; 3] {
	indices.map(|index| positions[index as usize])
}

fn centroid([a, b, c]: [Vec3; 3]) -> Vec3 {
	a / 3.0 + b / 3.0 + c / 3.0
}

#[cfg(test)]
mod tests {
	use super::*;

	fn triangle(x: f32, z: f32) -> [Vertex; 3] {
		[[x, 0.0, z], [x + 2.0, 0.0, z], [x, 2.0, z]]
			.map(|position| Vertex::new(position, [0.0; 2]).with_normal([0.0, 0.0, 1.0]))
	}

	#[test]
	fn surface_picking_rejects_empty_space_inside_the_bounds() {
		let geometry = EditorPickGeometry::retain(
			&EditorTopologyBudget::default(),
			&triangle(0.0, 3.0),
			&[0, 1, 2],
		)
			.unwrap();

		assert_eq!(
			geometry.ray_distance(Vec3::new(0.5, 0.5, 0.0), Vec3::Z, 4.0),
			Some(3.0)
		);
		assert_eq!(
			geometry.ray_distance(Vec3::new(1.5, 1.5, 0.0), Vec3::Z, 4.0),
			None
		);
		assert_eq!(
			geometry.ray_distance(Vec3::new(0.5, 0.5, 0.0), Vec3::Z, 2.9),
			None
		);
		assert_eq!(
			geometry.ray_distance(Vec3::new(0.5, 0.5, 3.5), -Vec3::Z * 0.5, 4.0),
			Some(1.0)
		);
	}

	#[test]
	fn finds_the_closest_triangle_across_bvh_branches() {
		let vertices: Vec<_> = (0..24)
			.rev()
			.flat_map(|index| triangle(0.0, index as f32 + 1.0))
			.collect();
		let indices: Vec<_> = (0..vertices.len() as u32).collect();
		let geometry =
			EditorPickGeometry::retain(&EditorTopologyBudget::default(), &vertices, &indices)
				.unwrap();

		assert!(geometry.nodes.len() > 1);
		assert_eq!(
			geometry.ray_distance(Vec3::new(1.0, 0.5, 0.0), Vec3::Z, 100.0),
			Some(1.0)
		);
		assert_eq!(
			geometry.ray_distance(Vec3::new(1.0, 0.5, 25.0), -Vec3::Z * 0.25, 10.0),
			Some(4.0)
		);
		assert_eq!(geometry.ray_distance(Vec3::ZERO, Vec3::ZERO, 100.0), None);
	}

	#[test]
	fn rejects_incomplete_or_out_of_range_triangles() {
		let budget = EditorTopologyBudget::default();
		let vertices = triangle(0.0, 3.0);
		assert!(EditorPickGeometry::retain(&budget, &vertices, &[0, 1, 2, 0]).is_none());
		assert!(EditorPickGeometry::retain(&budget, &vertices, &[0, 1, 3]).is_none());
	}
}
