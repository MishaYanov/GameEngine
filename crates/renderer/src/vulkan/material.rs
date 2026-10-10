use std::sync::Arc;

use ash::vk;

use super::VulkanTexture;

pub struct VulkanMaterial {
    texture: Arc<VulkanTexture>,
}

impl VulkanMaterial {
    pub fn new(texture: Arc<VulkanTexture>) -> Self {
        Self { texture }
    }

    pub fn descriptor_set_layout(&self) -> vk::DescriptorSetLayout {
        self.texture.descriptor_set_layout()
    }

    pub fn descriptor_set(&self) -> vk::DescriptorSet {
        self.texture.descriptor_set()
    }

    pub fn texture(&self) -> &VulkanTexture {
        &self.texture
    }

    pub fn texture_arc(&self) -> &Arc<VulkanTexture> {
        &self.texture
    }
}
