use std::{
    error::Error,
    ffi::CStr,
    fmt::{Display, Formatter},
};

use crate::vulkan::{
    VulkanDevice, VulkanDeviceError, VulkanInitError, VulkanInstance, format_api_version,
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
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
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

impl Error for RendererError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Instance(error) => Some(error),
            Self::Device(error) => Some(error),
        }
    }
}

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
        engine_logger::info!("Initializing Vulkan renderer");
        let instance = VulkanInstance::new()?;
        let device = VulkanDevice::new(instance.raw())?;
        engine_logger::info!("Vulkan renderer initialized");
        engine_logger::component_health(
            "renderer",
            "HEALTHY",
            false,
            "Vulkan renderer initialized",
        );

        Ok(Self { device, instance })
    }

    pub fn info(&self) -> RendererInfo {
        let properties = unsafe {
            self.instance
                .raw()
                .get_physical_device_properties(self.device.physical_device())
        };

        let gpu_name = unsafe { CStr::from_ptr(properties.device_name.as_ptr()) }
            .to_string_lossy()
            .into_owned();

        RendererInfo {
            name: "Vulkan".to_string(),
            gpu_name,
            vulkan_api: format_api_version(properties.api_version),
        }
    }
}
