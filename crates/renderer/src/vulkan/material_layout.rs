use ash::{Device, vk};

pub struct VulkanMaterialLayout {
    device: Device,
    layout: vk::DescriptorSetLayout,
}

impl VulkanMaterialLayout {
    pub fn new(device: &Device) -> Result<Self, vk::Result> {
        let bindings = [vk::DescriptorSetLayoutBinding::default()
            .binding(0)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::FRAGMENT)];

        let create_info = vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings);

        let layout = unsafe { device.create_descriptor_set_layout(&create_info, None)? };

        Ok(Self {
            device: device.clone(),
            layout,
        })
    }

    pub fn raw(&self) -> vk::DescriptorSetLayout {
        self.layout
    }
}

impl Drop for VulkanMaterialLayout {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_descriptor_set_layout(self.layout, None);
        }
    }
}
