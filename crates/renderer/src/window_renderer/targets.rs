//! Resize, target replacement, and draw retry state.

use crate::vulkan::{
	FrameStatus, VulkanDepthBuffer, VulkanFrame, VulkanFrameError, VulkanMaterialPipelines,
	VulkanSwapchain,
};

use super::{DrawStatus, RenderTargets, WindowRenderer};

impl WindowRenderer {
	/// Returns the actual surface extent, or None while minimized.
	pub fn prepare(&mut self) -> Result<Option<[u32; 2]>, String> {
		if self.size.contains(&0) {
			return Ok(None);
		}
		if self.dirty {
			self.wait_idle()?;
			// Retire the old targets before rebuilding. If creation fails, drawing
			// cannot accidentally reuse a swapchain retired by Vulkan.
			let old = self.resources.targets.take();
			let swapchain = match old.as_ref() {
				Some(old) => VulkanSwapchain::recreate(
					self.resources.instance.raw(),
					&self.resources.device,
					&self.resources.surface,
					self.size[0],
					self.size[1],
					&old.swapchain,
				),
				None => VulkanSwapchain::new(
					self.resources.instance.raw(),
					&self.resources.device,
					&self.resources.surface,
					self.size[0],
					self.size[1],
				),
			}
			.map_err(|e| e.to_string())?;
			let depth = VulkanDepthBuffer::new(
				self.resources.instance.raw(),
				&self.resources.device,
				swapchain.extent(),
			)
			.map_err(|e| e.to_string())?;
			// Extent is dynamic pipeline state. Layouts belong to this renderer
			// for its entire lifetime; only attachment format changes invalidate it.
			if self.resources.pipelines.as_ref().is_none_or(|pipelines| {
				!pipelines.supports_formats(swapchain.format(), depth.format())
			}) {
				self.resources.pipelines = Some(VulkanMaterialPipelines::new(
					self.resources.device.raw(),
					swapchain.format(),
					depth.format(),
					self.resources.camera.descriptor_set_layout(),
					self.resources.material_layout.raw(),
				)?);
			}
			let frame = match old {
				Some(old)
					if old.swapchain.format() == swapchain.format()
						&& old.depth.format() == depth.format() =>
				{
					VulkanFrame::recreate(&self.resources.device, &swapchain, old.frame)
				}
				_ => VulkanFrame::new(&self.resources.device, &swapchain),
			}
			.map_err(|e| e.to_string())?;
			self.resources.targets = Some(RenderTargets {
				frame,
				depth,
				swapchain,
			});
			self.dirty = false;
		}
		let extent = self
			.resources
			.targets
			.as_ref()
			.expect("prepared render targets")
			.swapchain
			.extent();
		Ok(Some([extent.width, extent.height]))
	}

	pub fn resize(&mut self, width: u32, height: u32) {
		if self.size != [width, height] {
			self.size = [width, height];
			self.dirty = true;
		}
	}
}

pub(super) fn finish_draw(
	dirty: &mut bool,
	result: Result<FrameStatus, VulkanFrameError>,
) -> Result<DrawStatus, String> {
	match result {
		Ok(FrameStatus::Rendered) => Ok(DrawStatus::Rendered),
		Ok(FrameStatus::SwapchainNeedsRebuild) => {
			*dirty = true;
			Ok(DrawStatus::NeedsRedraw)
		}
		Err(error) => {
			// prepare() must retire the failed frame after device completion;
			// its acquisition semaphore or reset fence cannot safely be reused.
			*dirty = true;
			Err(error.to_string())
		}
	}
}
