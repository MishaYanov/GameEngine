use super::super::MeshData;
use renderer::vulkan::Vertex;

pub(in crate::assets) fn build() -> MeshData {
	MeshData::new(
		vec![
			Vertex::new([-0.7, -0.6, 0.0], [0.0, 1.0]),
			Vertex::new([0.7, -0.6, 0.0], [1.0, 1.0]),
			Vertex::new([0.0, 0.7, 0.0], [0.5, 0.0]),
		],
		vec![0, 1, 2, 2, 1, 0],
	)
}
