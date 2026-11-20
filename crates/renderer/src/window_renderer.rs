use std::{
	mem::ManuallyDrop,
	sync::{
		Arc,
		atomic::{AtomicBool, Ordering},
	},
};

use raw_window_handle::{HasDisplayHandle, HasWindowHandle};

use crate::vulkan::*;

trait WindowOwner: HasDisplayHandle + HasWindowHandle {}
impl<T: HasDisplayHandle + HasWindowHandle> WindowOwner for T {}

/// Owns a presentation renderer. Uploaded resources must be released before this owner.
/// Field order releases GPU dependants before the device, surface, instance and window.
pub struct WindowRenderer {
	resources: ManuallyDrop<RendererResources>,
	size: [u32; 2],
	dirty: bool,
	lighting: LightingUniform,
	editor_visuals: crate::EditorVisuals,
	external_resources_retained: AtomicBool,
}

struct RendererResources {
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

impl WindowRenderer {
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
			resources: ManuallyDrop::new(RendererResources {
				targets: None,
				editor: None,
				pipelines: None,
				camera,
				material_layout,
				device,
				surface,
				instance,
				_window: window,
			}),
			size: [width, height],
			dirty: true,
			lighting: LightingUniform::default(),
			editor_visuals: crate::EditorVisuals::default(),
			external_resources_retained: AtomicBool::new(false),
		};
		renderer.prepare()?;
		Ok(renderer)
	}

	pub fn resize(&mut self, width: u32, height: u32) {
		if self.size != [width, height] {
			self.size = [width, height];
			self.dirty = true;
		}
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

	pub fn draw(
		&mut self,
		camera: &CameraUniform,
		objects: &[RenderObject<'_>],
	) -> Result<DrawStatus, String> {
		if self.prepare()?.is_none() {
			return Ok(DrawStatus::Suspended);
		}
		let resources = &mut *self.resources;
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
			camera,
			objects,
			editor,
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

fn finish_draw(
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
mod tests {
	use super::{
		DrawStatus, FrameStatus, VulkanFrameError, finish_draw, release_after_idle, retire_external,
	};
	use std::cell::RefCell;
	use std::sync::atomic::{AtomicBool, Ordering};
	struct Resource<'a>(&'a RefCell<Vec<u8>>, u8);
	impl Drop for Resource<'_> {
		fn drop(&mut self) {
			self.0.borrow_mut().push(self.1);
		}
	}

	#[test]
	fn draw_errors_preserve_the_error_and_force_target_replacement_before_retry() {
		let mut dirty = false;
		let error = ash::vk::Result::ERROR_OUT_OF_HOST_MEMORY;
		assert_eq!(
			finish_draw(&mut dirty, Err(VulkanFrameError::Vulkan(error))).unwrap_err(),
			VulkanFrameError::Vulkan(error).to_string()
		);
		assert!(
			dirty,
			"the next public draw must enter prepare's idle/replacement path"
		);
		dirty = false;
		assert_eq!(
			finish_draw(&mut dirty, Ok(FrameStatus::SwapchainNeedsRebuild)).unwrap(),
			DrawStatus::NeedsRedraw
		);
		assert!(dirty);
		dirty = false;
		assert_eq!(
			finish_draw(&mut dirty, Ok(FrameStatus::Rendered)).unwrap(),
			DrawStatus::Rendered
		);
		assert!(!dirty);
	}

	#[test]
	fn retained_external_children_keep_the_parent_retained_after_later_wait_success() {
		let retained = AtomicBool::new(false);
		let dropped = RefCell::new(Vec::new());
		assert!(
			retire_external(
				Resource(&dropped, 1),
				Err(ash::vk::Result::ERROR_OUT_OF_HOST_MEMORY),
				&retained
			)
			.is_err()
		);
		assert!(retained.load(Ordering::Acquire));
		assert!(dropped.borrow().is_empty());
		retire_external(Resource(&dropped, 2), Ok(()), &retained).unwrap();
		assert!(
			retained.load(Ordering::Acquire),
			"earlier forgotten children still own Vulkan handles"
		);
		assert_eq!(*dropped.borrow(), [2]);
		let retained = AtomicBool::new(false);
		retire_external(
			Resource(&dropped, 3),
			Err(ash::vk::Result::ERROR_DEVICE_LOST),
			&retained,
		)
		.unwrap();
		assert!(!retained.load(Ordering::Acquire));
		assert_eq!(*dropped.borrow(), [2, 3]);
	}

	#[test]
	fn shutdown_releases_dependants_before_parents_only_after_confirmed_completion() {
		for result in [
			Ok(()),
			Err(ash::vk::Result::ERROR_DEVICE_LOST),
			Err(ash::vk::Result::ERROR_OUT_OF_HOST_MEMORY),
		] {
			let dropped = RefCell::new(Vec::new());
			let outcome =
				release_after_idle((Resource(&dropped, 1), Resource(&dropped, 2)), result);
			if outcome.is_ok() {
				assert_eq!(*dropped.borrow(), [1, 2]);
			} else {
				assert!(dropped.borrow().is_empty());
			}
		}
	}
}
