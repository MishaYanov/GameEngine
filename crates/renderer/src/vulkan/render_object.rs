use super::{ModelPushConstants, VulkanMaterial, VulkanMesh};

pub struct RenderObject<'a> {
	mesh: &'a VulkanMesh,
	material: &'a VulkanMaterial,
	transform: ModelPushConstants,
	palette: Option<&'a [[f32; 16]]>,
}

impl<'a> RenderObject<'a> {
	pub fn with_skin_palette(mut self, palette: &'a [[f32; 16]]) -> Self {
		self.palette = Some(palette);
		self
	}
	pub fn skin_palette(&self) -> Option<&'a [[f32; 16]]> {
		self.palette
	}
	/// Multiplies the material's factor without replacing the shared material.
	pub fn with_tint(mut self, tint: [f32; 4]) -> Self {
		for (component, factor) in self.transform.base_color_factor.iter_mut().zip(tint) {
			*component *= factor;
		}
		self
	}
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
			palette: None,
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
