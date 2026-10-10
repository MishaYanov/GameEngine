use std::{
	error::Error,
	fmt::{self, Display, Formatter},
	path::Path,
};

use gltf::mesh::Mode;
use renderer::vulkan::Vertex;

use super::MeshData;

#[derive(Debug)]
pub enum GltfLoaderError {
	Import(gltf::Error),
	NoMeshes,
	NoPrimitives,
	UnsupportedMode(Mode),
	MissingPositions,
	MissingIndices,
	EmptyGeometry,
	TexCoordCountMismatch {
		positions: usize,
		tex_coords: usize,
	},
	InvalidTriangleIndexCount(usize),
	IndexOutOfBounds {
		index: u32,
		vertex_count: usize,
	},
}

impl Display for GltfLoaderError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
		match self {
			Self::Import(error) => {
				write!(formatter, "glTF import failed: {error}")
			}
			Self::NoMeshes => {
				write!(formatter, "glTF document contains no meshes")
			}
			Self::NoPrimitives => {
				write!(formatter, "first mesh contains no primitives")
			}
			Self::UnsupportedMode(mode) => {
				write!(formatter, "unsupported primitive mode: {mode:?}")
			}
			Self::MissingPositions => {
				write!(formatter, "primitive has no readable positions")
			}
			Self::MissingIndices => {
				write!(formatter, "primitive has no readable indices")
			}
			Self::EmptyGeometry => {
				write!(formatter, "primitive contains empty geometry")
			}
			Self::TexCoordCountMismatch {
				positions,
				tex_coords,
			} => {
				write!(
					formatter,
					"position count {positions} differs from UV count {tex_coords}"
				)
			}
			Self::InvalidTriangleIndexCount(count) => {
				write!(
					formatter,
					"triangle index count {count} is not divisible by three"
				)
			}
			Self::IndexOutOfBounds {
				index,
				vertex_count,
			} => {
				write!(
					formatter,
					"index {index} is outside the mesh's {vertex_count} vertices"
				)
			}
		}
	}
}

impl Error for GltfLoaderError {
	fn source(&self) -> Option<&(dyn Error + 'static)> {
		match self {
			Self::Import(error) => Some(error),
			_ => None,
		}
	}
}

impl From<gltf::Error> for GltfLoaderError {
	fn from(error: gltf::Error) -> Self {
		Self::Import(error)
	}
}

pub fn load_first_primitive(
	path: impl AsRef<Path>,
) -> Result<MeshData, GltfLoaderError> {
	let path = path.as_ref();

	engine_logger::debug!("Importing glTF file: {}", path.display());

	let (document, buffers, _images) = gltf::import(path)?;

	let mesh = document
		.meshes()
		.next()
		.ok_or(GltfLoaderError::NoMeshes)?;

	let primitive = mesh
		.primitives()
		.next()
		.ok_or(GltfLoaderError::NoPrimitives)?;

	if primitive.mode() != Mode::Triangles {
		return Err(GltfLoaderError::UnsupportedMode(primitive.mode()));
	}

	engine_logger::debug!("Selected mesh primitive. index: {}, primitive{}, mode: {:?}.",
		mesh.index(),
        primitive.index(),
        primitive.mode()
	);

	let reader = primitive.reader(|buffer| {
		buffers
			.get(buffer.index())
			.map(|data| data.0.as_slice())
	});

	// 4. Collect owned CPU data from the reader's iterators.
	let positions: Vec<[f32; 3]> = reader
		.read_positions()
		.ok_or(GltfLoaderError::MissingPositions)?
		.collect();

	let indices: Vec<u32> = reader
		.read_indices()
		.ok_or(GltfLoaderError::MissingIndices)?
		.into_u32()
		.collect();

	if positions.is_empty() || indices.is_empty() {
		return Err(GltfLoaderError::EmptyGeometry);
	}

	let tex_coords: Vec<[f32; 2]> = match reader.read_tex_coords(0) {
		Some(values) => values.into_f32().collect(),
		None => {
			engine_logger::warn!("No TEXCOORD_0; using zero UVs, {}", path.display());

			vec![[0.0, 0.0]; positions.len()]
		}
	};

	if positions.len() != tex_coords.len() {
		return Err(GltfLoaderError::TexCoordCountMismatch {
			positions: positions.len(),
			tex_coords: tex_coords.len(),
		});
	}

	if indices.len() % 3 != 0 {
		return Err(GltfLoaderError::InvalidTriangleIndexCount(
			indices.len(),
		));
	}

	for &index in &indices {
		let is_in_bounds = usize::try_from(index)
			.map(|index| index < positions.len())
			.unwrap_or(false);

		if !is_in_bounds {
			return Err(GltfLoaderError::IndexOutOfBounds {
				index,
				vertex_count: positions.len(),
			});
		}
	}

	let vertices: Vec<Vertex> = positions
		.into_iter()
		.zip(tex_coords)
		.map(|(position, uv)| Vertex::new(position, uv))
		.collect();

	engine_logger::info!("Loaded glTF mesh primitive. path: {}, vertex_count: {}, index_count: {}, triangle_count: {}",
		path.display(),
		vertices.len(),
		indices.len(),
		indices.len() / 3
	);

	Ok(MeshData::new(vertices, indices))
}