use std::mem::{offset_of, size_of};

use ash::vk;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Vertex {
	pub position: [f32; 3],
	pub uv: [f32; 2],
	pub normal: [f32; 3],
	/// Tangent XYZ and handedness W. Zero W means the basis is unavailable.
	pub tangent: [f32; 4],
	pub normal_uv: [f32; 2],
}

impl Vertex {
	pub const fn new(position: [f32; 3], uv: [f32; 2]) -> Self {
		Self {
			position,
			uv,
			normal: [0.0; 3],
			tangent: [0.0; 4],
			normal_uv: [0.0; 2],
		}
	}

	pub const fn with_normal(mut self, normal: [f32; 3]) -> Self {
		self.normal = normal;
		self
	}

	pub const fn with_normal_map(mut self, tangent: [f32; 4], normal_uv: [f32; 2]) -> Self {
		self.tangent = tangent;
		self.normal_uv = normal_uv;
		self
	}

	pub fn binding_description() -> vk::VertexInputBindingDescription {
		vk::VertexInputBindingDescription {
			binding: 0,

			stride: size_of::<Self>() as u32,

			input_rate: vk::VertexInputRate::VERTEX,
		}
	}

	pub fn attribute_descriptions() -> [vk::VertexInputAttributeDescription; 5] {
		[
			vk::VertexInputAttributeDescription {
				location: 2,
				binding: 0,
				format: vk::Format::R32G32B32_SFLOAT,
				offset: offset_of!(Vertex, normal) as u32,
			},
			vk::VertexInputAttributeDescription {
				location: 0,
				binding: 0,

				format: vk::Format::R32G32B32_SFLOAT,

				offset: offset_of!(Vertex, position) as u32,
			},
			vk::VertexInputAttributeDescription {
				location: 1,
				binding: 0,

				format: vk::Format::R32G32_SFLOAT,

				offset: offset_of!(Vertex, uv) as u32,
			},
			vk::VertexInputAttributeDescription {
				location: 8,
				binding: 0,
				format: vk::Format::R32G32B32A32_SFLOAT,
				offset: offset_of!(Vertex, tangent) as u32,
			},
			vk::VertexInputAttributeDescription {
				location: 9,
				binding: 0,
				format: vk::Format::R32G32_SFLOAT,
				offset: offset_of!(Vertex, normal_uv) as u32,
			},
		]
	}
}
