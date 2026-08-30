use std::mem::{offset_of, size_of};

use ash::vk;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Vertex {
    pub position: [f32; 3],
    pub uv: [f32; 2],
}

impl Vertex {
    pub const fn new(position: [f32; 3], uv: [f32; 2]) -> Self {
        Self { position, uv }
    }

    pub fn binding_description() -> vk::VertexInputBindingDescription {
        vk::VertexInputBindingDescription {
            binding: 0,

            stride: size_of::<Self>() as u32,

            input_rate: vk::VertexInputRate::VERTEX,
        }
    }

    pub fn attribute_descriptions() -> [vk::VertexInputAttributeDescription; 2] {
        [
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
        ]
    }
}
