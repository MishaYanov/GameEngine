use std::{
	error::Error,
	fmt::{Display, Formatter},
	mem::size_of_val,
	ptr,
	sync::Mutex,
};

use ash::{Device, Instance, vk};

use super::VulkanDevice;

pub struct VulkanBuffer {
	_metrics: engine_logger::metrics::ResourceGuard,
	device: Device,
	buffer: vk::Buffer,
	memory: vk::DeviceMemory,
	size: vk::DeviceSize,
	mapping: Mutex<Option<MappedMemory>>,
}

struct MappedMemory(std::ptr::NonNull<u8>);
// The allocation outlives this pointer. All host accesses are serialized by
// VulkanBuffer::mapping; callers must still finish GPU reads before writing.
unsafe impl Send for MappedMemory {}

#[derive(Debug)]
pub enum VulkanBufferError {
	Vulkan(vk::Result),
	NoSuitableMemoryType,
	EmptyData,
	DataTooLarge {
		buffer_size: vk::DeviceSize,
		data_size: vk::DeviceSize,
	},
}

impl Display for VulkanBufferError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Vulkan(error) => {
				write!(formatter, "Vulkan buffer error: {error:?}",)
			}

			Self::NoSuitableMemoryType => {
				write!(formatter, "no suitable Vulkan memory type found",)
			}

			Self::EmptyData => {
				write!(formatter, "cannot create Vulkan buffer from empty data",)
			}

			Self::DataTooLarge {
				buffer_size,
				data_size,
			} => {
				write!(
					formatter,
					"buffer upload is too large: \
					 {data_size} bytes for a \
					 {buffer_size}-byte buffer",
				)
			}
		}
	}
}

impl Error for VulkanBufferError {
	fn source(&self) -> Option<&(dyn Error + 'static)> {
		match self {
			Self::Vulkan(error) => Some(error),
			_ => None,
		}
	}
}

impl From<vk::Result> for VulkanBufferError {
	fn from(value: vk::Result) -> Self {
		Self::Vulkan(value)
	}
}

impl VulkanBuffer {
	pub(crate) fn set_debug_label(&self, label: &str) {
		self._metrics.set_label(label);
	}
	pub fn new(
		instance: &Instance,
		device: &VulkanDevice,
		size: vk::DeviceSize,
		usage: vk::BufferUsageFlags,
		memory_properties: vk::MemoryPropertyFlags,
	) -> Result<Self, VulkanBufferError> {
		let properties =
			unsafe { instance.get_physical_device_memory_properties(device.physical_device()) };
		Self::with_memory_properties(device, size, usage, memory_properties, &properties)
	}

	pub(crate) fn host_vertex(
		device: &VulkanDevice,
		size: vk::DeviceSize,
	) -> Result<Self, VulkanBufferError> {
		Self::with_memory_properties(
			device,
			size,
			vk::BufferUsageFlags::VERTEX_BUFFER,
			vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
			device.memory_properties(),
		)
	}

	fn with_memory_properties(
		device: &VulkanDevice,
		size: vk::DeviceSize,
		usage: vk::BufferUsageFlags,
		memory_properties: vk::MemoryPropertyFlags,
		physical_memory_properties: &vk::PhysicalDeviceMemoryProperties,
	) -> Result<Self, VulkanBufferError> {
		if size == 0 {
			return Err(VulkanBufferError::EmptyData);
		}
		let ash_device = device.raw().clone();

		let create_info = vk::BufferCreateInfo::default()
			.size(size)
			.usage(usage)
			.sharing_mode(vk::SharingMode::EXCLUSIVE);

		let buffer = unsafe { ash_device.create_buffer(&create_info, None)? };

		let requirements = unsafe { ash_device.get_buffer_memory_requirements(buffer) };

		let memory_type_index = match find_memory_type(
			requirements.memory_type_bits,
			memory_properties,
			physical_memory_properties,
		) {
			Some(index) => index,

			None => {
				unsafe {
					ash_device.destroy_buffer(buffer, None);
				}

				return Err(VulkanBufferError::NoSuitableMemoryType);
			}
		};

		let allocate_info = vk::MemoryAllocateInfo::default()
			.allocation_size(requirements.size)
			.memory_type_index(memory_type_index);

		let memory = match unsafe { ash_device.allocate_memory(&allocate_info, None) } {
			Ok(memory) => memory,

			Err(error) => {
				unsafe {
					ash_device.destroy_buffer(buffer, None);
				}

				return Err(error.into());
			}
		};
		let allocation_metrics = crate::metrics::allocation(
			device,
			"buffer",
			buffer,
			size,
			requirements.size,
			memory_type_index,
			&format!("usage={usage:?}"),
		);

		if let Err(error) = unsafe { ash_device.bind_buffer_memory(buffer, memory, 0) } {
			unsafe {
				ash_device.free_memory(memory, None);

				ash_device.destroy_buffer(buffer, None);
			}

			return Err(error.into());
		}

		let mut result = Self {
			_metrics: allocation_metrics,
			device: ash_device,
			buffer,
			memory,
			size,
			mapping: Mutex::new(None),
		};
		if memory_properties.contains(
			vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
		) {
			let pointer = unsafe {
				result
					.device
					.map_memory(memory, 0, size, vk::MemoryMapFlags::empty())?
			};
			*result
				.mapping
				.get_mut()
				.unwrap_or_else(|error| error.into_inner()) = Some(MappedMemory(
				std::ptr::NonNull::new(pointer.cast())
					.expect("successful Vulkan mapping is nonnull"),
			));
		}
		Ok(result)
	}

	pub fn vertex<T>(
		instance: &Instance,
		device: &VulkanDevice,
		data: &[T],
	) -> Result<Self, VulkanBufferError> {
		Self::upload_static(instance, device, data, vk::BufferUsageFlags::VERTEX_BUFFER)
	}

	pub fn index<T>(
		instance: &Instance,
		device: &VulkanDevice,
		data: &[T],
	) -> Result<Self, VulkanBufferError> {
		Self::upload_static(instance, device, data, vk::BufferUsageFlags::INDEX_BUFFER)
	}

	fn upload_static<T>(
		instance: &Instance,
		device: &VulkanDevice,
		data: &[T],
		final_usage: vk::BufferUsageFlags,
	) -> Result<Self, VulkanBufferError> {
		let buffers = Self::prepare_static(instance, device, data, final_usage)?;
		let (_, gpu) = super::upload::submit(device, buffers, |(staging, gpu), raw, command| {
			super::upload::copy_buffer(raw, command, staging, gpu);
		})?;
		Ok(gpu)
	}

	pub(super) fn prepare_static<T>(
		instance: &Instance,
		device: &VulkanDevice,
		data: &[T],
		final_usage: vk::BufferUsageFlags,
	) -> Result<(Self, Self), VulkanBufferError> {
		let size = size_of_val(data) as vk::DeviceSize;

		if size == 0 {
			return Err(VulkanBufferError::EmptyData);
		}

		let staging = Self::new(
			instance,
			device,
			size,
			vk::BufferUsageFlags::TRANSFER_SRC,
			vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
		)?;

		staging.write(data)?;

		/*
		 * Final GPU-local buffer.
		 *
		 * TRANSFER_DST is required because
		 * the staging buffer will copy into it.
		 */
		let gpu_buffer = Self::new(
			instance,
			device,
			size,
			final_usage | vk::BufferUsageFlags::TRANSFER_DST,
			vk::MemoryPropertyFlags::DEVICE_LOCAL,
		)?;

		Ok((staging, gpu_buffer))
	}

	pub fn write<T>(&self, data: &[T]) -> Result<(), VulkanBufferError> {
		let data_size = size_of_val(data) as vk::DeviceSize;

		if data_size > self.size {
			return Err(VulkanBufferError::DataTooLarge {
				buffer_size: self.size,

				data_size,
			});
		}

		if data_size == 0 {
			return Ok(());
		}

		let mapping = self
			.mapping
			.lock()
			.unwrap_or_else(|error| error.into_inner());
		let mapped = match mapping.as_ref() {
			Some(memory) => memory.0.as_ptr(),
			None => unsafe {
				self.device
					.map_memory(self.memory, 0, data_size, vk::MemoryMapFlags::empty())?
					.cast()
			},
		};

		unsafe {
			ptr::copy_nonoverlapping(
				data.as_ptr().cast::<u8>(),
				mapped.cast::<u8>(),
				data_size as usize,
			);

			if mapping.is_none() {
				self.device.unmap_memory(self.memory);
			}
		}

		Ok(())
	}

	pub fn raw(&self) -> vk::Buffer {
		self.buffer
	}

	pub fn size(&self) -> vk::DeviceSize {
		self.size
	}
}

impl Drop for VulkanBuffer {
	fn drop(&mut self) {
		unsafe {
			if self
				.mapping
				.get_mut()
				.unwrap_or_else(|error| error.into_inner())
				.is_some()
			{
				self.device.unmap_memory(self.memory);
			}
			self.device.destroy_buffer(self.buffer, None);

			self.device.free_memory(self.memory, None);
		}
	}
}

fn find_memory_type(
	type_filter: u32,
	required_properties: vk::MemoryPropertyFlags,
	memory_properties: &vk::PhysicalDeviceMemoryProperties,
) -> Option<u32> {
	for index in 0..memory_properties.memory_type_count {
		let supported = type_filter & (1_u32 << index) != 0;

		if !supported {
			continue;
		}

		let memory_type = memory_properties.memory_types[index as usize];

		if memory_type.property_flags.contains(required_properties) {
			return Some(index);
		}
	}

	None
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::vulkan::test_driver::{device, reset, state};

	#[test]
	fn coherent_buffer_maps_once_keeps_writes_bounded_and_unmaps_before_free() {
		reset();
		let device = device();
		let buffer = VulkanBuffer::host_vertex(&device, 4).unwrap();
		buffer.write(&[1u8, 2, 3, 4]).unwrap();
		buffer.write(&[9u8, 8]).unwrap();
		buffer.write::<u8>(&[]).unwrap();
		assert!(matches!(
			buffer.write(&[0u8; 5]),
			Err(VulkanBufferError::DataTooLarge { .. })
		));
		state(|state| {
			assert_eq!(state.calls["map_memory"], 1);
			assert_eq!(
				state.memory.values().next().unwrap().as_ref(),
				&[9, 8, 3, 4]
			);
			assert!(state.released.is_empty());
		});
		drop(buffer);
		state(|state| {
			assert_eq!(state.released, ["unmap", "buffer", "memory"]);
			assert!(state.memory.is_empty());
		});
		fn assert_send_sync<T: Send + Sync>() {}
		assert_send_sync::<VulkanBuffer>();
	}

	#[test]
	fn failed_persistent_map_releases_the_bound_buffer_and_memory() {
		reset();
		state(|state| state.fail = Some(("map_memory", 1)));
		let device = device();
		assert!(VulkanBuffer::host_vertex(&device, 4).is_err());
		state(|state| {
			assert_eq!(state.released, ["buffer", "memory"]);
			assert!(state.memory.is_empty());
		});
	}
}
