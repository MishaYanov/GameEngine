mod asset_manager;
pub mod gltf_loader;
mod image_asset;
pub mod material_data;
mod mesh_data;

pub use asset_manager::AssetManager;

pub use image_asset::{ImageAsset, ImageAssetError};

pub use mesh_data::MeshData;

pub use gltf_loader::load_first_mesh;

pub use material_data::MaterialData;
