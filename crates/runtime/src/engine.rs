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
#[path = "tests_engine.rs"]
mod tests;
