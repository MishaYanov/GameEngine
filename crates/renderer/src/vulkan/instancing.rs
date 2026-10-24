use std::mem::{offset_of, size_of};

use ash::vk;

use super::RenderObject;

/// Per-instance vertex input. Its layout matches shader locations 3 through 7.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct InstanceData {
	model: [f32; 16],
	tint: [f32; 4],
}

impl InstanceData {
	pub fn from_object(object: &RenderObject<'_>) -> Self {
		let mut tint = object.transform().base_color_factor;
		for (component, material) in tint.iter_mut().zip(object.material().base_color_factor()) {
			*component *= material;
		}
		Self {
			model: object.transform().model,
			tint,
		}
	}

	pub fn binding_description() -> vk::VertexInputBindingDescription {
		vk::VertexInputBindingDescription {
			binding: 1,
			stride: size_of::<Self>() as u32,
			input_rate: vk::VertexInputRate::INSTANCE,
		}
	}

	pub fn attribute_descriptions() -> [vk::VertexInputAttributeDescription; 5] {
		std::array::from_fn(|i| vk::VertexInputAttributeDescription {
			location: 3 + i as u32,
			binding: 1,
			format: vk::Format::R32G32B32A32_SFLOAT,
			offset: if i < 4 {
				(i * 16) as u32
			} else {
				offset_of!(Self, tint) as u32
			},
		})
	}
}

/// Actual submitted work; GPU time belongs to a previously completed frame.
/// It excludes CPU work, image acquisition, presentation and display pacing.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DrawMetrics {
	pub draw_calls: u64,
	pub instances: u64,
	pub triangles: u64,
	pub instanced_draw_calls: u64,
	pub gpu_time_ms: Option<f64>,
	pub gpu_timestamps_supported: bool,
}
