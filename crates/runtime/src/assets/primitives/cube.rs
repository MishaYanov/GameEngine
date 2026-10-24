use super::super::MeshData;
use renderer::vulkan::Vertex;

pub(in crate::assets) fn build() -> MeshData {
	let vertices = vec![
		/*
		 * Front +Z
		 */
		Vertex::new([-0.5, -0.5, 0.5], [0.0, 0.0]),
		Vertex::new([0.5, -0.5, 0.5], [1.0, 0.0]),
		Vertex::new([0.5, 0.5, 0.5], [1.0, 1.0]),
		Vertex::new([-0.5, 0.5, 0.5], [0.0, 1.0]),
		/*
		 * Back -Z
		 */
		Vertex::new([0.5, -0.5, -0.5], [0.0, 0.0]),
		Vertex::new([-0.5, -0.5, -0.5], [1.0, 0.0]),
		Vertex::new([-0.5, 0.5, -0.5], [1.0, 1.0]),
		Vertex::new([0.5, 0.5, -0.5], [0.0, 1.0]),
		/*
		 * Left -X
		 */
		Vertex::new([-0.5, -0.5, -0.5], [0.0, 0.0]),
		Vertex::new([-0.5, -0.5, 0.5], [1.0, 0.0]),
		Vertex::new([-0.5, 0.5, 0.5], [1.0, 1.0]),
		Vertex::new([-0.5, 0.5, -0.5], [0.0, 1.0]),
		/*
		 * Right +X
		 */
		Vertex::new([0.5, -0.5, 0.5], [0.0, 0.0]),
		Vertex::new([0.5, -0.5, -0.5], [1.0, 0.0]),
		Vertex::new([0.5, 0.5, -0.5], [1.0, 1.0]),
		Vertex::new([0.5, 0.5, 0.5], [0.0, 1.0]),
		/*
		 * Top +Y
		 */
		Vertex::new([-0.5, 0.5, 0.5], [0.0, 0.0]),
		Vertex::new([0.5, 0.5, 0.5], [1.0, 0.0]),
		Vertex::new([0.5, 0.5, -0.5], [1.0, 1.0]),
		Vertex::new([-0.5, 0.5, -0.5], [0.0, 1.0]),
		/*
		 * Bottom -Y
		 */
		Vertex::new([-0.5, -0.5, -0.5], [0.0, 0.0]),
		Vertex::new([0.5, -0.5, -0.5], [1.0, 0.0]),
		Vertex::new([0.5, -0.5, 0.5], [1.0, 1.0]),
		Vertex::new([-0.5, -0.5, 0.5], [0.0, 1.0]),
	];

	let indices = vec![
		0, 1, 2, 2, 3, 0, 4, 5, 6, 6, 7, 4, 8, 9, 10, 10, 11, 8, 12, 13, 14, 14, 15, 12, 16, 17,
		18, 18, 19, 16, 20, 21, 22, 22, 23, 20,
	];

	MeshData::new(vertices, indices)
}
