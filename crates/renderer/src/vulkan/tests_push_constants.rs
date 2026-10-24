use super::ModelPushConstants;
use std::mem::{offset_of, size_of};

#[test]
fn instance_vertex_stream_layout_matches_shader_matrix_and_tint_locations() {
	use crate::vulkan::instancing::InstanceData;
	let binding = InstanceData::binding_description();
	assert_eq!(size_of::<InstanceData>(), 80);
	assert_eq!(binding.binding, 1);
	assert_eq!(binding.stride, 80);
	assert_eq!(binding.input_rate, ash::vk::VertexInputRate::INSTANCE);
	let attributes = InstanceData::attribute_descriptions();
	for (index, attribute) in attributes.iter().enumerate() {
		assert_eq!(attribute.location, 3 + index as u32);
		assert_eq!(attribute.binding, 1);
		assert_eq!(attribute.offset, index as u32 * 16);
		assert_eq!(attribute.format, ash::vk::Format::R32G32B32A32_SFLOAT);
	}
}

#[test]
fn push_constant_layout_matches_shader_block() {
	// GLSL std430: mat4, vec4 color, uvec4 flags.
	assert_eq!(offset_of!(ModelPushConstants, model), 0);
	assert_eq!(offset_of!(ModelPushConstants, base_color_factor), 64);
	assert_eq!(offset_of!(ModelPushConstants, flags), 80);
	assert_eq!(size_of::<ModelPushConstants>(), 96);
	assert_eq!(ModelPushConstants::SIZE, 96);
	let defaults = ModelPushConstants::identity();
	assert_eq!(defaults.flags, [0; 4]);
	assert_eq!(defaults.as_bytes().len(), 96);
}

#[test]
fn normal_vertex_attribute_matches_the_gpu_buffer_layout() {
	use crate::vulkan::Vertex;
	use ash::vk;
	let vertex = Vertex::new([1.0, 2.0, 3.0], [0.2, 0.3]).with_normal([0.0, 1.0, 0.0]);
	assert_eq!(vertex.position, [1.0, 2.0, 3.0]);
	assert_eq!(vertex.uv, [0.2, 0.3]);
	assert_eq!(vertex.normal, [0.0, 1.0, 0.0]);
	assert_eq!(Vertex::binding_description().stride, 32);
	let attributes = Vertex::attribute_descriptions();
	for (location, offset, format) in [
		(0, 0, vk::Format::R32G32B32_SFLOAT),
		(1, 12, vk::Format::R32G32_SFLOAT),
		(2, 20, vk::Format::R32G32B32_SFLOAT),
	] {
		let attribute = attributes.iter().find(|a| a.location == location).unwrap();
		assert_eq!(attribute.offset, offset);
		assert_eq!(attribute.format, format);
		assert_eq!(attribute.binding, 0);
	}
}
