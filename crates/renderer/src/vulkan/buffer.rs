use std::{
    error::Error,
    fmt::{Display, Formatter},
    mem::size_of_val,
    ptr,
};

use ash::{Device, Instance, vk};

use super::VulkanDevice;

pub struct VulkanBuffer {
    device: Device,
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    size: vk::DeviceSize,
}

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

impl Error for VulkanBufferError {}

impl From<vk::Result> for VulkanBufferError {
    fn from(value: vk::Result) -> Self {
        Self::Vulkan(value)
    }
}

impl VulkanBuffer {
    pub fn new(
        instance: &Instance,
        device: &VulkanDevice,
        size: vk::DeviceSize,
        usage: vk::BufferUsageFlags,
        memory_properties: vk::MemoryPropertyFlags,
    ) -> Result<Self, VulkanBufferError> {
        let ash_device = device.raw().clone();

        let create_info = vk::BufferCreateInfo::default()
            .size(size)
            .usage(usage)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);

        let buffer = unsafe { ash_device.create_buffer(&create_info, None)? };

        let requirements = unsafe { ash_device.get_buffer_memory_requirements(buffer) };

        let physical_memory_properties =
            unsafe { instance.get_physical_device_memory_properties(device.physical_device()) };

        let memory_type_index = match find_memory_type(
            requirements.memory_type_bits,
            memory_properties,
            &physical_memory_properties,
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

        if let Err(error) = unsafe { ash_device.bind_buffer_memory(buffer, memory, 0) } {
            unsafe {
                ash_device.free_memory(memory, None);

                ash_device.destroy_buffer(buffer, None);
            }

            return Err(error.into());
        }

        Ok(Self {
            device: ash_device,
            buffer,
            memory,
            size,
        })
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

        copy_buffer(device, staging.raw(), gpu_buffer.raw(), size)?;

        /*
         * staging is dropped here after the GPU
         * copy has completed.
         */
        Ok(gpu_buffer)
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

        let mapped = unsafe {
            self.device
                .map_memory(self.memory, 0, data_size, vk::MemoryMapFlags::empty())?
        };

        unsafe {
            ptr::copy_nonoverlapping(
                data.as_ptr().cast::<u8>(),
                mapped.cast::<u8>(),
                data_size as usize,
            );

            self.device.unmap_memory(self.memory);
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
            self.device.destroy_buffer(self.buffer, None);

            self.device.free_memory(self.memory, None);
        }
    }
}

fn copy_buffer(
    device: &VulkanDevice,
    source: vk::Buffer,
    destination: vk::Buffer,
    size: vk::DeviceSize,
) -> Result<(), VulkanBufferError> {
    let ash_device = device.raw();

    let pool_info = vk::CommandPoolCreateInfo::default()
        .flags(vk::CommandPoolCreateFlags::TRANSIENT)
        .queue_family_index(device.queue_families().graphics);

    let command_pool = unsafe { ash_device.create_command_pool(&pool_info, None)? };

    let result = (|| -> Result<(), VulkanBufferError> {
        let allocate_info = vk::CommandBufferAllocateInfo::default()
            .command_pool(command_pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1);

        let command_buffers = unsafe { ash_device.allocate_command_buffers(&allocate_info)? };

        let command_buffer = command_buffers[0];

        let begin_info = vk::CommandBufferBeginInfo::default()
            .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);

        unsafe {
            ash_device.begin_command_buffer(command_buffer, &begin_info)?;
        }

        let copy_regions = [vk::BufferCopy::default()
            .src_offset(0)
            .dst_offset(0)
            .size(size)];

        unsafe {
            ash_device.cmd_copy_buffer(command_buffer, source, destination, &copy_regions);

            ash_device.end_command_buffer(command_buffer)?;
        }

        let command_buffers = [command_buffer];
        let submit_info = vk::SubmitInfo::default().command_buffers(&command_buffers);

        unsafe {
            ash_device.queue_submit(device.graphics_queue(), &[submit_info], vk::Fence::null())?;
            ash_device.queue_wait_idle(device.graphics_queue())?;
        }

        Ok(())
    })();

    unsafe {
        ash_device.destroy_command_pool(command_pool, None);
    }

    result
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
