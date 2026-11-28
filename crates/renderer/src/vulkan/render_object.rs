use std::ops::Range;

use glam::{Mat3, Mat4};

use super::{ModelPushConstants, VulkanMaterial, VulkanMesh};

pub struct RenderObject<'a> {
	mesh: &'a VulkanMesh,
	material: &'a VulkanMaterial,
	transform: ModelPushConstants,
	palette: Option<&'a [[f32; 16]]>,
	vertex_markers: bool,
	index_range: Option<Range<u32>>,
	visible: bool,
	shadow_caster: bool,
	reflected: bool,
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
	/// Request depth-tested vertex markers for this object in an editor draw.
	pub fn with_vertex_markers(mut self) -> Self {
		self.vertex_markers = true;
		self
	}
	pub(crate) fn shows_vertex_markers(&self) -> bool {
		self.uses_full_mesh() && self.vertex_markers && self.mesh.editor_vertex_count() != 0
	}
	/// Draw a contiguous, validated triangle range using this object's material.
	pub fn with_index_range(mut self, range: Range<u32>) -> Option<Self> {
		if range.start >= range.end
			|| range.end > self.mesh.index_count()
			|| !range.start.is_multiple_of(3)
			|| !range.end.is_multiple_of(3)
		{
			return None;
		}
		self.index_range = Some(range);
		Some(self)
	}
	pub(crate) fn uses_full_mesh(&self) -> bool {
		self.index_range.is_none()
	}
	pub fn index_range(&self) -> Range<u32> {
		self.index_range
			.clone()
			.unwrap_or(0..self.mesh.index_count())
	}
	pub fn new(
		mesh: &'a VulkanMesh,
		material: &'a VulkanMaterial,
		transform: ModelPushConstants,
	) -> Self {
		let reflected =
			Mat3::from_mat4(Mat4::from_cols_array(&transform.model)).determinant() < 0.0;
		Self {
			mesh,
			material,
			transform,
			palette: None,
			vertex_markers: false,
			index_range: None,
			visible: true,
			shadow_caster: true,
			reflected,
		}
	}

	/// Select the passes that need this object after view and light culling.
	pub fn with_pass_visibility(mut self, visible: bool, shadow_caster: bool) -> Self {
		self.visible = visible;
		self.shadow_caster = shadow_caster;
		self
	}

	pub(crate) fn visible(&self) -> bool {
		self.visible
	}

	pub(crate) fn shadow_caster(&self) -> bool {
		self.shadow_caster
	}

	pub(super) fn reflected(&self) -> bool {
		self.reflected
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
