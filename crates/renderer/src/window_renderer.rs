use std::{
	mem::ManuallyDrop,
	sync::{
		Arc,
		atomic::{AtomicBool, Ordering},
	},
};

use raw_window_handle::{HasDisplayHandle, HasWindowHandle};

use crate::vulkan::*;

mod targets;

use targets::finish_draw;

trait WindowOwner: HasDisplayHandle + HasWindowHandle {}
impl<T: HasDisplayHandle + HasWindowHandle> WindowOwner for T {}

struct RendererResources {
	max_viewport_dimensions: [u32; 2],
	viewport_bounds: [f32; 2],
	targets: Option<RenderTargets>,
	editor: Option<crate::vulkan::editor::EditorPass>,
	pipelines: Option<VulkanMaterialPipelines>,
	camera: VulkanCamera,
	material_layout: Arc<VulkanMaterialLayout>,
	device: VulkanDevice,
	surface: VulkanSurface,
	instance: VulkanInstance,
	_window: Arc<dyn WindowOwner>,
}

struct RenderTargets {
	frame: VulkanFrame,
	depth: VulkanDepthBuffer,
	swapchain: VulkanSwapchain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawStatus {
	Rendered,
	Suspended,
	NeedsRedraw,
}

/// Owns a presentation renderer. Uploaded resources must be released before this owner.
/// Field order releases GPU dependants before the device, surface, instance and window.
pub struct WindowRenderer {
	// Vulkan dispatch tables make this ownership tree large. Heap ownership
	// keeps debug constructor/move temporaries off the native main stack while
	// retaining the same ordered shutdown and uncertain-completion policy.
	resources: ManuallyDrop<Box<RendererResources>>,
	size: [u32; 2],
	dirty: bool,
	lighting: LightingUniform,
	editor_visuals: crate::EditorVisuals,
	external_resources_retained: AtomicBool,
}

impl WindowRenderer {
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
		let limits = unsafe {
			instance
				.raw()
				.get_physical_device_properties(device.physical_device())
		}
		.limits;
		let material_layout =
			Arc::new(VulkanMaterialLayout::new(device.raw()).map_err(|e| e.to_string())?);
		let mut renderer = Self {
			resources: ManuallyDrop::new(Box::new(RendererResources {
				max_viewport_dimensions: limits.max_viewport_dimensions,
				viewport_bounds: limits.viewport_bounds_range,
				targets: None,
				editor: None,
				pipelines: None,
				camera,
				material_layout,
				device,
				surface,
				instance,
				_window: window,
			})),
			size: [width, height],
			dirty: true,
			lighting: LightingUniform::default(),
			editor_visuals: crate::EditorVisuals::default(),
			external_resources_retained: AtomicBool::new(false),
		};
		renderer.prepare()?;
		Ok(renderer)
	}

	/// The physical device actually selected for this presentation surface.
	pub fn physical_device_info(&self) -> PhysicalDeviceInfo {
		let p = unsafe {
			self.resources
				.instance
				.raw()
				.get_physical_device_properties(self.resources.device.physical_device())
		};
		PhysicalDeviceInfo {
			name: unsafe { std::ffi::CStr::from_ptr(p.device_name.as_ptr()) }
				.to_string_lossy()
				.into_owned(),
			device_type: p.device_type,
			api_version: p.api_version,
			driver_version: p.driver_version,
			vendor_id: p.vendor_id,
			device_id: p.device_id,
		}
	}
	/// Upload a bounded group with one submission. Results preserve input order
	/// and are ready to draw on return; staging is retained until completion.
	pub fn upload_batch(
		&self,
		meshes: &[crate::MeshUpload<'_>],
		textures: &[crate::Rgba8Upload<'_>],
	) -> Result<crate::UploadedBatch, String> {
		crate::vulkan::upload_batch::upload(
			self.resources.instance.raw(),
			&self.resources.device,
			&self.resources.material_layout,
			meshes,
			textures,
		)
	}
	/// Exact instrumented allocation sizes, including alignment and staging,
	/// with driver-owned presentation memory explicitly excluded.
	pub fn resource_metrics(&self) -> Option<engine_logger::metrics::MetricsSnapshot> {
		crate::metrics::snapshot(self.resources.device.raw())
	}
	pub fn create_dynamic_mesh(
		&self,
		vertices: usize,
		indices: usize,
	) -> Result<VulkanMesh, String> {
		VulkanMesh::dynamic(
			self.resources.instance.raw(),
			&self.resources.device,
			vertices,
			indices,
		)
		.map_err(|e| e.to_string())
	}

	pub fn write_dynamic_mesh(
		&self,
		mesh: &mut VulkanMesh,
		vertices: &[Vertex],
		indices: &[u32],
	) -> Result<(), String> {
		if let Some(targets) = &self.resources.targets {
			targets.frame.wait_complete().map_err(|e| e.to_string())?;
		}
		mesh.write_dynamic(vertices, indices)
			.map_err(|e| e.to_string())
	}
	/// Validated CPU state; upload occurs after the previous frame completes.
	pub fn set_lights(&mut self, lights: &[Light]) -> Result<(), String> {
		self.lighting = LightingUniform::from_lights(lights)?;
		Ok(())
	}

	/// Editor-only presentation; no scene entities/materials are created.
	pub fn set_editor_visuals(&mut self, visuals: &crate::EditorVisuals) -> Result<(), String> {
		visuals.validate()?;
		self.editor_visuals.grid = visuals.grid;
		self.editor_visuals.lines.clone_from(&visuals.lines);
		Ok(())
	}

	pub fn last_metrics(&self) -> DrawMetrics {
		self.resources
			.targets
			.as_ref()
			.map(|targets| targets.frame.last_metrics())
			.unwrap_or_default()
	}

	pub fn draw(
		&mut self,
		camera: &CameraUniform,
		objects: &[RenderObject<'_>],
	) -> Result<DrawStatus, String> {
		self.draw_region(camera, objects, None)
	}

	/// Render into physical [x, y, width, height]. Screen-space geometry uses content-local coordinates.
	pub fn draw_in_rect(
		&mut self,
		camera: &CameraUniform,
		objects: &[RenderObject<'_>],
		pixels: [u32; 4],
	) -> Result<DrawStatus, String> {
		self.draw_region(camera, objects, Some(pixels))
	}

	fn draw_region(
		&mut self,
		camera: &CameraUniform,
		objects: &[RenderObject<'_>],
		pixels: Option<[u32; 4]>,
	) -> Result<DrawStatus, String> {
		let Some(extent) = self.prepare()? else {
			return Ok(DrawStatus::Suspended);
		};
		let resources = &mut **self.resources;
		let region = validated_region(
			pixels.unwrap_or([0, 0, extent[0], extent[1]]),
			extent,
			resources.max_viewport_dimensions,
			resources.viewport_bounds,
		)?;
		let targets = resources.targets.as_mut().expect("prepared render targets");
		let editor = if self.editor_visuals.is_empty() {
			None
		} else {
			if resources.editor.as_ref().is_none_or(|pass| {
				!pass.supports_formats(targets.swapchain.format(), targets.depth.format())
			}) {
				resources.editor = Some(crate::vulkan::editor::EditorPass::new(
					&resources.device,
					targets.swapchain.format(),
					targets.depth.format(),
				)?);
			}
			let pass = resources.editor.as_mut().expect("editor pass initialized");
			Some(crate::vulkan::frame::FrameEditor {
				pass,
				visuals: &self.editor_visuals,
			})
		};
		targets.frame.set_lighting(self.lighting);
		let result = targets.frame.draw_with_editor(
			&resources.device,
			FrameTargets {
				swapchain: &targets.swapchain,
				depth_buffer: &targets.depth,
			},
			resources.pipelines.as_ref().expect("prepared pipelines"),
			&resources.camera,
			objects,
			crate::vulkan::frame::FrameView {
				uniform: camera,
				region,
				editor,
			},
		);
		finish_draw(&mut self.dirty, result)
	}

	pub fn upload_mesh(
		&self,
		vertices: &[Vertex],
		indices: &[u32],
	) -> Result<VulkanMesh, VulkanMeshError> {
		VulkanMesh::new(
			self.resources.instance.raw(),
			&self.resources.device,
			vertices,
			indices,
		)
	}

	pub fn upload_skinned_mesh(
		&self,
		vertices: &[Vertex],
		indices: &[u32],
		influences: &[crate::vulkan::mesh::SkinInfluence],
	) -> Result<VulkanMesh, VulkanMeshError> {
		VulkanMesh::new_skinned(
			self.resources.instance.raw(),
			&self.resources.device,
			vertices,
			indices,
			influences,
		)
	}

	pub fn upload_rgba8(
		&self,
		width: u32,
		height: u32,
		pixels: &[u8],
	) -> Result<VulkanTexture, VulkanTextureError> {
		VulkanTexture::from_rgba8(
			self.resources.instance.raw(),
			&self.resources.device,
			Arc::clone(&self.resources.material_layout),
			width,
			height,
			pixels,
		)
	}

	/// Call before releasing external meshes/textures that may still be in flight.
	pub fn wait_idle(&self) -> Result<(), String> {
		self.wait_idle_raw().map_err(|e| e.to_string())
	}

	/// Release external GPU owners after completion or device loss. If completion
	/// cannot be established, retain both these resources and the renderer's
	/// device/window ownership tree, even if a later wait succeeds.
	pub fn retire_resources<T: 'static>(&self, resources: T) -> Result<(), String> {
		retire_external(
			resources,
			self.wait_idle_raw(),
			&self.external_resources_retained,
		)
		.map_err(|error| error.to_string())
	}

	fn wait_idle_raw(&self) -> Result<(), ash::vk::Result> {
		if let Some(targets) = &self.resources.targets {
			targets.frame.wait_complete_raw()?;
		}
		unsafe { self.resources.device.raw().device_wait_idle() }
	}
}

fn validated_region(
	[x, y, width, height]: [u32; 4],
	extent: [u32; 2],
	max_dimensions: [u32; 2],
	bounds: [f32; 2],
) -> Result<ash::vk::Rect2D, String> {
	for (origin, size, limit, max_size) in [
		(x, width, extent[0], max_dimensions[0]),
		(y, height, extent[1], max_dimensions[1]),
	] {
		if size == 0
			|| size > max_size
			|| origin
				.checked_add(size)
				.is_none_or(|end| end > limit.min(i32::MAX as u32))
			|| (origin as f32) < bounds[0]
			|| (origin as f32 + size as f32) > bounds[1]
		{
			return Err(
				"Render rectangle exceeds the prepared attachment or device viewport limits".into(),
			);
		}
	}
	Ok(ash::vk::Rect2D {
		offset: ash::vk::Offset2D {
			x: x as i32,
			y: y as i32,
		},
		extent: ash::vk::Extent2D { width, height },
	})
}

impl Drop for WindowRenderer {
	fn drop(&mut self) {
		let completed = self.wait_idle_raw();
		// Drop is the only path that takes this owner; the field cannot drop again.
		let resources = unsafe { ManuallyDrop::take(&mut self.resources) };
		if *self.external_resources_retained.get_mut() {
			std::mem::forget(resources);
			return;
		}
		if let Err(error) = release_after_idle(resources, completed) {
			engine_logger::error!(
				"Retaining renderer resources after unrecoverable shutdown wait failure: {error:?}"
			);
		}
	}
}

fn retire_external<T>(
	resources: T,
	completed: Result<(), ash::vk::Result>,
	retained: &AtomicBool,
) -> Result<(), ash::vk::Result> {
	if let Err(error) = release_after_idle(resources, completed) {
		retained.store(true, Ordering::Release);
		return Err(error);
	}
	Ok(())
}

fn release_after_idle<T>(
	resources: T,
	completed: Result<(), ash::vk::Result>,
) -> Result<(), ash::vk::Result> {
	match completed {
		Ok(()) | Err(ash::vk::Result::ERROR_DEVICE_LOST) => {
			drop(resources);
			Ok(())
		}
		Err(error) => {
			// Pending draws may reference targets, uniforms and the native surface.
			// Retain their complete ownership tree, including the loader and window.
			std::mem::forget(resources);
			Err(error)
		}
	}
}

#[cfg(test)]
#[path = "../tests/unit/window_renderer.rs"]
mod tests;
