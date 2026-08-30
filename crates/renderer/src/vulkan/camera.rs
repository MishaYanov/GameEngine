use std::{
    error::Error,
    fmt::{Display, Formatter},
    mem::size_of,
};

use ash::{Device, Instance, vk};

use glam::{
    Vec3,
    camera::rh::{proj::vulkan::perspective, view::look_at_mat4},
};

use super::{VulkanBuffer, VulkanBufferError, VulkanDevice};

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CameraUniform {
    pub view_projection: [f32; 16],
}

pub struct VulkanCamera {
    device: Device,

    uniform_buffer: VulkanBuffer,

    descriptor_set_layout: vk::DescriptorSetLayout,

    descriptor_pool: vk::DescriptorPool,

    descriptor_set: vk::DescriptorSet,
}

#[derive(Debug)]
pub enum VulkanCameraError {
    Vulkan(vk::Result),
    Buffer(VulkanBufferError),
}

impl Display for VulkanCameraError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Vulkan(error) => {
                write!(formatter, "Vulkan camera error: {error:?}",)
            }

            Self::Buffer(error) => {
                write!(formatter, "Vulkan camera buffer error: {error}",)
            }
        }
    }
}

impl Error for VulkanCameraError {}

impl From<vk::Result> for VulkanCameraError {
    fn from(value: vk::Result) -> Self {
        Self::Vulkan(value)
    }
}

impl From<VulkanBufferError> for VulkanCameraError {
    fn from(value: VulkanBufferError) -> Self {
        Self::Buffer(value)
    }
}

impl CameraUniform {
    pub fn perspective(
        position: [f32; 3],
        target: [f32; 3],
        up: [f32; 3],

        vertical_fov_radians: f32,
        aspect_ratio: f32,

        near: f32,
        far: f32,
    ) -> Self {
        let position = Vec3::from_array(position);

        let target = Vec3::from_array(target);

        let up = Vec3::from_array(up).normalize();

        let view = look_at_mat4(position, target, up);

        let projection = perspective(vertical_fov_radians, aspect_ratio, near, far);

        let view_projection = projection * view;

        Self {
            view_projection: view_projection.to_cols_array(),
        }
    }
}

impl VulkanCamera {
    pub fn new(instance: &Instance, device: &VulkanDevice) -> Result<Self, VulkanCameraError> {
        let ash_device = device.raw().clone();

        let uniform_size = size_of::<CameraUniform>() as vk::DeviceSize;

        let uniform_buffer = VulkanBuffer::new(
            instance,
            device,
            uniform_size,
            vk::BufferUsageFlags::UNIFORM_BUFFER,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        )?;

        let bindings = [vk::DescriptorSetLayoutBinding::default()
            .binding(0)
            .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::VERTEX)];

        let layout_info = vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings);

        let descriptor_set_layout =
            unsafe { ash_device.create_descriptor_set_layout(&layout_info, None)? };

        let pool_sizes = [vk::DescriptorPoolSize {
            ty: vk::DescriptorType::UNIFORM_BUFFER,

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
                }

                return Err(error.into());
            }
        };

        let layouts = [descriptor_set_layout];

        let allocate_info = vk::DescriptorSetAllocateInfo::default()
            .descriptor_pool(descriptor_pool)
            .set_layouts(&layouts);

        let descriptor_sets = match unsafe { ash_device.allocate_descriptor_sets(&allocate_info) } {
            Ok(sets) => sets,

            Err(error) => {
                unsafe {
                    ash_device.destroy_descriptor_pool(descriptor_pool, None);

                    ash_device.destroy_descriptor_set_layout(descriptor_set_layout, None);
                }

                return Err(error.into());
            }
        };

        let descriptor_set = descriptor_sets[0];

        let buffer_info = [vk::DescriptorBufferInfo::default()
            .buffer(uniform_buffer.raw())
            .offset(0)
            .range(uniform_size)];

        let writes = [vk::WriteDescriptorSet::default()
            .dst_set(descriptor_set)
            .dst_binding(0)
            .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
            .buffer_info(&buffer_info)];

        unsafe {
            ash_device.update_descriptor_sets(&writes, &[]);
        }

        Ok(Self {
            device: ash_device,

            uniform_buffer,

            descriptor_set_layout,
            descriptor_pool,
            descriptor_set,
        })
    }

    pub fn update(&self, camera: &CameraUniform) -> Result<(), VulkanCameraError> {
        self.uniform_buffer.write(std::slice::from_ref(camera))?;

        Ok(())
    }

    pub fn descriptor_set_layout(&self) -> vk::DescriptorSetLayout {
        self.descriptor_set_layout
    }

    pub fn descriptor_set(&self) -> vk::DescriptorSet {
        self.descriptor_set
    }
}

impl Drop for VulkanCamera {
    fn drop(&mut self) {
        unsafe {
            self.device
                .destroy_descriptor_pool(self.descriptor_pool, None);

            self.device
                .destroy_descriptor_set_layout(self.descriptor_set_layout, None);
        }
    }
}
