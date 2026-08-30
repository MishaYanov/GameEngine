use ash::vk;

use super::VulkanTexture;

pub struct VulkanMaterial {
    texture: VulkanTexture,
}

impl VulkanMaterial {
    pub fn new(texture: VulkanTexture) -> Self {
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
}
