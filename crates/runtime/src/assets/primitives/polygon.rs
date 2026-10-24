use super::super::MeshData;
use renderer::vulkan::Vertex;

pub(in crate::assets) fn build(sides: u32) -> MeshData {
	assert!(sides >= 3, "a polygon needs at least three sides");
	let mut vertices = vec![Vertex::new([0.0; 3], [0.5; 2])];
	let mut indices = Vec::new();
	for side in 0..sides {
		let angle = std::f32::consts::TAU * side as f32 / sides as f32;
		let (y, x) = angle.sin_cos();
		vertices.push(Vertex::new(
			[0.5 * x, 0.5 * y, 0.0],
			[0.5 + 0.5 * x, 0.5 + 0.5 * y],
		));
		indices.extend_from_slice(&[0, side + 1, (side + 1) % sides + 1]);
	}
	MeshData::new(vertices, indices)
}
