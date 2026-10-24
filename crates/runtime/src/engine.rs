use std::{
	error::Error,
	fmt::{Display, Formatter},
};

use bridge::EngineStatusDto;
use renderer::{Renderer, RendererError};

pub struct GraphicsProbe {
	renderer: Renderer,
}

#[derive(Debug)]
pub enum GraphicsProbeError {
	Renderer(RendererError),
}

impl Display for GraphicsProbeError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Renderer(error) => {
				write!(formatter, "renderer initialization failed: {error}")
			}
		}
	}
}

impl Error for GraphicsProbeError {
	fn source(&self) -> Option<&(dyn Error + 'static)> {
		match self {
			Self::Renderer(error) => Some(error),
		}
	}
}

impl From<RendererError> for GraphicsProbeError {
	fn from(value: RendererError) -> Self {
		Self::Renderer(value)
	}
}

impl GraphicsProbe {
	/// Availability probe used by the editor setup screen before a viewport
	/// exists. No logical device or long-lived renderer is created.
	pub fn probe_status() -> Result<EngineStatusDto, GraphicsProbeError> {
		let renderer = Renderer::probe_info()?;
		Ok(EngineStatusDto {
			initialized: true,
			renderer: renderer.name,
			gpu_name: renderer.gpu_name,
			vulkan_api: renderer.vulkan_api,
		})
	}

	pub fn new() -> Result<Self, GraphicsProbeError> {
		engine_logger::debug!(target: "graphics_probe", "Creating graphics probe resources");
		let renderer = Renderer::new()?;
		engine_logger::debug!(target: "graphics_probe", "Graphics probe resources created");

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
