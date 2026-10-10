use renderer::vulkan::{VulkanMaterial, VulkanMesh};
use std::sync::Arc;

pub struct ModelPart {
    mesh: Arc<VulkanMesh>,
    material: VulkanMaterial,
}

impl ModelPart {
    pub(super) fn new(mesh: Arc<VulkanMesh>, material: VulkanMaterial) -> Self {
        Self { mesh, material }
    }

    pub fn mesh(&self) -> &VulkanMesh {
        self.mesh.as_ref()
    }

    pub fn material(&self) -> &VulkanMaterial {
        &self.material
    }
}

pub struct Model {
    parts: Vec<ModelPart>,
}

impl Model {
    pub(super) fn new(parts: Vec<ModelPart>) -> Self {
        Self { parts }
    }

    pub fn parts(&self) -> &[ModelPart] {
        &self.parts
    }
}
