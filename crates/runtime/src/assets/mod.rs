mod asset_manager;
pub mod catalog;
pub mod gltf_loader;
mod image_asset;
mod import_budget;
pub mod material_data;
mod mesh_data;
mod metadata_import;
pub mod model;
mod primitives;
pub mod project_assets;
mod scene_import;
mod warm_model;

pub use warm_model::PreparedModelBatch;

pub use asset_manager::AssetManager;
pub use asset_manager::AssetManagerError;
pub use asset_manager::PreparedGpuAsset;
pub use metadata_import::{AssetImportService, SceneMetadataImporter};
pub use scene_import::{
	ImportedMaterial, ImportedMesh, ImportedNode, ImportedPrimitiveMetadata, ImportedScene,
};

pub use image_asset::{ImageAsset, ImageAssetError};

pub use mesh_data::MeshData;

pub use material_data::MaterialData;

pub use model::Model;
