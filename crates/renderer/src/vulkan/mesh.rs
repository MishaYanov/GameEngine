use std::{
    error::Error,
    fmt::{Display, Formatter},
};

use ash::Instance;

use super::{Vertex, VulkanBuffer, VulkanBufferError, VulkanDevice};

pub struct VulkanMesh {
    vertex_buffer: VulkanBuffer,
    index_buffer: VulkanBuffer,
    index_count: u32,
}

#[derive(Debug)]
pub enum VulkanMeshError {
    Buffer(VulkanBufferError),
    TooManyIndices,
}

impl Display for VulkanMeshError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Buffer(error) => {
                write!(formatter, "Vulkan mesh buffer error: {error}",)
            }

            Self::TooManyIndices => {
                write!(formatter, "mesh contains too many indices",)
            }
        }
    }
}

impl Error for VulkanMeshError {}

impl From<VulkanBufferError> for VulkanMeshError {
    fn from(value: VulkanBufferError) -> Self {
        Self::Buffer(value)
    }
}

impl VulkanMesh {
    pub fn new(
        instance: &Instance,
        device: &VulkanDevice,
        vertices: &[Vertex],
        indices: &[u16],
    ) -> Result<Self, VulkanMeshError> {
        let index_count =
            u32::try_from(indices.len()).map_err(|_| VulkanMeshError::TooManyIndices)?;

        let vertex_buffer = VulkanBuffer::vertex(instance, device, vertices)?;

        let index_buffer = VulkanBuffer::index(instance, device, indices)?;

        Ok(Self {
            vertex_buffer,
            index_buffer,
            index_count,
        })
    }

    pub fn vertex_buffer(&self) -> &VulkanBuffer {
        &self.vertex_buffer
    }

    pub fn index_buffer(&self) -> &VulkanBuffer {
        &self.index_buffer
    }

    pub fn index_count(&self) -> u32 {
        self.index_count
    }
}
