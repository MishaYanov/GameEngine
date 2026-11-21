use std::{
	error::Error,
	ffi::CStr,
	fmt::{Display, Formatter},
	sync::Mutex,
};

use ash::{Device, Instance, khr::swapchain, vk};

use super::{VulkanSurface, VulkanSurfaceError};

mod selection;

use selection::{find_graphics_queue_family, select_physical_device};

#[derive(Debug, Clone, Copy)]
pub struct QueueFamilies {
	pub graphics: u32,
	pub present: u32,
}

pub struct VulkanDevice {
	_metrics: engine_logger::metrics::ResourceGuard,
	device: Device,
	physical_device: vk::PhysicalDevice,
	instance: vk::Instance,

	graphics_queue: vk::Queue,
	present_queue: vk::Queue,

	queue_families: QueueFamilies,
	memory_properties: vk::PhysicalDeviceMemoryProperties,
	timestamp_valid_bits: u32,
	timestamp_period: f32,
	pub(super) upload_retirements: Mutex<Vec<Box<dyn Send>>>,
}

#[derive(Debug)]
pub enum VulkanDeviceError {
	NoSuitablePhysicalDevice,
	Vulkan(vk::Result),
	Surface(VulkanSurfaceError),
}

impl Display for VulkanDeviceError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::NoSuitablePhysicalDevice => {
				write!(formatter, "no suitable Vulkan physical device found",)
			}

			Self::Vulkan(error) => {
				write!(formatter, "Vulkan device error: {error:?}",)
			}

			Self::Surface(error) => {
				write!(formatter, "Vulkan surface error: {error}",)
			}
		}
	}
}

impl Error for VulkanDeviceError {
	fn source(&self) -> Option<&(dyn Error + 'static)> {
		match self {
			Self::Vulkan(error) => Some(error),
			Self::Surface(error) => Some(error),
			_ => None,
		}
	}
}

impl From<vk::Result> for VulkanDeviceError {
	fn from(value: vk::Result) -> Self {
		Self::Vulkan(value)
	}
}

impl From<VulkanSurfaceError> for VulkanDeviceError {
	fn from(value: VulkanSurfaceError) -> Self {
		Self::Surface(value)
	}
}

impl VulkanDevice {
	#[cfg(test)]
	pub(super) fn for_test(device: Device) -> Self {
		let mut memory_properties = vk::PhysicalDeviceMemoryProperties {
			memory_type_count: 1,
			..Default::default()
		};
		memory_properties.memory_types[0].property_flags = vk::MemoryPropertyFlags::HOST_VISIBLE
			| vk::MemoryPropertyFlags::HOST_COHERENT
			| vk::MemoryPropertyFlags::DEVICE_LOCAL;
		Self {
			_metrics: engine_logger::metrics::ResourceGuard::disabled(),
			device,
			physical_device: vk::PhysicalDevice::null(),
			instance: vk::Instance::null(),
			graphics_queue: vk::Queue::null(),
			present_queue: vk::Queue::null(),
			queue_families: QueueFamilies {
				graphics: 0,
				present: 0,
			},
			memory_properties,
			timestamp_valid_bits: 0,
			timestamp_period: 0.0,
			upload_retirements: Mutex::new(Vec::new()),
		}
	}
	pub fn new(instance: &Instance) -> Result<Self, VulkanDeviceError> {
		let physical_device = Self::headless_physical_device(instance)?;

		let graphics_queue_family = find_graphics_queue_family(instance, physical_device)
			.ok_or(VulkanDeviceError::NoSuitablePhysicalDevice)?;

		let priorities = [1.0_f32];

		let queue_create_infos = [vk::DeviceQueueCreateInfo::default()
			.queue_family_index(graphics_queue_family)
			.queue_priorities(&priorities)];

		let device_create_info =
			vk::DeviceCreateInfo::default().queue_create_infos(&queue_create_infos);

		let device = unsafe { instance.create_device(physical_device, &device_create_info, None)? };

		let graphics_queue = unsafe { device.get_device_queue(graphics_queue_family, 0) };

		let gpu_properties = unsafe { instance.get_physical_device_properties(physical_device) };
		let gpu_name = unsafe { CStr::from_ptr(gpu_properties.device_name.as_ptr()) }
			.to_string_lossy()
			.into_owned();
		let metrics = crate::metrics::register(&device, &gpu_name);
		Ok(Self {
			upload_retirements: Mutex::new(Vec::new()),
			_metrics: metrics,
			device,
			physical_device,
			instance: instance.handle(),
			memory_properties: unsafe {
				instance.get_physical_device_memory_properties(physical_device)
			},
			timestamp_valid_bits: unsafe {
				instance.get_physical_device_queue_family_properties(physical_device)
			}[graphics_queue_family as usize]
				.timestamp_valid_bits,
			timestamp_period: unsafe { instance.get_physical_device_properties(physical_device) }
				.limits
				.timestamp_period,

			graphics_queue,

			/*
			 * Headless path has no presentation surface.
			 *
			 * We use the graphics queue as the stored
			 * presentation queue for now, but it must
			 * never actually be used for presentation.
			 */
			present_queue: graphics_queue,

			queue_families: QueueFamilies {
				graphics: graphics_queue_family,
				present: graphics_queue_family,
			},
		})
	}
	pub fn for_surface(
		instance: &Instance,
		surface: &VulkanSurface,
	) -> Result<Self, VulkanDeviceError> {
		let (physical_device, queue_families) = select_physical_device(instance, surface)?;

		let priority = [1.0_f32];

		let mut unique_families = vec![queue_families.graphics];

		if queue_families.present != queue_families.graphics {
			unique_families.push(queue_families.present);
		}

		let queue_create_infos = unique_families
			.iter()
			.map(|family| {
				vk::DeviceQueueCreateInfo::default()
					.queue_family_index(*family)
					.queue_priorities(&priority)
			})
			.collect::<Vec<_>>();

		let device_extensions = [swapchain::NAME.as_ptr()];

		let mut dynamic_rendering_features =
			vk::PhysicalDeviceDynamicRenderingFeatures::default().dynamic_rendering(true);

		let create_info = vk::DeviceCreateInfo::default()
			.queue_create_infos(&queue_create_infos)
			.enabled_extension_names(&device_extensions)
			.push_next(&mut dynamic_rendering_features);

		let device = unsafe { instance.create_device(physical_device, &create_info, None)? };

		let graphics_queue = unsafe { device.get_device_queue(queue_families.graphics, 0) };

		let present_queue = unsafe { device.get_device_queue(queue_families.present, 0) };

		let gpu_properties = unsafe { instance.get_physical_device_properties(physical_device) };
		let gpu_name = unsafe { CStr::from_ptr(gpu_properties.device_name.as_ptr()) }
			.to_string_lossy()
			.into_owned();
		let metrics = crate::metrics::register(&device, &gpu_name);
		Ok(Self {
			upload_retirements: Mutex::new(Vec::new()),
			_metrics: metrics,
			device,
			physical_device,
			instance: instance.handle(),
			memory_properties: unsafe {
				instance.get_physical_device_memory_properties(physical_device)
			},
			timestamp_valid_bits: unsafe {
				instance.get_physical_device_queue_family_properties(physical_device)
			}[queue_families.graphics as usize]
				.timestamp_valid_bits,
			timestamp_period: unsafe { instance.get_physical_device_properties(physical_device) }
				.limits
				.timestamp_period,
			graphics_queue,
			present_queue,
			queue_families,
		})
	}

	pub fn raw(&self) -> &Device {
		&self.device
	}

	pub fn physical_device(&self) -> vk::PhysicalDevice {
		self.physical_device
	}

	pub fn graphics_queue(&self) -> vk::Queue {
		self.graphics_queue
	}

	pub fn present_queue(&self) -> vk::Queue {
		self.present_queue
	}

	pub fn queue_families(&self) -> QueueFamilies {
		self.queue_families
	}

	pub(crate) fn memory_properties(&self) -> &vk::PhysicalDeviceMemoryProperties {
		&self.memory_properties
	}

	pub(crate) fn timestamp_properties(&self) -> Option<(u32, f32)> {
		(self.timestamp_valid_bits > 0
			&& self.timestamp_period.is_finite()
			&& self.timestamp_period > 0.0)
			.then_some((self.timestamp_valid_bits.min(64), self.timestamp_period))
	}
}

impl Drop for VulkanDevice {
	fn drop(&mut self) {
		let retired = self
			.upload_retirements
			.get_mut()
			.unwrap_or_else(|error| error.into_inner());
		if !retired.is_empty() {
			match unsafe { self.device.device_wait_idle() } {
				Ok(()) | Err(vk::Result::ERROR_DEVICE_LOST) => retired.clear(),
				Err(error) => {
					// Completion could not be established. Releasing these allocations
					// (or their device) would free resources still used by the GPU.
					engine_logger::error!(
						"Retaining Vulkan device and upload resources after unrecoverable wait failure: {error:?}"
					);
					std::mem::forget(std::mem::take(retired));
					super::instance::retain_parent(self.instance);
					return;
				}
			}
		}
		unsafe {
			self.device.destroy_device(None);
		}
		crate::metrics::finish(&self.device);
	}
}

#[cfg(test)]
#[path = "../../tests/unit/vulkan/device.rs"]
mod tests;
