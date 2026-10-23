use std::{
    error::Error,
    fmt::{self, Display, Formatter},
    path::Path,
};

use gltf::mesh::Mode;
use renderer::vulkan::Vertex;

use super::{ImageAsset, ImageAssetError, MaterialData, MeshData};

#[cfg(test)]
#[path = "tests_gltf_loader.rs"]
mod tests;

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

/// Import every primitive of the first mesh. Scene-node transforms are not
/// applied yet; all returned geometry remains in mesh-local coordinates.
pub fn load_first_mesh(path: impl AsRef<Path>) -> Result<Vec<ImportedPrimitive>, GltfLoaderError> {
    let path = path.as_ref();
    let (document, buffers, images) = load_resources(path)?;
    let parts = import_first_mesh(&document, &buffers, &images)?;
    engine_logger::info!("Imported {} parts from {}", parts.len(), path.display());
    Ok(parts)
}

pub(super) fn load_first_mesh_in_root(
    path: &Path,
    root: &Path,
) -> Result<Vec<ImportedPrimitive>, GltfLoaderError> {
    let (document, buffers, images) = load_resources_in_root(path, root)?;
    import_first_mesh(&document, &buffers, &images)
}

const RESOURCE_LIMIT: usize = 64 * 1024 * 1024;
const IMPORT_LIMIT: usize = 256 * 1024 * 1024;

fn rejected(message: impl ToString) -> GltfLoaderError {
    GltfLoaderError::Resource(message.to_string())
}

fn charge(budget: &mut usize, bytes: usize) -> Result<(), GltfLoaderError> {
    *budget = budget
        .checked_sub(bytes)
        .ok_or_else(|| rejected("import exceeds 256 MiB"))?;
    Ok(())
}

fn read_bounded(path: &Path, budget: &mut usize) -> Result<Vec<u8>, GltfLoaderError> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(rejected)?;
    let metadata = file.metadata().map_err(rejected)?;
    if !metadata.is_file() || metadata.len() > RESOURCE_LIMIT.min(*budget) as u64 {
        return Err(rejected(
            "resource is not a regular file or exceeds byte limit",
        ));
    }
    let mut bytes = Vec::new();
    file.take(RESOURCE_LIMIT.min(*budget) as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(rejected)?;
    if bytes.len() > RESOURCE_LIMIT {
        return Err(rejected("resource exceeds 64 MiB"));
    }
    charge(budget, bytes.len())?;
    Ok(bytes)
}

fn read_uri_in_root(
    base: &Path,
    root: &Path,
    uri: &str,
    budget: &mut usize,
) -> Result<Vec<u8>, GltfLoaderError> {
    if let Some(data) = uri.strip_prefix("data:") {
        let (_, encoded) = data
            .split_once(";base64,")
            .ok_or_else(|| rejected("unsupported data URI"))?;
        if encoded.len() > RESOURCE_LIMIT.min(*budget).saturating_add(2) / 3 * 4 {
            return Err(rejected("data URI exceeds byte limit"));
        }
        let bytes = base64::decode(encoded).map_err(rejected)?;
        if bytes.len() > RESOURCE_LIMIT {
            return Err(rejected("resource exceeds 64 MiB"));
        }
        charge(budget, bytes.len())?;
        return Ok(bytes);
    }
    let decoded = urlencoding::decode(uri).map_err(rejected)?;
    // Reject schemes, drive prefixes, UNC paths and platform-dependent separators.
    if decoded.contains([':', '\\', '\0']) || Path::new(decoded.as_ref()).is_absolute() {
        return Err(rejected("only relative asset references are supported"));
    }
    let path = base
        .join(decoded.as_ref())
        .canonicalize()
        .map_err(rejected)?;
    if !path.starts_with(root) {
        return Err(rejected("reference escapes the model directory"));
    }
    read_bounded(&path, budget)
}

type Resources = (
    gltf::Document,
    Vec<gltf::buffer::Data>,
    Vec<gltf::image::Data>,
);

fn load_resources(path: &Path) -> Result<Resources, GltfLoaderError> {
    let path = path.canonicalize().map_err(rejected)?;
    let root = path
        .parent()
        .ok_or_else(|| rejected("missing model directory"))?;
    load_resources_in_root(&path, root)
}

fn load_resources_in_root(path: &Path, root: &Path) -> Result<Resources, GltfLoaderError> {
    let path = path.canonicalize().map_err(rejected)?;
    let root = root.canonicalize().map_err(rejected)?;
    if !path.starts_with(&root) {
        return Err(rejected("model escapes asset root"));
    }
    let base = path
        .parent()
        .ok_or_else(|| rejected("missing model directory"))?;
    let mut budget = IMPORT_LIMIT;
    let bytes = read_bounded(&path, &mut budget)?;
    let gltf::Gltf { document, mut blob } = gltf::Gltf::from_slice(&bytes)?;
    let mut buffers = Vec::new();
    for buffer in document.buffers() {
        if buffer.length() > RESOURCE_LIMIT {
            return Err(rejected("buffer exceeds 64 MiB"));
        }
        let data = match buffer.source() {
            gltf::buffer::Source::Uri(uri) => read_uri_in_root(base, &root, uri, &mut budget)?,
            gltf::buffer::Source::Bin => blob
                .take()
                .ok_or_else(|| rejected("missing binary buffer"))?,
        };
        if data.len() < buffer.length() {
            return Err(rejected("buffer is shorter than declared"));
        }
        buffers.push(gltf::buffer::Data(data));
    }
    let mut images = Vec::new();
    for image in document.images() {
        let external;
        let encoded = match image.source() {
            gltf::image::Source::Uri { uri, .. } => {
                external = read_uri_in_root(base, &root, uri, &mut budget)?;
                external.as_slice()
            }
            gltf::image::Source::View { view, .. } => {
                let end = view
                    .offset()
                    .checked_add(view.length())
                    .ok_or_else(|| rejected("image range overflow"))?;
                buffers
                    .get(view.buffer().index())
                    .and_then(|b| b.0.get(view.offset()..end))
                    .ok_or_else(|| rejected("image view outside buffer"))?
            }
        };
        let mut reader = image::ImageReader::new(std::io::Cursor::new(encoded))
            .with_guessed_format()
            .map_err(rejected)?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(4096);
        limits.max_image_height = Some(4096);
        limits.max_alloc = Some(RESOURCE_LIMIT.min(budget) as u64);
        reader.limits(limits);
        let decoded = reader.decode().map_err(rejected)?;
        let size = (decoded.width() as usize)
            .checked_mul(decoded.height() as usize)
            .and_then(|n| n.checked_mul(4))
            .ok_or_else(|| rejected("image size overflow"))?;
        charge(&mut budget, size)?;
        let rgba = decoded.into_rgba8();
        images.push(gltf::image::Data {
            width: rgba.width(),
            height: rgba.height(),
            format: gltf::image::Format::R8G8B8A8,
            pixels: rgba.into_raw(),
        });
    }
    Ok((document, buffers, images))
}

fn import_first_mesh(
    document: &gltf::Document,
    buffers: &[gltf::buffer::Data],
    images: &[gltf::image::Data],
) -> Result<Vec<ImportedPrimitive>, GltfLoaderError> {
    let mesh = document.meshes().next().ok_or(GltfLoaderError::NoMeshes)?;
    let mut parts = Vec::new();
    for primitive in mesh.primitives() {
        parts.push(import_primitive(mesh.index(), primitive, buffers, images)?);
    }
    if parts.is_empty() {
        return Err(GltfLoaderError::NoPrimitives);
    }
    Ok(parts)
}

fn import_primitive(
    mesh_index: usize,
    primitive: gltf::Primitive<'_>,
    buffers: &[gltf::buffer::Data],
    images: &[gltf::image::Data],
) -> Result<ImportedPrimitive, GltfLoaderError> {
    if primitive.mode() != Mode::Triangles {
        return Err(GltfLoaderError::UnsupportedMode(primitive.mode()));
    }

    let primitive_index = primitive.index();

    let material = primitive.material();
    let pbr = material.pbr_metallic_roughness();

    let base_color_factor = pbr.base_color_factor();
    let base_color_texture = pbr.base_color_texture();

    let tex_coord_set = base_color_texture
        .as_ref()
        .map(|info| info.tex_coord())
        .unwrap_or(0);

    engine_logger::debug!(
        "Primitive {}: color factor {:?}, double-sided {}",
        primitive_index,
        base_color_factor,
        material.double_sided()
    );

    let base_color_image_index = base_color_texture
        .as_ref()
        .map(|info| info.texture().source().index());

    let base_color_image = match base_color_texture {
        Some(info) => {
            let image_index = info.texture().source().index();

            let image = images
                .get(image_index)
                .ok_or(GltfLoaderError::InvalidImageReference(image_index))?;

            let rgba_pixels = match image.format {
                gltf::image::Format::R8G8B8 => {
                    if image.pixels.len() % 3 != 0 {
                        return Err(GltfLoaderError::InvalidRgbByteCount(image.pixels.len()));
                    }

                    let mut rgba = Vec::new();

                    for rgb in image.pixels.chunks_exact(3) {
                        rgba.extend_from_slice(rgb);
                        rgba.push(255);
                    }

                    rgba
                }

                gltf::image::Format::R8G8B8A8 => image.pixels.clone(),

                format => {
                    return Err(GltfLoaderError::UnsupportedImageFormat(format));
                }
            };

            let image_asset = ImageAsset::from_rgba8(image.width, image.height, rgba_pixels)?;

            engine_logger::info!(
                "Primitive {}: image {}, {}x{}, {} RGBA bytes, UV set {}",
                primitive_index,
                image_index,
                image_asset.width(),
                image_asset.height(),
                image_asset.pixels().len(),
                tex_coord_set,
            );

            Some(image_asset)
        }

        None => {
            engine_logger::debug!("Primitive {} uses base-color factor only", primitive_index);

            None
        }
    };

    let reader =
        primitive.reader(|buffer| buffers.get(buffer.index()).map(|data| data.0.as_slice()));

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

    let tex_coords: Vec<[f32; 2]> = match reader.read_tex_coords(tex_coord_set) {
        Some(values) => values.into_f32().collect(),

        None if base_color_image.is_some() => {
            return Err(GltfLoaderError::MissingTexCoords(tex_coord_set));
        }

        None => vec![[0.0, 0.0]; positions.len()],
    };

    if positions.len() != tex_coords.len() {
        return Err(GltfLoaderError::TexCoordCountMismatch {
            positions: positions.len(),
            tex_coords: tex_coords.len(),
        });
    }

    if indices.len() % 3 != 0 {
        return Err(GltfLoaderError::InvalidTriangleIndexCount(indices.len()));
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

	let normals = reader
		.read_normals()
		.map(|values| values.collect::<Vec<_>>());
	if let Some(normals) = &normals
		&& (normals.len() != positions.len()
		|| normals.iter().any(|normal| {
		let normal = glam::Vec3::from_array(*normal);
		!normal.is_finite() || normal.length_squared() < 1.0e-12
	}))
	{
		return Err(rejected(
			"NORMAL must contain one finite nonzero vector per position",
		));
	}
    let vertices: Vec<Vertex> = positions
        .into_iter()
        .zip(tex_coords)
		.enumerate()
		.map(|(index, (position, uv))| {
			Vertex::new(position, uv).with_normal(normals.as_ref().map_or([0.0; 3], |normals| {
				glam::Vec3::from_array(normals[index])
					.normalize()
					.to_array()
			}))
		})
        .collect();

    engine_logger::info!(
        "Imported primitive {}: {} vertices, {} indices, {} triangles",
        primitive_index,
        vertices.len(),
        indices.len(),
        indices.len() / 3
    );

    Ok(ImportedPrimitive {
        mesh_index,
        primitive_index,
        base_color_image_index,
        mesh: MeshData::new(vertices, indices),
        material: MaterialData {
            base_color_image,
            base_color_factor,
            double_sided: material.double_sided(),
        },
    })
}
