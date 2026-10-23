use super::{ModelPushConstants, VulkanMaterial, VulkanMesh};

pub struct RenderObject<'a> {
    mesh: &'a VulkanMesh,
    material: &'a VulkanMaterial,
    transform: ModelPushConstants,
}

impl<'a> RenderObject<'a> {
	pub fn unlit(mut self) -> Self {
		self.transform.flags[0] |= 1;
		self
	}
	pub fn screen_space(mut self) -> Self {
		self.transform.flags[0] |= 3;
		self
	}
    pub fn new(
        mesh: &'a VulkanMesh,
        material: &'a VulkanMaterial,
        transform: ModelPushConstants,
    ) -> Self {
        Self {
            mesh,
            material,
            transform,
        }
    }

    pub fn mesh(&self) -> &VulkanMesh {
        self.mesh
    }

    pub fn material(&self) -> &VulkanMaterial {
        self.material
    }

    pub fn transform(&self) -> &ModelPushConstants {
        &self.transform
    }
}
