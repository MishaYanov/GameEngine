use std::{
    error::Error,
    fmt::{Display, Formatter},
};

use ash::{Device, Instance, vk};

use super::VulkanDevice;

pub struct VulkanDepthBuffer {
    device: Device,

    image: vk::Image,
    memory: vk::DeviceMemory,
    view: vk::ImageView,

    extent: vk::Extent2D,
    format: vk::Format,
}

#[derive(Debug)]
pub enum VulkanDepthBufferError {
    Vulkan(vk::Result),
    NoSuitableMemoryType,
}

impl Display for VulkanDepthBufferError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Vulkan(error) => {
                write!(formatter, "Vulkan depth-buffer error: {error:?}",)
            }

            Self::NoSuitableMemoryType => {
                write!(
                    formatter,
                    "no suitable Vulkan memory type found for depth buffer",
                )
            }
        }
    }
}

impl Error for VulkanDepthBufferError {}

impl From<vk::Result> for VulkanDepthBufferError {
    fn from(value: vk::Result) -> Self {
        Self::Vulkan(value)
    }
}

impl VulkanDepthBuffer {
    pub const FORMAT: vk::Format = vk::Format::D32_SFLOAT;

    pub fn new(
        instance: &Instance,
        device: &VulkanDevice,
        extent: vk::Extent2D,
    ) -> Result<Self, VulkanDepthBufferError> {
        let ash_device = device.raw().clone();

        let image_info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .extent(vk::Extent3D {
                width: extent.width,

                height: extent.height,

                depth: 1,
            })
            .mip_levels(1)
            .array_layers(1)
            .format(Self::FORMAT)
            .tiling(vk::ImageTiling::OPTIMAL)
            .initial_layout(vk::ImageLayout::UNDEFINED)
            .usage(vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT)
            .samples(vk::SampleCountFlags::TYPE_1)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);

        let image = unsafe { ash_device.create_image(&image_info, None)? };

        let requirements = unsafe { ash_device.get_image_memory_requirements(image) };

        let physical_memory_properties =
            unsafe { instance.get_physical_device_memory_properties(device.physical_device()) };

        let memory_type_index = match find_memory_type(
            requirements.memory_type_bits,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
            &physical_memory_properties,
        ) {
            Some(index) => index,

            None => {
                unsafe {
                    ash_device.destroy_image(image, None);
                }

                return Err(VulkanDepthBufferError::NoSuitableMemoryType);
            }
        };

        let allocation_info = vk::MemoryAllocateInfo::default()
            .allocation_size(requirements.size)
            .memory_type_index(memory_type_index);

        let memory = match unsafe { ash_device.allocate_memory(&allocation_info, None) } {
            Ok(memory) => memory,

            Err(error) => {
                unsafe {
                    ash_device.destroy_image(image, None);
                }

                return Err(error.into());
            }
        };

        if let Err(error) = unsafe { ash_device.bind_image_memory(image, memory, 0) } {
            unsafe {
                ash_device.free_memory(memory, None);

                ash_device.destroy_image(image, None);
            }

            return Err(error.into());
        }

        let subresource_range = vk::ImageSubresourceRange::default()
            .aspect_mask(vk::ImageAspectFlags::DEPTH)
            .base_mip_level(0)
            .level_count(1)
            .base_array_layer(0)
            .layer_count(1);

        let view_info = vk::ImageViewCreateInfo::default()
            .image(image)
            .view_type(vk::ImageViewType::TYPE_2D)
            .format(Self::FORMAT)
            .subresource_range(subresource_range);

        let view = match unsafe { ash_device.create_image_view(&view_info, None) } {
            Ok(view) => view,

            Err(error) => {
                unsafe {
                    ash_device.free_memory(memory, None);

                    ash_device.destroy_image(image, None);
                }

                return Err(error.into());
            }
        };

        Ok(Self {
            device: ash_device,

            image,
            memory,
            view,

            extent,
            format: Self::FORMAT,
        })
    }

    pub fn image(&self) -> vk::Image {
        self.image
    }

    pub fn view(&self) -> vk::ImageView {
        self.view
    }

    pub fn format(&self) -> vk::Format {
        self.format
    }

    pub fn extent(&self) -> vk::Extent2D {
        self.extent
    }
}

impl Drop for VulkanDepthBuffer {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_image_view(self.view, None);

            self.device.destroy_image(self.image, None);

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
