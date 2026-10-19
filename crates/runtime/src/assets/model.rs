use renderer::vulkan::{VulkanMaterial, VulkanMesh};
use std::sync::Arc;

pub struct ModelPart {
    mesh: Arc<VulkanMesh>,
    material: VulkanMaterial,
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

pub struct Model {
    parts: Vec<ModelPart>,
}

impl Model {
    pub fn new(parts: Vec<ModelPart>) -> Self {
        Self { parts }
    }

    pub fn parts(&self) -> &[ModelPart] {
        &self.parts
    }
}

#[cfg(test)]
impl Model {
    pub fn empty_for_test() -> Self {
        Self::new(Vec::new())
    }
}
