use std::sync::Arc;

use ash::vk;

use super::{ModelPushConstants, TextureColorSpace, VulkanTexture};

/// Surface properties read by both the color and shadow passes.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MaterialSurface {
	/// Coverage threshold for a masked material. A value above one hides it.
	pub alpha_cutoff: Option<f32>,
	/// Linear light emitted by a material without an emissive texture.
	pub emissive_factor: [f32; 3],
}

#[derive(Clone)]
pub struct VulkanMaterial {
	texture: Arc<VulkanTexture>,
	normal_map: Option<(Arc<VulkanTexture>, f32)>,
	double_sided: bool,
	base_color_factor: [f32; 4],
	surface: MaterialSurface,
}

impl VulkanMaterial {
	pub fn new(
		texture: Arc<VulkanTexture>,
		double_sided: bool,
		base_color_factor: [f32; 4],
	) -> Self {
		Self {
			texture,
			normal_map: None,
			double_sided,
			base_color_factor,
			surface: MaterialSurface::default(),
		}
	}

	pub fn with_surface(mut self, surface: MaterialSurface) -> Self {
		self.surface = surface;
		self
	}

	/// Bind a tangent-space map uploaded as linear RGBA8 on the base texture's device.
	pub fn with_normal_map(mut self, texture: Arc<VulkanTexture>, scale: f32) -> Self {
		if scale.is_finite()
			&& scale != 0.0
			&& texture.color_space() == TextureColorSpace::Linear
			&& texture.shares_device(&self.texture)
		{
			self.normal_map = Some((texture, scale));
		}
		self
	}

	pub(super) fn normal_map(&self) -> Option<(vk::DescriptorSet, f32)> {
		self.normal_map
			.as_ref()
			.map(|(image, scale)| (image.descriptor_set(), *scale))
	}

	pub(crate) fn surface(&self) -> MaterialSurface {
		self.surface
	}

	pub(super) fn apply_surface_constants(&self, constants: &mut ModelPushConstants) {
		if let Some(cutoff) = self.surface.alpha_cutoff {
			constants.flags[0] |=
				ModelPushConstants::ALPHA_TEST | ModelPushConstants::MATERIAL_ALPHA_CUTOFF;
			constants.flags[3] = cutoff.to_bits();
		}
		constants.emissive_factor[..3].copy_from_slice(&self.surface.emissive_factor);
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
