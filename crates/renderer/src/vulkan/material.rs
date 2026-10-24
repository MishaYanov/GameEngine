use std::sync::Arc;

use ash::vk;

use super::VulkanTexture;

pub struct VulkanMaterial {
	texture: Arc<VulkanTexture>,
	double_sided: bool,
	base_color_factor: [f32; 4],
}

impl VulkanMaterial {
	pub fn new(
		texture: Arc<VulkanTexture>,
		double_sided: bool,
		base_color_factor: [f32; 4],
	) -> Self {
		Self {
			texture,
			double_sided,
			base_color_factor,
		}
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

	pub fn double_sided(&self) -> bool {
		self.double_sided
	}

	pub fn base_color_factor(&self) -> [f32; 4] {
		self.base_color_factor
	}
}
