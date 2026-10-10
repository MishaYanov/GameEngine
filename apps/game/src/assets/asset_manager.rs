use std::{
    collections::HashMap,
    error::Error,
    fmt::{Display, Formatter},
    io,
    path::{Path, PathBuf},
    sync::Arc,
};

use renderer::vulkan::{
    VulkanDevice, VulkanInstance, VulkanMaterial, VulkanMaterialLayout, VulkanMesh,
    VulkanMeshError, VulkanTexture, VulkanTextureError,
};

use super::{ImageAsset, ImageAssetError, MaterialData, MeshData};
use super::{
    Model,
    gltf_loader::{GltfLoaderError, load_first_mesh},
    model::ModelPart,
};

pub struct AssetManager {
    root: PathBuf,
    textures: HashMap<PathBuf, Arc<VulkanTexture>>,
    meshes: HashMap<String, Arc<VulkanMesh>>,
    image_textures: HashMap<String, Arc<VulkanTexture>>,
    material_layout: Arc<VulkanMaterialLayout>,
}

#[derive(Debug)]
pub enum AssetManagerError {
    Io(io::Error),
    Image(ImageAssetError),
    Texture(VulkanTextureError),
    Mesh(VulkanMeshError),
    Gltf(GltfLoaderError),
    OutsideAssetRoot { path: PathBuf, root: PathBuf },
}

impl Display for AssetManagerError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => {
                write!(formatter, "asset I/O error: {error}",)
            }

            Self::Image(error) => {
                write!(formatter, "image asset error: {error}",)
            }

            Self::Texture(error) => {
                write!(formatter, "Vulkan texture asset error: {error}",)
            }

            Self::Mesh(error) => {
                write!(formatter, "Vulkan mesh asset error: {error}",)
            }

            Self::Gltf(error) => write!(formatter, "model import error: {error}"),
            Self::OutsideAssetRoot { path, root } => {
                write!(
                    formatter,
                    "asset path '{}' is outside asset root '{}'",
                    path.display(),
                    root.display(),
                )
            }
        }
    }
}

impl Error for AssetManagerError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Image(error) => Some(error),
            Self::Texture(error) => Some(error),
            Self::Mesh(error) => Some(error),
            Self::Gltf(error) => Some(error),
            Self::OutsideAssetRoot { .. } => None,
        }
    }
}

impl From<GltfLoaderError> for AssetManagerError {
    fn from(value: GltfLoaderError) -> Self {
        Self::Gltf(value)
    }
}

impl From<io::Error> for AssetManagerError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<ImageAssetError> for AssetManagerError {
    fn from(value: ImageAssetError) -> Self {
        Self::Image(value)
    }
}

impl From<VulkanTextureError> for AssetManagerError {
    fn from(value: VulkanTextureError) -> Self {
        Self::Texture(value)
    }
}

impl From<VulkanMeshError> for AssetManagerError {
    fn from(value: VulkanMeshError) -> Self {
        Self::Mesh(value)
    }
}

impl AssetManager {
    /// Import the first mesh and reuse any already-uploaded GPU resources.
    pub fn load_model(
        &mut self,
        instance: &VulkanInstance,
        device: &VulkanDevice,
        path: impl AsRef<Path>,
    ) -> Result<Model, AssetManagerError> {
        let full_path = self.resolve(path)?;
        let imported_parts = load_first_mesh(&full_path)?;
        let mut parts = Vec::with_capacity(imported_parts.len());

        for imported in imported_parts {
            let mesh_key = format!(
                "{}#mesh:{}#primitive:{}",
                full_path.display(),
                imported.mesh_index,
                imported.primitive_index,
            );
            let image_key = match imported.base_color_image_index {
                Some(index) => format!("{}#image:{}", full_path.display(), index),
                None => "builtin:white".to_string(),
            };
            let mesh = self.load_mesh(instance, device, mesh_key, &imported.mesh)?;
            let material = self.load_material(instance, device, &image_key, &imported.material)?;
            engine_logger::info!(
                "Prepared primitive {}: {} indices, double-sided {}",
                imported.primitive_index,
                mesh.index_count(),
                material.double_sided(),
            );
            parts.push(ModelPart::new(mesh, material));
        }

        engine_logger::info!(
            "Loaded {}: {} model parts; GPU meshes: {}, GPU textures: {}",
            full_path.display(),
            parts.len(),
            self.loaded_mesh_count(),
            self.loaded_texture_count(),
        );
        Ok(Model::new(parts))
    }

    pub fn new(
        root: impl AsRef<Path>,
        material_layout: Arc<VulkanMaterialLayout>,
    ) -> Result<Self, AssetManagerError> {
        let root = root.as_ref().canonicalize()?;

        Ok(Self {
            root,
            textures: HashMap::new(),
            meshes: HashMap::new(),
            image_textures: HashMap::new(),
            material_layout,
        })
    }

    /*
     * Texture assets are identified by their
     * canonical file path.
     */
    pub fn load_texture(
        &mut self,

        instance: &VulkanInstance,

        device: &VulkanDevice,

        path: impl AsRef<Path>,
    ) -> Result<Arc<VulkanTexture>, AssetManagerError> {
        let full_path = self.resolve(path)?;

        if let Some(texture) = self.textures.get(&full_path) {
            return Ok(Arc::clone(texture));
        }

        let image = ImageAsset::load(&full_path)?;

        let texture = VulkanTexture::from_rgba8(
            instance.raw(),
            device,
            Arc::clone(&self.material_layout),
            image.width(),
            image.height(),
            image.pixels(),
        )?;

        let texture = Arc::new(texture);

        self.textures.insert(full_path, Arc::clone(&texture));

        Ok(texture)
    }

    /*
     * Upload CPU MeshData once and cache
     * the resulting GPU mesh.
     *
     * For procedural/builtin resources use
     * stable IDs such as:
     *
     * builtin:cube
     *
     * Later glTF can use IDs such as:
     *
     * models/scene.glb#mesh:2
     */
    pub fn load_mesh(
        &mut self,

        instance: &VulkanInstance,

        device: &VulkanDevice,

        id: impl Into<String>,

        data: &MeshData,
    ) -> Result<Arc<VulkanMesh>, AssetManagerError> {
        let id = id.into();

        if let Some(mesh) = self.meshes.get(&id) {
            return Ok(Arc::clone(mesh));
        }

        let mesh = VulkanMesh::new(instance.raw(), device, data.vertices(), data.indices())?;

        let mesh = Arc::new(mesh);

        self.meshes.insert(id, Arc::clone(&mesh));

        Ok(mesh)
    }

    pub fn loaded_texture_count(&self) -> usize {
        self.textures.len() + self.image_textures.len()
    }

    pub fn loaded_mesh_count(&self) -> usize {
        self.meshes.len()
    }

    pub fn contains_texture(&self, path: impl AsRef<Path>) -> bool {
        let Ok(full_path) = self.resolve(path) else {
            return false;
        };

        self.textures.contains_key(&full_path)
    }

    pub fn contains_mesh(&self, id: &str) -> bool {
        self.meshes.contains_key(id)
    }

    pub fn clear(&mut self) {
        self.meshes.clear();

        self.textures.clear();
        self.image_textures.clear();
    }

    pub fn load_image_texture(
        &mut self,
        instance: &VulkanInstance,
        device: &VulkanDevice,
        id: impl Into<String>,
        image: &ImageAsset,
    ) -> Result<Arc<VulkanTexture>, AssetManagerError> {
        let id = id.into();

        if let Some(texture) = self.image_textures.get(&id) {
            return Ok(Arc::clone(texture));
        }

        let texture = Arc::new(VulkanTexture::from_rgba8(
            instance.raw(),
            device,
            Arc::clone(&self.material_layout),
            image.width(),
            image.height(),
            image.pixels(),
        )?);

        self.image_textures.insert(id, Arc::clone(&texture));
        Ok(texture)
    }

    pub fn load_material(
        &mut self,
        instance: &VulkanInstance,
        device: &VulkanDevice,
        image_id: &str,
        data: &MaterialData,
    ) -> Result<VulkanMaterial, AssetManagerError> {
        let texture = match data.base_color_image.as_ref() {
            Some(image) => self.load_image_texture(instance, device, image_id, image)?,
            None => {
                let white = ImageAsset::from_rgba8(1, 1, vec![255, 255, 255, 255])?;
                self.load_image_texture(instance, device, "builtin:white", &white)?
            }
        };

        Ok(VulkanMaterial::new(
            texture,
            data.double_sided,
            data.base_color_factor,
        ))
    }

    fn resolve(&self, path: impl AsRef<Path>) -> Result<PathBuf, AssetManagerError> {
        let path = self.root.join(path.as_ref()).canonicalize()?;

        if !path.starts_with(&self.root) {
            return Err(AssetManagerError::OutsideAssetRoot {
                path,

                root: self.root.clone(),
            });
        }

        Ok(path)
    }
}
