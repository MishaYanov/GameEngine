use std::{
    error::Error,
    fmt::{self, Display, Formatter},
    path::Path,
};

use gltf::mesh::Mode;
use renderer::vulkan::Vertex;

use super::{ImageAsset, ImageAssetError, MaterialData, MeshData};

#[cfg(test)]
mod tests {
    use super::*;

    const MODEL: &str = r#"{
        "asset":{"version":"2.0"},
        "buffers":[{"byteLength":42}],
        "bufferViews":[
            {"buffer":0,"byteOffset":0,"byteLength":36},
            {"buffer":0,"byteOffset":36,"byteLength":6}
        ],
        "accessors":[
            {"bufferView":0,"componentType":5126,"count":3,"type":"VEC3",
             "min":[0,0,0],"max":[1,1,0]},
            {"bufferView":1,"componentType":5123,"count":3,"type":"SCALAR"}
        ],
        "materials":[
            {"doubleSided":true,"pbrMetallicRoughness":{"baseColorFactor":[1,0,0,1]}},
            {"pbrMetallicRoughness":{"baseColorFactor":[0,1,0,1]}}
        ],
        "meshes":[{"primitives":[
            {"attributes":{"POSITION":0},"indices":1,"material":0,"mode":4},
            {"attributes":{"POSITION":0},"indices":1,"material":1,"mode":4}
        ]}]
    }"#;

    fn geometry() -> Vec<gltf::buffer::Data> {
        let mut bytes = Vec::new();
        for value in [0.0_f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for index in [0_u16, 1, 2] {
            bytes.extend_from_slice(&index.to_le_bytes());
        }
        vec![gltf::buffer::Data(bytes)]
    }

    #[test]
    fn imports_all_parts_with_their_own_materials() {
        let document = gltf::Gltf::from_slice(MODEL.as_bytes()).unwrap().document;
        let parts = import_first_mesh(&document, &geometry(), &[]).unwrap();
        assert_eq!(parts.len(), 2);
        for (index, part) in parts.iter().enumerate() {
            assert_eq!(part.mesh_index, 0);
            assert_eq!(part.primitive_index, index);
            assert_eq!(part.mesh.vertices().len(), 3);
            assert_eq!(part.mesh.indices(), &[0, 1, 2]);
            assert_eq!(part.base_color_image_index, None);
            assert!(part.material.base_color_image.is_none());
        }
        assert!(parts[0].material.double_sided);
        assert!(!parts[1].material.double_sided);
        assert_eq!(parts[0].material.base_color_factor, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(parts[1].material.base_color_factor, [0.0, 1.0, 0.0, 1.0]);
    }

    #[test]
    fn rejects_unsupported_parts_instead_of_silently_skipping_them() {
        let model = MODEL.replacen("\"mode\":4", "\"mode\":1", 1);
        let document = gltf::Gltf::from_slice(model.as_bytes()).unwrap().document;
        assert!(matches!(
            import_first_mesh(&document, &geometry(), &[]),
            Err(GltfLoaderError::UnsupportedMode(Mode::Lines))
        ));
    }
}

pub struct ImportedPrimitive {
    pub mesh: MeshData,
    pub(crate) material: MaterialData,
    pub primitive_index: usize,
    pub mesh_index: usize,
    pub base_color_image_index: Option<usize>,
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
    let (document, buffers, images) = gltf::import(path)?;
    let parts = import_first_mesh(&document, &buffers, &images)?;
    engine_logger::info!("Imported {} parts from {}", parts.len(), path.display());
    Ok(parts)
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

    let vertices: Vec<Vertex> = positions
        .into_iter()
        .zip(tex_coords)
        .map(|(position, uv)| Vertex::new(position, uv))
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
