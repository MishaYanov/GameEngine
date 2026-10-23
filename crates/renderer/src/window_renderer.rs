use std::sync::Arc;

use raw_window_handle::{HasDisplayHandle, HasWindowHandle};

use crate::vulkan::*;

trait WindowOwner: HasDisplayHandle + HasWindowHandle {}
impl<T: HasDisplayHandle + HasWindowHandle> WindowOwner for T {}

/// Owns a presentation renderer. Uploaded resources must be released before this owner.
/// Field order releases GPU dependants before the device, surface, instance and window.
pub struct WindowRenderer {
    targets: Option<RenderTargets>,
    camera: VulkanCamera,
    material_layout: Arc<VulkanMaterialLayout>,
    device: VulkanDevice,
    surface: VulkanSurface,
    instance: VulkanInstance,
    _window: Arc<dyn WindowOwner>,
    size: [u32; 2],
    dirty: bool,
}

struct RenderTargets {
    frame: VulkanFrame,
    pipelines: VulkanMaterialPipelines,
    depth: VulkanDepthBuffer,
    swapchain: VulkanSwapchain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawStatus {
    Rendered,
    Suspended,
    NeedsRedraw,
}

impl WindowRenderer {
	pub fn create_dynamic_mesh(
		&self,
		vertices: usize,
		indices: usize,
	) -> Result<VulkanMesh, String> {
		VulkanMesh::dynamic(self.instance.raw(), &self.device, vertices, indices)
			.map_err(|e| e.to_string())
	}

	pub fn write_dynamic_mesh(
		&self,
		mesh: &mut VulkanMesh,
		vertices: &[Vertex],
		indices: &[u32],
	) -> Result<(), String> {
		if let Some(targets) = &self.targets {
			targets.frame.wait_complete().map_err(|e| e.to_string())?;
		}
		mesh.write_dynamic(vertices, indices)
			.map_err(|e| e.to_string())
	}
    pub fn new<W: HasDisplayHandle + HasWindowHandle + 'static>(
        window: Arc<W>,
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        let display = window.display_handle().map_err(|e| e.to_string())?.as_raw();
        let handle = window.window_handle().map_err(|e| e.to_string())?.as_raw();
        let instance = VulkanInstance::for_display(display).map_err(|e| e.to_string())?;
        let surface = VulkanSurface::new(&instance, display, handle).map_err(|e| e.to_string())?;
        let device =
            VulkanDevice::for_surface(instance.raw(), &surface).map_err(|e| e.to_string())?;
        let camera = VulkanCamera::new(instance.raw(), &device).map_err(|e| e.to_string())?;
        let material_layout =
            Arc::new(VulkanMaterialLayout::new(device.raw()).map_err(|e| e.to_string())?);
        let mut renderer = Self {
            targets: None,
            camera,
            material_layout,
            device,
            surface,
            instance,
            _window: window,
            size: [width, height],
            dirty: true,
        };
        renderer.prepare()?;
        Ok(renderer)
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.size = [width, height];
        self.dirty = true;
    }

    /// Returns the actual surface extent, or None while minimized.
    pub fn prepare(&mut self) -> Result<Option<[u32; 2]>, String> {
        if self.size.contains(&0) {
            return Ok(None);
        }
        if self.dirty {
            self.wait_idle()?;
            // Retire the old targets before rebuilding. If creation fails, drawing
            // cannot accidentally reuse a swapchain retired by Vulkan.
            let old = self.targets.take();
            let swapchain = match old.as_ref() {
                Some(old) => VulkanSwapchain::recreate(
                    self.instance.raw(),
                    &self.device,
                    &self.surface,
                    self.size[0],
                    self.size[1],
                    &old.swapchain,
                ),
                None => VulkanSwapchain::new(
                    self.instance.raw(),
                    &self.device,
                    &self.surface,
                    self.size[0],
                    self.size[1],
                ),
            }
            .map_err(|e| e.to_string())?;
            let depth =
                VulkanDepthBuffer::new(self.instance.raw(), &self.device, swapchain.extent())
                    .map_err(|e| e.to_string())?;
            let pipelines = VulkanMaterialPipelines::new(
                self.device.raw(),
                swapchain.format(),
                depth.format(),
                self.camera.descriptor_set_layout(),
                self.material_layout.raw(),
            )?;
            let frame = VulkanFrame::new(&self.device, &swapchain).map_err(|e| e.to_string())?;
            self.targets = Some(RenderTargets {
                frame,
                pipelines,
                depth,
                swapchain,
            });
            self.dirty = false;
        }
        let extent = self
            .targets
            .as_ref()
            .expect("prepared render targets")
            .swapchain
            .extent();
        Ok(Some([extent.width, extent.height]))
    }

    pub fn draw(
        &mut self,
        camera: &CameraUniform,
        objects: &[RenderObject<'_>],
    ) -> Result<DrawStatus, String> {
        if self.prepare()?.is_none() {
            return Ok(DrawStatus::Suspended);
        }
        let targets = self.targets.as_mut().expect("prepared render targets");
        match targets
            .frame
            .draw(
                &self.device,
                &targets.swapchain,
                &targets.depth,
                &targets.pipelines,
                &self.camera,
                camera,
                objects,
            )
            .map_err(|e| e.to_string())?
        {
            FrameStatus::Rendered => Ok(DrawStatus::Rendered),
            FrameStatus::SwapchainNeedsRebuild => {
                self.dirty = true;
                Ok(DrawStatus::NeedsRedraw)
            }
        }
    }

    pub fn upload_mesh(
        &self,
        vertices: &[Vertex],
        indices: &[u32],
    ) -> Result<VulkanMesh, VulkanMeshError> {
        VulkanMesh::new(self.instance.raw(), &self.device, vertices, indices)
    }

    pub fn upload_rgba8(
        &self,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Result<VulkanTexture, VulkanTextureError> {
        VulkanTexture::from_rgba8(
            self.instance.raw(),
            &self.device,
            Arc::clone(&self.material_layout),
            width,
            height,
            pixels,
        )
    }

    /// Call before releasing external meshes/textures that may still be in flight.
    pub fn wait_idle(&self) -> Result<(), String> {
        unsafe { self.device.raw().device_wait_idle() }.map_err(|e| e.to_string())
    }
}

impl Drop for WindowRenderer {
    fn drop(&mut self) {
        if let Err(error) = self.wait_idle() {
            engine_logger::error!("Failed waiting for renderer shutdown: {}", error);
        }
    }
}
