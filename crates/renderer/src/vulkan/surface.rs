use std::{
    error::Error,
    fmt::{Display, Formatter},
};

use ash::{khr::surface, vk};

use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

use super::VulkanInstance;

pub struct VulkanSurface {
    loader: surface::Instance,
    surface: vk::SurfaceKHR,
}

#[derive(Debug)]
pub enum VulkanSurfaceError {
    Vulkan(vk::Result),
}

impl Display for VulkanSurfaceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Vulkan(error) => {
                write!(formatter, "Vulkan surface error: {error:?}")
            }
        }
    }
}

impl Error for VulkanSurfaceError {}

impl From<vk::Result> for VulkanSurfaceError {
    fn from(value: vk::Result) -> Self {
        Self::Vulkan(value)
    }
}

impl VulkanSurface {
    pub fn new(
        instance: &VulkanInstance,
        display_handle: RawDisplayHandle,
        window_handle: RawWindowHandle,
    ) -> Result<Self, VulkanSurfaceError> {
        let loader = surface::Instance::new(instance.entry(), instance.raw());

        let surface = unsafe {
            ash_window::create_surface(
                instance.entry(),
                instance.raw(),
                display_handle,
                window_handle,
                None,
            )?
        };

        Ok(Self { loader, surface })
    }

    pub fn raw(&self) -> vk::SurfaceKHR {
        self.surface
    }

    pub fn supports_presentation(
        &self,
        physical_device: vk::PhysicalDevice,
        queue_family_index: u32,
    ) -> Result<bool, VulkanSurfaceError> {
        let supported = unsafe {
            self.loader.get_physical_device_surface_support(
                physical_device,
                queue_family_index,
                self.surface,
            )?
        };

        Ok(supported)
    }

    pub fn formats(
        &self,
        physical_device: vk::PhysicalDevice,
    ) -> Result<Vec<vk::SurfaceFormatKHR>, VulkanSurfaceError> {
        let formats = unsafe {
            self.loader
                .get_physical_device_surface_formats(physical_device, self.surface)?
        };

        Ok(formats)
    }

    pub fn present_modes(
        &self,
        physical_device: vk::PhysicalDevice,
    ) -> Result<Vec<vk::PresentModeKHR>, VulkanSurfaceError> {
        let modes = unsafe {
            self.loader
                .get_physical_device_surface_present_modes(physical_device, self.surface)?
        };

        Ok(modes)
    }

    pub fn capabilities(
        &self,
        physical_device: vk::PhysicalDevice,
    ) -> Result<vk::SurfaceCapabilitiesKHR, VulkanSurfaceError> {
        let capabilities = unsafe {
            self.loader
                .get_physical_device_surface_capabilities(physical_device, self.surface)?
        };

        Ok(capabilities)
    }
}

impl Drop for VulkanSurface {
    fn drop(&mut self) {
        unsafe {
            self.loader.destroy_surface(self.surface, None);
        }
    }
}
