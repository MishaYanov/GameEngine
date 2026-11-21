use std::{
	error::Error,
	fmt::{self, Display, Formatter},
	path::Path,
};

use gltf::mesh::Mode;

use super::{ImageAssetError, MaterialData, MeshData};

mod mesh;
mod resources;

pub use super::scene_import::{ImportedNode, ImportedScene};
pub(super) use mesh::preflight_meshes;
pub use resources::load_first_mesh;
pub(super) use resources::{Resources, load_resources_in_root};

pub(super) const IMPORT_LIMIT: usize = 256 * 1024 * 1024;

pub struct ImportedPrimitive {
	pub mesh: MeshData,
	pub material: MaterialData,
	pub primitive_index: usize,
	pub mesh_index: usize,
	pub base_color_image_index: Option<usize>,
}

#[derive(Debug)]
pub enum GltfLoaderError {
	Resource(String),
	Import(gltf::Error),
	Image(ImageAssetError),
	NoMeshes,
	NoPrimitives,
	UnsupportedMode(Mode),
	MissingPositions,
	MissingIndices,
	MissingTexCoords(u32),
	EmptyGeometry,
	TexCoordCountMismatch { positions: usize, tex_coords: usize },
	InvalidTriangleIndexCount(usize),
	IndexOutOfBounds { index: u32, vertex_count: usize },
	InvalidImageReference(usize),
	InvalidRgbByteCount(usize),
	UnsupportedImageFormat(gltf::image::Format),
}

impl Display for GltfLoaderError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
		match self {
			Self::Resource(message) => write!(formatter, "glTF resource rejected: {message}"),
			Self::Import(error) => {
				write!(formatter, "glTF import failed: {error}")
			}
			Self::Image(error) => {
				write!(formatter, "glTF image conversion failed: {error}")
			}
			Self::NoMeshes => {
				write!(formatter, "glTF document contains no meshes")
			}
			Self::NoPrimitives => {
				write!(formatter, "selected mesh contains no primitives")
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
			Self::MissingTexCoords(set) => {
				write!(
					formatter,
					"material requires missing texture-coordinate set {set}"
				)
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
			Self::InvalidImageReference(index) => {
				write!(formatter, "invalid image reference: {index}")
			}
			Self::InvalidRgbByteCount(bytes) => {
				write!(
					formatter,
					"invalid RGB byte count: {bytes} (must be divisible by three)"
				)
			}
			Self::UnsupportedImageFormat(format) => {
				write!(formatter, "unsupported image format: {format:?}")
			}
		}
	}
}

impl Error for GltfLoaderError {
	fn source(&self) -> Option<&(dyn Error + 'static)> {
		match self {
			Self::Import(error) => Some(error),
			Self::Image(error) => Some(error),
			_ => None,
		}
	}
}

impl From<gltf::Error> for GltfLoaderError {
	fn from(error: gltf::Error) -> Self {
		Self::Import(error)
	}
}

impl From<ImageAssetError> for GltfLoaderError {
	fn from(error: ImageAssetError) -> Self {
		Self::Image(error)
	}
}

/// Read the default scene (or the first scene when no default is declared).
/// Geometry stays mesh-local; node transforms are returned separately.
pub fn load_scene(path: impl AsRef<Path>) -> Result<ImportedScene, GltfLoaderError> {
	let path = path.as_ref().canonicalize().map_err(rejected)?;
	let root = path
		.parent()
		.ok_or_else(|| rejected("missing model directory"))?;
	load_scene_in_root(&path, root)
}

pub(super) fn load_scene_in_root(
	path: &Path,
	root: &Path,
) -> Result<ImportedScene, GltfLoaderError> {
	super::metadata_import::load_scene_metadata(path, root, None, None)
}

fn rejected(message: impl ToString) -> GltfLoaderError {
	GltfLoaderError::Resource(message.to_string())
}

fn charge(budget: &mut usize, bytes: usize) -> Result<(), GltfLoaderError> {
	*budget = budget
		.checked_sub(bytes)
		.ok_or_else(|| rejected("import exceeds 256 MiB"))?;
	Ok(())
}

#[cfg(test)]
#[path = "../../tests/unit/assets/gltf_loader.rs"]
mod tests;
