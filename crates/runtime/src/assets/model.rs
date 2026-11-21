use renderer::vulkan::{VulkanMaterial, VulkanMesh};
use std::sync::Arc;

pub struct ModelPart {
	mesh: Arc<VulkanMesh>,
	material: VulkanMaterial,
}

impl Clone for ModelPart {
	fn clone(&self) -> Self {
		Self {
			mesh: Arc::clone(&self.mesh),
			material: VulkanMaterial::new(
				Arc::clone(self.material.texture_arc()),
				self.material.double_sided(),
				self.material.base_color_factor(),
			),
		}
	}
}

impl ModelPart {
	pub fn new(mesh: Arc<VulkanMesh>, material: VulkanMaterial) -> Self {
		Self { mesh, material }
	}

	pub fn mesh(&self) -> &VulkanMesh {
		self.mesh.as_ref()
	}

	pub(super) fn mesh_arc(&self) -> &Arc<VulkanMesh> {
		&self.mesh
	}

	pub fn material(&self) -> &VulkanMaterial {
		&self.material
	}
}

#[derive(Clone)]
pub struct Model {
	parts: Vec<ModelPart>,
	bounds: Option<crate::Bounds3>,
	animation_asset: Option<Arc<engine_core::animation::AnimationAsset>>,
}

impl Model {
	pub fn skinned(
		parts: Vec<ModelPart>,
		asset: Arc<engine_core::animation::AnimationAsset>,
	) -> Self {
		Self {
			parts,
			bounds: None,
			animation_asset: Some(asset),
		}
	}
	pub fn animation_asset(&self) -> Option<&Arc<engine_core::animation::AnimationAsset>> {
		self.animation_asset.as_ref()
	}

	pub fn new(parts: Vec<ModelPart>) -> Self {
		Self {
			parts,
			bounds: None,
			animation_asset: None,
		}
	}

	pub fn with_bounds(parts: Vec<ModelPart>, bounds: Option<crate::Bounds3>) -> Self {
		Self {
			parts,
			bounds,
			animation_asset: None,
		}
	}

	pub fn bounds(&self) -> Option<crate::Bounds3> {
		self.bounds
	}

	pub fn parts(&self) -> &[ModelPart] {
		&self.parts
	}
}
