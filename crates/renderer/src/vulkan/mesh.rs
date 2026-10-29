use std::{
	error::Error,
	fmt::{Display, Formatter},
};

use ash::{vk, Instance};

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

impl Error for VulkanMeshError {
	fn source(&self) -> Option<&(dyn Error + 'static)> {
		match self {
			Self::Buffer(error) => Some(error),
			_ => None,
		}
	}
}

impl From<VulkanBufferError> for VulkanMeshError {
	fn from(value: VulkanBufferError) -> Self {
		Self::Buffer(value)
	}
}

impl VulkanMesh {
	pub fn set_debug_label(&self, label: &str) {
		self.vertex_buffer
			.set_debug_label(&format!("{label} · vertices"));
		self.index_buffer
			.set_debug_label(&format!("{label} · indices"));
	}
	pub(crate) fn dynamic(
		instance: &Instance,
		device: &VulkanDevice,
		vertices: usize,
		indices: usize,
	) -> Result<Self, VulkanMeshError> {
		let properties =
			vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT;
		let vertex_buffer = VulkanBuffer::new(
			instance,
			device,
			(vertices * size_of::<Vertex>()) as u64,
			vk::BufferUsageFlags::VERTEX_BUFFER,
			properties,
		)?;
		let index_buffer = VulkanBuffer::new(
			instance,
			device,
			(indices * size_of::<u32>()) as u64,
			vk::BufferUsageFlags::INDEX_BUFFER,
			properties,
		)?;
		Ok(Self {
			vertex_buffer,
			index_buffer,
			index_count: 0,
		})
	}

	/// Caller must wait for all draws that reference these buffers first.
	pub(crate) fn write_dynamic(
		&mut self,
		vertices: &[Vertex],
		indices: &[u32],
	) -> Result<(), VulkanMeshError> {
		let count = u32::try_from(indices.len()).map_err(|_| VulkanMeshError::TooManyIndices)?;
		if size_of_val(vertices) as u64 > self.vertex_buffer.size()
			|| size_of_val(indices) as u64 > self.index_buffer.size()
		{
			return Err(VulkanMeshError::TooManyIndices);
		}
		self.vertex_buffer.write(vertices)?;
		self.index_buffer.write(indices)?;
		self.index_count = count;
		Ok(())
	}
	pub fn new(
		instance: &Instance,
		device: &VulkanDevice,
		vertices: &[Vertex],
		indices: &[u32],
	) -> Result<Self, VulkanMeshError> {
		let resources = Self::prepare_upload(instance, device, vertices, indices)?;
		let (mesh, _) =
			super::upload::submit(device, resources, |(mesh, staging), raw, command| {
				super::upload::copy_buffer(raw, command, &staging[0], mesh.vertex_buffer());
				super::upload::copy_buffer(raw, command, &staging[1], mesh.index_buffer());
			})
				.map_err(VulkanBufferError::from)?;
		Ok(mesh)
	}

	pub(super) fn prepare_upload(
		instance: &Instance,
		device: &VulkanDevice,
		vertices: &[Vertex],
		indices: &[u32],
	) -> Result<(Self, [VulkanBuffer; 2]), VulkanMeshError> {
		let index_count =
			u32::try_from(indices.len()).map_err(|_| VulkanMeshError::TooManyIndices)?;

		let (vertices, vertex_buffer) = VulkanBuffer::prepare_static(
			instance,
			device,
			vertices,
			vk::BufferUsageFlags::VERTEX_BUFFER,
		)?;

		let (indices, index_buffer) = VulkanBuffer::prepare_static(
			instance,
			device,
			indices,
			vk::BufferUsageFlags::INDEX_BUFFER,
		)?;

		Ok((
			Self {
				vertex_buffer,
				index_buffer,
				index_count,
			},
			[vertices, indices],
		))
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
