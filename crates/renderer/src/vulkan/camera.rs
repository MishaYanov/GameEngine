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

use super::{LightingUniform, VulkanBuffer, VulkanBufferError, VulkanDevice};

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CameraUniform {
	pub view_projection: [f32; 16],
}

pub struct VulkanCamera {
	_metrics: Vec<engine_logger::metrics::ResourceGuard>,
	device: Device,

	uniform_buffer: VulkanBuffer,
	lighting_buffer: VulkanBuffer,

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

impl Error for VulkanCameraError {
	fn source(&self) -> Option<&(dyn Error + 'static)> {
		match self {
			Self::Vulkan(error) => Some(error),
			Self::Buffer(error) => Some(error),
		}
	}
}

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
	pub fn from_view_projection(matrix: glam::Mat4) -> Self {
		Self {
			view_projection: matrix.to_cols_array(),
		}
	}

	pub fn orthographic(
		position: [f32; 3],
		target: [f32; 3],
		up: [f32; 3],
		vertical_height: f32,
		aspect_ratio: f32,
		near: f32,
		far: f32,
	) -> Self {
		let half_height = vertical_height * 0.5;
		let half_width = half_height * aspect_ratio;
		let projection = glam::camera::rh::proj::vulkan::orthographic(
			-half_width,
			half_width,
			-half_height,
			half_height,
			near,
			far,
		);
		let view = look_at_mat4(
			Vec3::from_array(position),
			Vec3::from_array(target),
			Vec3::from_array(up).normalize(),
		);
		Self::from_view_projection(projection * view)
	}

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
	#[cfg(test)]
	pub(super) fn for_test(device: &VulkanDevice) -> Self {
		Self {
			_metrics: Vec::new(),
			device: device.raw().clone(),
			uniform_buffer: VulkanBuffer::host_vertex(device, size_of::<CameraUniform>() as u64)
				.unwrap(),
			lighting_buffer: VulkanBuffer::host_vertex(device, size_of::<LightingUniform>() as u64)
				.unwrap(),
			descriptor_set_layout: vk::DescriptorSetLayout::null(),
			descriptor_pool: vk::DescriptorPool::null(),
			descriptor_set: vk::DescriptorSet::null(),
		}
	}
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

		let lighting_buffer = VulkanBuffer::new(
			instance,
			device,
			size_of::<LightingUniform>() as u64,
			vk::BufferUsageFlags::UNIFORM_BUFFER,
			vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
		)?;
		lighting_buffer.write(&[LightingUniform::default()])?;

		let bindings = [
			vk::DescriptorSetLayoutBinding::default()
				.binding(0)
				.descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
				.descriptor_count(1)
				.stage_flags(vk::ShaderStageFlags::VERTEX),
			vk::DescriptorSetLayoutBinding::default()
				.binding(1)
				.descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
				.descriptor_count(1)
				.stage_flags(vk::ShaderStageFlags::FRAGMENT),
		];

		let layout_info = vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings);

		let descriptor_set_layout =
			unsafe { ash_device.create_descriptor_set_layout(&layout_info, None)? };

		let pool_sizes = [vk::DescriptorPoolSize {
			ty: vk::DescriptorType::UNIFORM_BUFFER,

			descriptor_count: 2,
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

		let lighting_info = [vk::DescriptorBufferInfo::default()
			.buffer(lighting_buffer.raw())
			.offset(0)
			.range(size_of::<LightingUniform>() as u64)];
		let writes = [
			vk::WriteDescriptorSet::default()
				.dst_set(descriptor_set)
				.dst_binding(0)
				.descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
				.buffer_info(&buffer_info),
			vk::WriteDescriptorSet::default()
				.dst_set(descriptor_set)
				.dst_binding(1)
				.descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
				.buffer_info(&lighting_info),
		];

		unsafe {
			ash_device.update_descriptor_sets(&writes, &[]);
		}

		Ok(Self {
			_metrics: vec![
				crate::metrics::object(
					&ash_device,
					"descriptor_set_layout",
					descriptor_set_layout,
					"Camera/lighting layout",
					"",
				),
				crate::metrics::object(
					&ash_device,
					"descriptor_pool",
					descriptor_pool,
					"Camera/lighting descriptors",
					"max_sets=1",
				),
				crate::metrics::object(
					&ash_device,
					"descriptor_set",
					descriptor_set,
					"Camera/lighting descriptor",
					"",
				),
			],
			device: ash_device,

			uniform_buffer,
			lighting_buffer,

			descriptor_set_layout,
			descriptor_pool,
			descriptor_set,
		})
	}

	pub fn update(&self, camera: &CameraUniform) -> Result<(), VulkanCameraError> {
		self.uniform_buffer.write(std::slice::from_ref(camera))?;

		Ok(())
	}

	/// The caller must finish the previous frame before updating uniforms.
	pub(crate) fn update_lighting(
		&self,
		lighting: &LightingUniform,
	) -> Result<(), VulkanCameraError> {
		self.lighting_buffer.write(std::slice::from_ref(lighting))?;
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

#[cfg(test)]
mod projection_tests {
	use super::*;
	#[test]
	fn orthographic_camera_keeps_parallel_size_and_vulkan_depth_range() {
		let camera = CameraUniform::orthographic(
			[0.0, 0.0, 10.0],
			[0.0; 3],
			[0.0, 1.0, 0.0],
			8.0,
			2.0,
			0.1,
			100.0,
		);
		let matrix = glam::Mat4::from_cols_array(&camera.view_projection);
		let near = matrix * glam::Vec4::new(0.0, 0.0, 9.9, 1.0);
		let far = matrix * glam::Vec4::new(0.0, 0.0, -90.0, 1.0);
		assert!((near.z / near.w).abs() < 1e-5);
		assert!((far.z / far.w - 1.0).abs() < 1e-5);
		let front = matrix * glam::Vec4::new(1.0, 1.0, 0.0, 1.0);
		let back = matrix * glam::Vec4::new(1.0, 1.0, -10.0, 1.0);
		assert!((front.x / front.w - back.x / back.w).abs() < 1e-6);
		assert!((front.y / front.w - back.y / back.w).abs() < 1e-6);
		assert!(
			front.y < 0.0,
			"Vulkan projection flips Y for a positive-height viewport"
		);
	}
}
