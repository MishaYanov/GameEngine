use std::{
    error::Error,
    fmt::{Display, Formatter},
};

use bridge::EngineStatusDto;
use renderer::{Renderer, RendererError};

pub struct Engine {
    renderer: Renderer,
}

#[derive(Debug)]
pub enum EngineError {
    Renderer(RendererError),
}

impl Display for EngineError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Renderer(error) => {
                write!(formatter, "renderer initialization failed: {error}")
            }
        }
    }
}

impl Error for EngineError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Renderer(error) => Some(error),
        }
    }
}

impl From<RendererError> for EngineError {
    fn from(value: RendererError) -> Self {
        Self::Renderer(value)
    }
}

impl Engine {
    pub fn new() -> Result<Self, EngineError> {
        engine_logger::info!("Initializing engine runtime");
        let renderer = Renderer::new()?;
        engine_logger::info!("Engine runtime initialized");
        engine_logger::component_health("runtime", "HEALTHY", false, "Engine runtime initialized");

        Ok(Self { renderer })
    }

    pub fn status(&self) -> EngineStatusDto {
        let renderer = self.renderer.info();

        EngineStatusDto {
            initialized: true,
            renderer: renderer.name,
            gpu_name: renderer.gpu_name,
            vulkan_api: renderer.vulkan_api,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use renderer::vulkan::VulkanInitError;

    #[test]
    fn initialization_errors_preserve_the_root_cause() {
        let error = EngineError::from(RendererError::from(VulkanInitError::NoPhysicalDevices));
        assert!(error.to_string().contains("renderer initialization failed"));
        let renderer = error.source().unwrap();
        assert!(renderer.to_string().contains("Vulkan instance error"));
        assert_eq!(
            renderer.source().unwrap().to_string(),
            "no Vulkan-capable physical devices found"
        );
    }

    #[test]
    #[ignore = "requires a Vulkan 1.3 loader, GPU and debug validation layer"]
    fn native_runtime_initializes_and_reports_consistent_status() {
        let engine = Engine::new().expect("native runtime initialization");
        for _ in 0..2 {
            let status = engine.status();
            assert!(status.initialized);
            assert_eq!(status.renderer, "Vulkan");
            assert!(!status.gpu_name.trim().is_empty());
            assert_eq!(status.vulkan_api.split('.').count(), 3);
        }
    }
}
