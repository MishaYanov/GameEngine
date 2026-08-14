use std::{
	error::Error,
	fmt::{Display, Formatter},
};

use ash::{
	vk,
	Device,
	Instance,
};

pub struct VulkanDevice {
	device: Device,
	physical_device: vk::PhysicalDevice,

	graphics_queue: vk::Queue,
	graphics_queue_family: u32,
}

#[derive(Debug)]
pub enum VulkanDeviceError {
	NoSuitablePhysicalDevice,
	Vulkan(vk::Result),
}

impl Display for VulkanDeviceError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::NoSuitablePhysicalDevice => {
				write!(formatter, "no suitable Vulkan physical device found")
			}

			Self::Vulkan(error) => {
				write!(formatter, "Vulkan device error: {error:?}")
			}
		}
	}
}

impl Error for VulkanDeviceError {}

impl From<vk::Result> for VulkanDeviceError {
	fn from(value: vk::Result) -> Self {
		Self::Vulkan(value)
	}
}

impl VulkanDevice {
	pub fn new(
		instance: &Instance,
	) -> Result<Self, VulkanDeviceError> {
		let (physical_device, graphics_queue_family) =
			select_physical_device(instance)?;

		let queue_priorities = [1.0_f32];

		let queue_create_info = [
			vk::DeviceQueueCreateInfo::default()
				.queue_family_index(graphics_queue_family)
				.queue_priorities(&queue_priorities)
		];

		let device_create_info =
			vk::DeviceCreateInfo::default()
				.queue_create_infos(&queue_create_info);

		let device = unsafe {
			instance.create_device(
				physical_device,
				&device_create_info,
				None,
			)?
		};

		let graphics_queue = unsafe {
			device.get_device_queue(
				graphics_queue_family,
				0,
			)
		};

		Ok(Self {
			device,
			physical_device,
			graphics_queue,
			graphics_queue_family,
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

	pub fn graphics_queue_family(&self) -> u32 {
		self.graphics_queue_family
	}
}

impl Drop for VulkanDevice {
	fn drop(&mut self) {
		unsafe {
			self.device.destroy_device(None);
		}
	}
}

fn select_physical_device(
	instance: &Instance,
) -> Result<(vk::PhysicalDevice, u32), VulkanDeviceError> {
	let devices = unsafe {
		instance.enumerate_physical_devices()?
	};

	let mut candidates = devices
		.into_iter()
		.filter_map(|device| {
			let queue_family =
				find_graphics_queue_family(instance, device)?;

			let properties = unsafe {
				instance.get_physical_device_properties(device)
			};

			let score = score_device(&properties);

			Some((
				score,
				device,
				queue_family,
			))
		})
		.collect::<Vec<_>>();

	candidates.sort_by_key(|candidate| {
		std::cmp::Reverse(candidate.0)
	});

	candidates
		.into_iter()
		.next()
		.map(|(_, device, queue_family)| {
			(device, queue_family)
		})
		.ok_or(VulkanDeviceError::NoSuitablePhysicalDevice)
}

fn find_graphics_queue_family(
	instance: &Instance,
	physical_device: vk::PhysicalDevice,
) -> Option<u32> {
	let queue_families = unsafe {
		instance.get_physical_device_queue_family_properties(
			physical_device,
		)
	};

	queue_families
		.iter()
		.enumerate()
		.find(|(_, family)| {
			family.queue_count > 0
				&& family
				.queue_flags
				.contains(vk::QueueFlags::GRAPHICS)
		})
		.map(|(index, _)| index as u32)
}

fn score_device(
	properties: &vk::PhysicalDeviceProperties,
) -> u32 {
	match properties.device_type {
		vk::PhysicalDeviceType::DISCRETE_GPU => 1000,
		vk::PhysicalDeviceType::INTEGRATED_GPU => 500,
		vk::PhysicalDeviceType::VIRTUAL_GPU => 250,
		vk::PhysicalDeviceType::CPU => 100,

		_ => 0,
	}
}