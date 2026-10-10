use std::{
    error::Error,
    fmt::{self, Display, Formatter},
    path::Path,
};

use gltf::mesh::Mode;
use renderer::vulkan::Vertex;

use super::{ImageAsset, ImageAssetError, MeshData};

pub struct ImportedPrimitive {
    pub mesh: MeshData,
    pub base_color_image: Option<ImageAsset>,
    pub base_color_factor: [f32; 4],
    pub double_sided: bool
}

#[derive(Debug)]
pub enum GltfLoaderError {
    Import(gltf::Error),
    Image(ImageAssetError),
    NoMeshes,
    NoPrimitives,
    UnsupportedMode(Mode),
    MissingPositions,
    MissingIndices,
    MissingTexCoords(u32),
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
    InvalidImageReference(usize),
    InvalidRgbByteCount(usize),
    UnsupportedImageFormat(gltf::image::Format),
}

impl Display for GltfLoaderError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
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

/// Imports the first mesh's first indexed triangle primitive.
///
/// Returns owned CPU mesh data and optional base-color image data.
/// Node transforms, additional primitives, and full PBR rendering
/// are not handled here.
pub fn load_first_primitive(
    path: impl AsRef<Path>,
) -> Result<ImportedPrimitive, GltfLoaderError> {
    let path = path.as_ref();

    engine_logger::debug!("Importing glTF file: {}", path.display());

    let (document, buffers, images) = gltf::import(path)?;

    // Select the first mesh and its first primitive.
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

    engine_logger::debug!(
        "Selected mesh: {}, primitive: {}, mode: {:?}",
        mesh.index(),
        primitive.index(),
        primitive.mode()
    );

    // Read the material's base-color properties.
    let material = primitive.material();
    let material_color_scheme = material.pbr_metallic_roughness();
    let base_color_factor = material_color_scheme.base_color_factor();
    let base_color_texture = material_color_scheme.base_color_texture();

    // Select the UV set requested by the base-color texture.
    let tex_coord_set = base_color_texture
        .as_ref()
        .map(|info| info.tex_coord())
        .unwrap_or(0);

    engine_logger::info!("Color factor: {:?}", base_color_factor);

    // Convert the imported image into owned RGBA8 pixels.
    let base_color_image = match base_color_texture {
        Some(info) => {
            let image_index = info.texture().source().index();

            let image = images
                .get(image_index)
                .ok_or(GltfLoaderError::InvalidImageReference(image_index))?;

            let rgba_pixels = match image.format {
                gltf::image::Format::R8G8B8 => {
                    if image.pixels.len() % 3 != 0 {
                        return Err(GltfLoaderError::InvalidRgbByteCount(
                            image.pixels.len(),
                        ));
                    }

                    let mut rgba = Vec::new();

                    for rgb in image.pixels.chunks_exact(3) {
                        rgba.extend_from_slice(rgb);
                        rgba.push(255);
                    }

                    rgba
                }

                gltf::image::Format::R8G8B8A8 => {
                    image.pixels.clone()
                }

                format => {
                    return Err(GltfLoaderError::UnsupportedImageFormat(format));
                }
            };

            engine_logger::info!(
                "Base-color image: {}, size: {}x{}, format: {:?}, UV set: {}",
                image_index,
                image.width,
                image.height,
                image.format,
                tex_coord_set
            );

            // Validate dimensions and byte count without flipping rows.
            let image_asset = ImageAsset::from_rgba8(
                image.width,
                image.height,
                rgba_pixels,
            )?;

            engine_logger::info!(
                "Converted RGBA image: {}x{}, {} bytes",
                image_asset.width(),
                image_asset.height(),
                image_asset.pixels().len()
            );

            Some(image_asset)
        }

        None => {
            engine_logger::info!("Material uses base-color factor only");
            None
        }
    };

    // Connect glTF buffer descriptions to their loaded bytes.
    let reader = primitive.reader(|buffer| {
        buffers
            .get(buffer.index())
            .map(|data| data.0.as_slice())
    });

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

    let tex_coords: Vec<[f32; 2]> =
        match reader.read_tex_coords(tex_coord_set) {
            Some(values) => values.into_f32().collect(),

            // A textured material needs the UV set it references.
            None if base_color_image.is_some() => {
                return Err(GltfLoaderError::MissingTexCoords(tex_coord_set));
            }

            // Without a texture, zero UVs are sufficient.
            None => vec![[0.0, 0.0]; positions.len()],
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

    engine_logger::info!(
        "Loaded glTF primitive: {}, vertices: {}, indices: {}, triangles: {}",
        path.display(),
        vertices.len(),
        indices.len(),
        indices.len() / 3
    );

    Ok(ImportedPrimitive {
        mesh: MeshData::new(vertices, indices),
        base_color_image,
        base_color_factor,
        double_sided: true,
    })
}