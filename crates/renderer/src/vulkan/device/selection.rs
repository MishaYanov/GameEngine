use std::ffi::CStr;

use ash::{Instance, khr::swapchain, vk};

use super::{QueueFamilies, VulkanDevice, VulkanDeviceError, VulkanSurface};

impl VulkanDevice {
	pub(crate) fn headless_physical_device(
		instance: &Instance,
	) -> Result<vk::PhysicalDevice, VulkanDeviceError> {
		unsafe { instance.enumerate_physical_devices()? }
			.into_iter()
			.filter(|device| find_graphics_queue_family(instance, *device).is_some())
			.max_by_key(|device| {
				let properties = unsafe { instance.get_physical_device_properties(*device) };

				score_device(&properties)
			})
			.ok_or(VulkanDeviceError::NoSuitablePhysicalDevice)
	}
}

pub(super) fn select_physical_device(
	instance: &Instance,
	surface: &VulkanSurface,
) -> Result<(vk::PhysicalDevice, QueueFamilies), VulkanDeviceError> {
	let devices = unsafe { instance.enumerate_physical_devices()? };

	let mut candidates = Vec::new();

	for physical_device in devices {
		let properties = unsafe { instance.get_physical_device_properties(physical_device) };
		if properties.api_version < vk::API_VERSION_1_3 {
			continue;
		}
		let mut dynamic_rendering = vk::PhysicalDeviceDynamicRenderingFeatures::default();
		let mut features = vk::PhysicalDeviceFeatures2::default().push_next(&mut dynamic_rendering);
		unsafe { instance.get_physical_device_features2(physical_device, &mut features) };
		let Some(score) =
			presentation_device_score(&properties, dynamic_rendering.dynamic_rendering)
		else {
			continue;
		};

		if !supports_swapchain(instance, physical_device)? {
			continue;
		}

		let Some(queue_families) = find_queue_families(instance, surface, physical_device)? else {
			continue;
		};

		/*
		 * VK_KHR_swapchain existing is not enough.
		 *
		 * We need at least one format and one
		 * presentation mode for this surface.
		 */
		if surface.formats(physical_device)?.is_empty() {
			continue;
		}

		if surface.present_modes(physical_device)?.is_empty() {
			continue;
		}

		candidates.push((score, physical_device, queue_families));
	}

	candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.0));

	candidates
		.into_iter()
		.next()
		.map(|(_, physical_device, queue_families)| (physical_device, queue_families))
		.ok_or(VulkanDeviceError::NoSuitablePhysicalDevice)
}

fn find_queue_families(
	instance: &Instance,
	surface: &VulkanSurface,
	physical_device: vk::PhysicalDevice,
) -> Result<Option<QueueFamilies>, VulkanDeviceError> {
	let properties =
		unsafe { instance.get_physical_device_queue_family_properties(physical_device) };

	let mut graphics = None;
	let mut present = None;

	for (index, queue_family) in properties.iter().enumerate() {
		let index = index as u32;

		if queue_family.queue_count == 0 {
			continue;
		}

		if queue_family.queue_flags.contains(vk::QueueFlags::GRAPHICS) {
			graphics = Some(index);
		}

		if surface.supports_presentation(physical_device, index)? {
			present = Some(index);
		}

		if graphics.is_some() && present.is_some() {
			break;
		}
	}

	Ok(match (graphics, present) {
		(Some(graphics), Some(present)) => Some(QueueFamilies { graphics, present }),

		_ => None,
	})
}

fn supports_swapchain(
	instance: &Instance,
	physical_device: vk::PhysicalDevice,
) -> Result<bool, VulkanDeviceError> {
	let extensions = unsafe { instance.enumerate_device_extension_properties(physical_device)? };

	Ok(extensions.iter().any(|extension| {
		let name = unsafe { CStr::from_ptr(extension.extension_name.as_ptr()) };

		name == swapchain::NAME
	}))
}

fn score_device(properties: &vk::PhysicalDeviceProperties) -> u32 {
	match properties.device_type {
		vk::PhysicalDeviceType::DISCRETE_GPU => 1000,

		vk::PhysicalDeviceType::INTEGRATED_GPU => 500,

		vk::PhysicalDeviceType::VIRTUAL_GPU => 250,

		vk::PhysicalDeviceType::CPU => 100,

		_ => 0,
	}
}

pub(super) fn presentation_device_score(
	properties: &vk::PhysicalDeviceProperties,
	dynamic_rendering: vk::Bool32,
) -> Option<u32> {
	(properties.api_version >= vk::API_VERSION_1_3 && dynamic_rendering == vk::TRUE)
		.then(|| score_device(properties))
}

pub(super) fn find_graphics_queue_family(
	instance: &Instance,
	physical_device: vk::PhysicalDevice,
) -> Option<u32> {
	let queue_families =
		unsafe { instance.get_physical_device_queue_family_properties(physical_device) };

	queue_families
		.iter()
		.enumerate()
		.find(|(_, family)| {
			family.queue_count > 0 && family.queue_flags.contains(vk::QueueFlags::GRAPHICS)
		})
		.map(|(index, _)| index as u32)
}
