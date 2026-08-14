use std::{
	error::Error,
	ffi::CStr,
	fmt::{Display, Formatter},
};

use ash::vk;

use crate::vulkan::{
	format_api_version,
	VulkanDevice,
	VulkanDeviceError,
	VulkanInitError,
	VulkanInstance,
};

pub struct Renderer {
	device: VulkanDevice,
	instance: VulkanInstance,
}

#[derive(Debug, Clone)]
pub struct RendererInfo {
	pub name: String,
	pub gpu_name: String,
	pub vulkan_api: String,
}

#[derive(Debug)]
pub enum RendererError {
	Instance(VulkanInitError),
	Device(VulkanDeviceError),
}

impl Display for RendererError {
	fn fmt(
		&self,
		formatter: &mut Formatter<'_>,
	) -> std::fmt::Result {
		match self {
			Self::Instance(error) => {
				write!(formatter, "Vulkan instance error: {error}")
			}

			Self::Device(error) => {
				write!(formatter, "Vulkan device error: {error}")
			}
		}
	}
}

impl Error for RendererError {}

impl From<VulkanInitError> for RendererError {
	fn from(value: VulkanInitError) -> Self {
		Self::Instance(value)
	}
}

impl From<VulkanDeviceError> for RendererError {
	fn from(value: VulkanDeviceError) -> Self {
		Self::Device(value)
	}
}

impl Renderer {
	pub fn new() -> Result<Self, RendererError> {
		let instance = VulkanInstance::new()?;
		let device = VulkanDevice::new(instance.raw())?;

		Ok(Self {
			device,
			instance,
		})
	}

	pub fn info(&self) -> RendererInfo {
		let properties = unsafe {
			self.instance
				.raw()
				.get_physical_device_properties(
					self.device.physical_device(),
				)
		};

		let gpu_name = unsafe {
			CStr::from_ptr(properties.device_name.as_ptr())
		}
			.to_string_lossy()
			.into_owned();

		RendererInfo {
			name: "Vulkan".to_string(),
			gpu_name,
			vulkan_api: format_api_version(
				properties.api_version,
			),
		}
	}
}