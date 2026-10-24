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

	pub fn material(&self) -> &VulkanMaterial {
		&self.material
	}
}

#[derive(Clone)]
pub struct Model {
	parts: Vec<ModelPart>,
	bounds: Option<crate::Bounds3>,
}

impl Model {
	pub fn new(parts: Vec<ModelPart>) -> Self {
		Self {
			parts,
			bounds: None,
		}
	}

	pub fn with_bounds(parts: Vec<ModelPart>, bounds: Option<crate::Bounds3>) -> Self {
		Self { parts, bounds }
	}

	pub fn bounds(&self) -> Option<crate::Bounds3> {
		self.bounds
	}

	pub fn parts(&self) -> &[ModelPart] {
		&self.parts
	}
}
