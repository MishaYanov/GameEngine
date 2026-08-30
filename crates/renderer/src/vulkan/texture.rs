use std::{
    error::Error,
    fmt::{Display, Formatter},
};

use ash::{Device, Instance, vk};

use super::{VulkanBuffer, VulkanBufferError, VulkanDevice};

pub struct VulkanTexture {
    device: Device,

    image: vk::Image,
    memory: vk::DeviceMemory,
    view: vk::ImageView,
    sampler: vk::Sampler,

    descriptor_set_layout: vk::DescriptorSetLayout,

    descriptor_pool: vk::DescriptorPool,

    descriptor_set: vk::DescriptorSet,
}

#[derive(Debug)]
pub enum VulkanTextureError {
    Vulkan(vk::Result),

    Buffer(VulkanBufferError),

    NoSuitableMemoryType,

    InvalidPixelData { expected: usize, actual: usize },
}

impl Display for VulkanTextureError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Vulkan(error) => {
                write!(formatter, "Vulkan texture error: {error:?}",)
            }

            Self::Buffer(error) => {
                write!(formatter, "Vulkan texture buffer error: {error}",)
            }

            Self::NoSuitableMemoryType => {
                write!(
                    formatter,
                    "no suitable Vulkan memory type found for texture",
                )
            }

            Self::InvalidPixelData { expected, actual } => {
                write!(
                    formatter,
                    "invalid RGBA texture data: expected {expected} bytes, got {actual}",
                )
            }
        }
    }
}

impl Error for VulkanTextureError {}

impl From<vk::Result> for VulkanTextureError {
    fn from(value: vk::Result) -> Self {
        Self::Vulkan(value)
    }
}

impl From<VulkanBufferError> for VulkanTextureError {
    fn from(value: VulkanBufferError) -> Self {
        Self::Buffer(value)
    }
}

impl VulkanTexture {
    pub fn checkerboard(
        instance: &Instance,
        device: &VulkanDevice,
    ) -> Result<Self, VulkanTextureError> {
        Self::checkerboard_with_colors(instance, device, [235, 235, 235, 255], [35, 35, 35, 255])
    }

    pub fn checkerboard_with_colors(
        instance: &Instance,
        device: &VulkanDevice,

        color_a: [u8; 4],
        color_b: [u8; 4],
    ) -> Result<Self, VulkanTextureError> {
        const WIDTH: u32 = 4;
        const HEIGHT: u32 = 4;

        let mut pixels = Vec::with_capacity((WIDTH * HEIGHT * 4) as usize);

        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let color = if (x + y) % 2 == 0 { color_a } else { color_b };

                pixels.extend_from_slice(&color);
            }
        }

        Self::from_rgba8(instance, device, WIDTH, HEIGHT, &pixels)
    }

    pub fn from_rgba8(
        instance: &Instance,
        device: &VulkanDevice,

        width: u32,
        height: u32,

        pixels: &[u8],
    ) -> Result<Self, VulkanTextureError> {
        let expected_size = (width as usize) * (height as usize) * 4;

        if pixels.len() != expected_size {
            return Err(VulkanTextureError::InvalidPixelData {
                expected: expected_size,

                actual: pixels.len(),
            });
        }

        let ash_device = device.raw().clone();

        let image_size = pixels.len() as vk::DeviceSize;

        /*
         * CPU-visible staging buffer.
         */
        let staging = VulkanBuffer::new(
            instance,
            device,
            image_size,
            vk::BufferUsageFlags::TRANSFER_SRC,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        )?;

        staging.write(pixels)?;

        /*
         * GPU-local texture image.
         */
        let image_info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .extent(vk::Extent3D {
                width,
                height,
                depth: 1,
            })
            .mip_levels(1)
            .array_layers(1)
            .format(vk::Format::R8G8B8A8_SRGB)
            .tiling(vk::ImageTiling::OPTIMAL)
            .initial_layout(vk::ImageLayout::UNDEFINED)
            .usage(vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED)
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

                return Err(VulkanTextureError::NoSuitableMemoryType);
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

        if let Err(error) = upload_image(device, staging.raw(), image, width, height) {
            unsafe {
                ash_device.free_memory(memory, None);

                ash_device.destroy_image(image, None);
            }

            return Err(error);
        }

        let range = vk::ImageSubresourceRange::default()
            .aspect_mask(vk::ImageAspectFlags::COLOR)
            .base_mip_level(0)
            .level_count(1)
            .base_array_layer(0)
            .layer_count(1);

        let view_info = vk::ImageViewCreateInfo::default()
            .image(image)
            .view_type(vk::ImageViewType::TYPE_2D)
            .format(vk::Format::R8G8B8A8_SRGB)
            .subresource_range(range);

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

        let sampler_info = vk::SamplerCreateInfo::default()
            .mag_filter(vk::Filter::NEAREST)
            .min_filter(vk::Filter::NEAREST)
            .mipmap_mode(vk::SamplerMipmapMode::NEAREST)
            .address_mode_u(vk::SamplerAddressMode::REPEAT)
            .address_mode_v(vk::SamplerAddressMode::REPEAT)
            .address_mode_w(vk::SamplerAddressMode::REPEAT)
            .mip_lod_bias(0.0)
            .anisotropy_enable(false)
            .compare_enable(false)
            .min_lod(0.0)
            .max_lod(0.0)
            .unnormalized_coordinates(false);

        let sampler = match unsafe { ash_device.create_sampler(&sampler_info, None) } {
            Ok(sampler) => sampler,

            Err(error) => {
                unsafe {
                    ash_device.destroy_image_view(view, None);

                    ash_device.free_memory(memory, None);

                    ash_device.destroy_image(image, None);
                }

                return Err(error.into());
            }
        };

        /*
         * set = 1
         * binding = 0
         */
        let bindings = [vk::DescriptorSetLayoutBinding::default()
            .binding(0)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::FRAGMENT)];

        let layout_info = vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings);

        let descriptor_set_layout =
            match unsafe { ash_device.create_descriptor_set_layout(&layout_info, None) } {
                Ok(layout) => layout,

                Err(error) => {
                    unsafe {
                        ash_device.destroy_sampler(sampler, None);

                        ash_device.destroy_image_view(view, None);

                        ash_device.free_memory(memory, None);

                        ash_device.destroy_image(image, None);
                    }

                    return Err(error.into());
                }
            };

        let pool_sizes = [vk::DescriptorPoolSize {
            ty: vk::DescriptorType::COMBINED_IMAGE_SAMPLER,

            descriptor_count: 1,
        }];

        let pool_info = vk::DescriptorPoolCreateInfo::default()
            .pool_sizes(&pool_sizes)
            .max_sets(1);

        let descriptor_pool = match unsafe { ash_device.create_descriptor_pool(&pool_info, None) } {
            Ok(pool) => pool,

            Err(error) => {
                unsafe {
                    ash_device.destroy_descriptor_set_layout(descriptor_set_layout, None);

                    ash_device.destroy_sampler(sampler, None);

                    ash_device.destroy_image_view(view, None);

                    ash_device.free_memory(memory, None);

                    ash_device.destroy_image(image, None);
                }

                return Err(error.into());
            }
        };

        let layouts = [descriptor_set_layout];

        let allocate_info = vk::DescriptorSetAllocateInfo::default()
            .descriptor_pool(descriptor_pool)
            .set_layouts(&layouts);

        let descriptor_set = match unsafe { ash_device.allocate_descriptor_sets(&allocate_info) } {
            Ok(sets) => sets[0],

            Err(error) => {
                unsafe {
                    ash_device.destroy_descriptor_pool(descriptor_pool, None);

                    ash_device.destroy_descriptor_set_layout(descriptor_set_layout, None);

                    ash_device.destroy_sampler(sampler, None);

                    ash_device.destroy_image_view(view, None);

                    ash_device.free_memory(memory, None);

                    ash_device.destroy_image(image, None);
                }

                return Err(error.into());
            }
        };

        let image_infos = [vk::DescriptorImageInfo::default()
            .sampler(sampler)
            .image_view(view)
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];

        let writes = [vk::WriteDescriptorSet::default()
            .dst_set(descriptor_set)
            .dst_binding(0)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .image_info(&image_infos)];

        unsafe {
            ash_device.update_descriptor_sets(&writes, &[]);
        }

        Ok(Self {
            device: ash_device,

            image,
            memory,
            view,
            sampler,

            descriptor_set_layout,
            descriptor_pool,
            descriptor_set,
        })
    }

    pub fn descriptor_set_layout(&self) -> vk::DescriptorSetLayout {
        self.descriptor_set_layout
    }

    pub fn descriptor_set(&self) -> vk::DescriptorSet {
        self.descriptor_set
    }
}

impl Drop for VulkanTexture {
    fn drop(&mut self) {
        unsafe {
            self.device
                .destroy_descriptor_pool(self.descriptor_pool, None);

            self.device
                .destroy_descriptor_set_layout(self.descriptor_set_layout, None);

            self.device.destroy_sampler(self.sampler, None);

            self.device.destroy_image_view(self.view, None);

            self.device.destroy_image(self.image, None);

            self.device.free_memory(self.memory, None);
        }
    }
}

fn upload_image(
    device: &VulkanDevice,

    staging_buffer: vk::Buffer,
    image: vk::Image,

    width: u32,
    height: u32,
) -> Result<(), VulkanTextureError> {
    let ash_device = device.raw();

    let pool_info = vk::CommandPoolCreateInfo::default()
        .flags(vk::CommandPoolCreateFlags::TRANSIENT)
        .queue_family_index(device.queue_families().graphics);

    let command_pool = unsafe { ash_device.create_command_pool(&pool_info, None)? };

    let result = (|| -> Result<(), VulkanTextureError> {
        let allocate_info = vk::CommandBufferAllocateInfo::default()
            .command_pool(command_pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1);

        let command_buffer = unsafe { ash_device.allocate_command_buffers(&allocate_info)? }[0];

        let begin_info = vk::CommandBufferBeginInfo::default()
            .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);

        unsafe {
            ash_device.begin_command_buffer(command_buffer, &begin_info)?;
        }

        let range = vk::ImageSubresourceRange::default()
            .aspect_mask(vk::ImageAspectFlags::COLOR)
            .base_mip_level(0)
            .level_count(1)
            .base_array_layer(0)
            .layer_count(1);

        let to_transfer = vk::ImageMemoryBarrier::default()
            .src_access_mask(vk::AccessFlags::empty())
            .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .old_layout(vk::ImageLayout::UNDEFINED)
            .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .image(image)
            .subresource_range(range);

        unsafe {
            ash_device.cmd_pipeline_barrier(
                command_buffer,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[to_transfer],
            );
        }

        let subresource = vk::ImageSubresourceLayers::default()
            .aspect_mask(vk::ImageAspectFlags::COLOR)
            .mip_level(0)
            .base_array_layer(0)
            .layer_count(1);

        let regions = [vk::BufferImageCopy::default()
            .buffer_offset(0)
            .buffer_row_length(0)
            .buffer_image_height(0)
            .image_subresource(subresource)
            .image_offset(vk::Offset3D { x: 0, y: 0, z: 0 })
            .image_extent(vk::Extent3D {
                width,
                height,
                depth: 1,
            })];

        unsafe {
            ash_device.cmd_copy_buffer_to_image(
                command_buffer,
                staging_buffer,
                image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &regions,
            );
        }

        let to_shader = vk::ImageMemoryBarrier::default()
            .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .dst_access_mask(vk::AccessFlags::SHADER_READ)
            .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
            .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .image(image)
            .subresource_range(range);

        unsafe {
            ash_device.cmd_pipeline_barrier(
                command_buffer,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::FRAGMENT_SHADER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[to_shader],
            );

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
