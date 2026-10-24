use renderer::vulkan::Vertex;

pub struct MeshData {
	vertices: Vec<Vertex>,
	indices: Vec<u32>,
}

impl MeshData {
	pub fn plane() -> Self {
		super::primitives::plane::build()
	}

	pub fn polygon(sides: u32) -> Self {
		super::primitives::polygon::build(sides)
	}

	pub fn sphere() -> Self {
		super::primitives::sphere::build()
	}

	pub fn capsule() -> Self {
		super::primitives::capsule::build()
	}

	pub fn triangle() -> Self {
		super::primitives::triangle::build()
	}
	pub fn new(mut vertices: Vec<Vertex>, indices: Vec<u32>) -> Self {
		if vertices.iter().all(|vertex| vertex.normal != [0.0; 3]) {
			return Self { vertices, indices };
		}
		// Area-weighted smooth normals for missing attributes. Split vertices
		// (as on a cube's six faces) retain their hard edges.
		let mut sums = vec![glam::Vec3::ZERO; vertices.len()];
		for triangle in indices.chunks_exact(3) {
			if triangle.iter().any(|&i| i as usize >= vertices.len()) {
				continue;
			}
			let [a, b, c] = [
				triangle[0] as usize,
				triangle[1] as usize,
				triangle[2] as usize,
			];
			let normal = (glam::Vec3::from_array(vertices[b].position)
				- glam::Vec3::from_array(vertices[a].position))
			.cross(
				glam::Vec3::from_array(vertices[c].position)
					- glam::Vec3::from_array(vertices[a].position),
			);
			for i in [a, b, c] {
				sums[i] += normal;
			}
		}
		for (vertex, sum) in vertices.iter_mut().zip(sums) {
			if vertex.normal == [0.0; 3] {
				vertex.normal = sum.try_normalize().unwrap_or(glam::Vec3::Z).to_array();
			}
		}
		Self { vertices, indices }
	}

	pub fn vertices(&self) -> &[Vertex] {
		&self.vertices
	}

	pub fn indices(&self) -> &[u32] {
		&self.indices
	}

	pub fn cube() -> Self {
		super::primitives::cube::build()
	}
}
