use std::{
	error::Error,
	ffi::CStr,
	fmt::{Display, Formatter},
};

use ash::{
	khr::swapchain,
	vk,
	Device,
	Instance,
};

use super::{
	VulkanSurface,
	VulkanSurfaceError,
};

#[derive(Debug, Clone, Copy)]
pub struct QueueFamilies {
	pub graphics: u32,
	pub present: u32,
}

pub struct VulkanDevice {
	device: Device,
	physical_device: vk::PhysicalDevice,

	graphics_queue: vk::Queue,
	present_queue: vk::Queue,

	queue_families: QueueFamilies,
}

#[derive(Debug)]
pub enum VulkanDeviceError {
	NoSuitablePhysicalDevice,
	Vulkan(vk::Result),
	Surface(VulkanSurfaceError),
}

impl Display for VulkanDeviceError {
	fn fmt(
		&self,
		formatter: &mut Formatter<'_>,
	) -> std::fmt::Result {
		match self {
			Self::NoSuitablePhysicalDevice => {
				write!(
					formatter,
					"no suitable Vulkan physical device found",
				)
			}

			Self::Vulkan(error) => {
				write!(
					formatter,
					"Vulkan device error: {error:?}",
				)
			}

			Self::Surface(error) => {
				write!(
					formatter,
					"Vulkan surface error: {error}",
				)
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

impl From<VulkanSurfaceError> for VulkanDeviceError {
	fn from(value: VulkanSurfaceError) -> Self {
		Self::Surface(value)
	}
}

impl VulkanDevice {

	pub fn new(
		instance: &Instance,
	) -> Result<Self, VulkanDeviceError> {
		let physical_device = unsafe {
			instance.enumerate_physical_devices()?
		}
			.into_iter()
			.max_by_key(|device| {
				let properties = unsafe {
					instance.get_physical_device_properties(*device)
				};

				score_device(&properties)
			})
			.ok_or(VulkanDeviceError::NoSuitablePhysicalDevice)?;

		let graphics_queue_family =
			find_graphics_queue_family(
				instance,
				physical_device,
			)
				.ok_or(
					VulkanDeviceError::NoSuitablePhysicalDevice,
				)?;

		let priorities = [1.0_f32];

		let queue_create_infos = [
			vk::DeviceQueueCreateInfo::default()
				.queue_family_index(
					graphics_queue_family,
				)
				.queue_priorities(&priorities),
		];

		let device_create_info =
			vk::DeviceCreateInfo::default()
				.queue_create_infos(
					&queue_create_infos,
				);

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
		let (
			physical_device,
			queue_families,
		) = select_physical_device(
			instance,
			surface,
		)?;

		let priority = [1.0_f32];

		/*
		 * Graphics and presentation may use the same queue
		 * family or two different families.
		 */
		let mut unique_families = vec![
			queue_families.graphics,
		];

		if queue_families.present
			!= queue_families.graphics
		{
			unique_families.push(
				queue_families.present,
			);
		}

		let queue_create_infos = unique_families
			.iter()
			.map(|family| {
				vk::DeviceQueueCreateInfo::default()
					.queue_family_index(*family)
					.queue_priorities(&priority)
			})
			.collect::<Vec<_>>();

		let device_extensions = [
			swapchain::NAME.as_ptr(),
		];

		let device_create_info =
			vk::DeviceCreateInfo::default()
				.queue_create_infos(
					&queue_create_infos,
				)
				.enabled_extension_names(
					&device_extensions,
				);

		let device = unsafe {
			instance.create_device(
				physical_device,
				&device_create_info,
				None,
			)?
		};

		let graphics_queue = unsafe {
			device.get_device_queue(
				queue_families.graphics,
				0,
			)
		};

		let present_queue = unsafe {
			device.get_device_queue(
				queue_families.present,
				0,
			)
		};

		Ok(Self {
			device,
			physical_device,
			graphics_queue,
			present_queue,
			queue_families,
		})
	}

	pub fn raw(&self) -> &Device {
		&self.device
	}

	pub fn physical_device(
		&self,
	) -> vk::PhysicalDevice {
		self.physical_device
	}

	pub fn graphics_queue(&self) -> vk::Queue {
		self.graphics_queue
	}

	pub fn present_queue(&self) -> vk::Queue {
		self.present_queue
	}

	pub fn queue_families(
		&self,
	) -> QueueFamilies {
		self.queue_families
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
	surface: &VulkanSurface,
) -> Result<
	(vk::PhysicalDevice, QueueFamilies),
	VulkanDeviceError,
> {
	let devices = unsafe {
		instance.enumerate_physical_devices()?
	};

	let mut candidates = Vec::new();

	for physical_device in devices {
		if !supports_swapchain(
			instance,
			physical_device,
		)? {
			continue;
		}

		let Some(queue_families) =
			find_queue_families(
				instance,
				surface,
				physical_device,
			)?
		else {
			continue;
		};

		/*
		 * VK_KHR_swapchain existing is not enough.
		 *
		 * We need at least one format and one
		 * presentation mode for this surface.
		 */
		if surface
			.formats(physical_device)?
			.is_empty()
		{
			continue;
		}

		if surface
			.present_modes(physical_device)?
			.is_empty()
		{
			continue;
		}

		let properties = unsafe {
			instance
				.get_physical_device_properties(
					physical_device,
				)
		};

		candidates.push((
			score_device(&properties),
			physical_device,
			queue_families,
		));
	}

	candidates.sort_by_key(|candidate| {
		std::cmp::Reverse(candidate.0)
	});

	candidates
		.into_iter()
		.next()
		.map(
			|(
				 _,
				 physical_device,
				 queue_families,
			 )| {
				(
					physical_device,
					queue_families,
				)
			},
		)
		.ok_or(
			VulkanDeviceError::
			NoSuitablePhysicalDevice,
		)
}

fn find_queue_families(
	instance: &Instance,
	surface: &VulkanSurface,
	physical_device: vk::PhysicalDevice,
) -> Result<Option<QueueFamilies>, VulkanDeviceError> {
	let properties = unsafe {
		instance
			.get_physical_device_queue_family_properties(
				physical_device,
			)
	};

	let mut graphics = None;
	let mut present = None;

	for (
		index,
		queue_family,
	) in properties.iter().enumerate() {
		let index = index as u32;

		if queue_family.queue_count == 0 {
			continue;
		}

		if queue_family
			.queue_flags
			.contains(
				vk::QueueFlags::GRAPHICS,
			)
		{
			graphics = Some(index);
		}

		if surface.supports_presentation(
			physical_device,
			index,
		)? {
			present = Some(index);
		}

		if graphics.is_some()
			&& present.is_some()
		{
			break;
		}
	}

	Ok(
		match (graphics, present) {
			(
				Some(graphics),
				Some(present),
			) => Some(
				QueueFamilies {
					graphics,
					present,
				},
			),

			_ => None,
		},
	)
}

fn supports_swapchain(
	instance: &Instance,
	physical_device: vk::PhysicalDevice,
) -> Result<bool, VulkanDeviceError> {
	let extensions = unsafe {
		instance
			.enumerate_device_extension_properties(
				physical_device,
			)?
	};

	Ok(
		extensions
			.iter()
			.any(|extension| {
				let name = unsafe {
					CStr::from_ptr(
						extension
							.extension_name
							.as_ptr(),
					)
				};

				name == swapchain::NAME
			}),
	)
}

fn score_device(
	properties: &vk::PhysicalDeviceProperties,
) -> u32 {
	match properties.device_type {
		vk::PhysicalDeviceType::DISCRETE_GPU => {
			1000
		}

		vk::PhysicalDeviceType::INTEGRATED_GPU => {
			500
		}

		vk::PhysicalDeviceType::VIRTUAL_GPU => {
			250
		}

		vk::PhysicalDeviceType::CPU => {
			100
		}

		_ => 0,
	}
}

fn find_graphics_queue_family(
	instance: &Instance,
	physical_device: vk::PhysicalDevice,
) -> Option<u32> {
	let queue_families = unsafe {
		instance
			.get_physical_device_queue_family_properties(
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
				.contains(
					vk::QueueFlags::GRAPHICS,
				)
		})
		.map(|(index, _)| index as u32)
}