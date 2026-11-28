use std::{
	mem::ManuallyDrop,
	sync::{
		Arc,
		atomic::{AtomicBool, Ordering},
	},
};

use raw_window_handle::{HasDisplayHandle, HasWindowHandle};

use crate::{
	EditorShadingMode,
	vulkan::{mesh::EditorTopologyBudget, *},
};

mod targets;

use targets::finish_draw;

trait WindowOwner: HasDisplayHandle + HasWindowHandle {}
impl<T: HasDisplayHandle + HasWindowHandle> WindowOwner for T {}

struct RendererResources {
	max_viewport_dimensions: [u32; 2],
	viewport_bounds: [f32; 2],
	targets: Option<RenderTargets>,
	editor: Option<crate::vulkan::editor::EditorPass>,
	sky: Option<crate::vulkan::sky::SkyPass>,
	sky_panorama_pass: Option<crate::vulkan::sky::SkyPass>,
	sky_panorama: Option<Arc<VulkanTexture>>,
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
	sky: Option<SkyGradient>,
	editor_topology: Option<EditorTopologyBudget>,
	external_resources_retained: AtomicBool,
}

impl WindowRenderer {
	pub fn new<W: HasDisplayHandle + HasWindowHandle + 'static>(
		window: Arc<W>,
		width: u32,
		height: u32,
	) -> Result<Self, String> {
		Self::create(window, width, height, None)
	}

	/// Retain mesh data for editor wireframe draws and static surface picking.
	pub fn new_editor<W: HasDisplayHandle + HasWindowHandle + 'static>(
		window: Arc<W>,
		width: u32,
		height: u32,
	) -> Result<Self, String> {
		Self::create(window, width, height, Some(EditorTopologyBudget::default()))
	}

	fn create<W: HasDisplayHandle + HasWindowHandle + 'static>(
		window: Arc<W>,
		width: u32,
		height: u32,
		editor_topology: Option<EditorTopologyBudget>,
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
				sky: None,
				sky_panorama_pass: None,
				sky_panorama: None,
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
			sky: None,
			editor_topology,
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
		let textures: Vec<_> = textures.iter().copied().map(Into::into).collect();
		self.upload_batch_with_color_space(meshes, &textures)
	}

	/// Upload a bounded group with explicit color space for each texture.
	pub fn upload_batch_with_color_space(
		&self,
		meshes: &[crate::MeshUpload<'_>],
		textures: &[crate::Rgba8UploadWithColorSpace<'_>],
	) -> Result<crate::UploadedBatch, String> {
		let mut uploaded = crate::vulkan::upload_batch::upload(
			self.resources.instance.raw(),
			&self.resources.device,
			&self.resources.material_layout,
			meshes,
			textures,
		)?;
		if let Some(budget) = &self.editor_topology {
			for (mesh, input) in uploaded.meshes.iter_mut().zip(meshes) {
				mesh.retain_editor_topology(budget, input.indices);
				mesh.retain_editor_pick_geometry(budget, input.vertices, input.indices);
			}
		}
		Ok(uploaded)
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
	/// The active directional map's clip transform, for culling shadow casters.
	pub fn directional_shadow_view_projection(
		&self,
		camera: &CameraUniform,
		shading: Option<EditorShadingMode>,
	) -> Option<glam::Mat4> {
		if !self
			.resources
			.pipelines
			.as_ref()
			.is_some_and(VulkanMaterialPipelines::has_shadows)
		{
			return None;
		}
		self.resources
			.camera
			.shadow_map()?
			.uniform(camera, &self.lighting, shading)
			.map(|shadow| shadow.view_projection())
	}

	pub fn set_lights(&mut self, lights: &[Light]) -> Result<(), String> {
		let mut lighting = LightingUniform::from_lights(lights)?;
		lighting.retain_fog(&self.lighting);
		self.lighting = lighting;
		Ok(())
	}

	pub fn set_ambient(&mut self, ambient: [f32; 3]) -> Result<(), String> {
		self.lighting.set_ambient(ambient)
	}

	/// Configure height and distance fog for the active scene.
	pub fn set_fog(&mut self, fog: Option<Fog>) -> Result<(), String> {
		self.lighting.set_fog(fog)
	}

	/// Active scene background. An absent sky uses the existing color clear.
	pub fn set_sky(&mut self, sky: Option<SkyGradient>) {
		self.sky = sky;
	}

	/// Retain the currently sampled texture until Vulkan finishes its last draw.
	pub fn set_sky_panorama(&mut self, panorama: Option<Arc<VulkanTexture>>) -> Result<(), String> {
		if panorama
			.as_ref()
			.is_some_and(|texture| !texture.belongs_to(self.resources.device.raw()))
		{
			return Err("Sky Panorama texture belongs to a different Vulkan device".into());
		}
		let current = self.resources.sky_panorama.as_ref();
		if current
			.zip(panorama.as_ref())
			.is_some_and(|(old, new)| Arc::ptr_eq(old, new))
		{
			return Ok(());
		}
		if current.is_some() {
			self.wait_idle()?;
		}
		self.resources.sky_panorama = panorama;
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
		self.draw_region(camera, objects, None, None, true)
	}

	/// Render into physical [x, y, width, height]. Screen-space geometry uses content-local coordinates.
	pub fn draw_in_rect(
		&mut self,
		camera: &CameraUniform,
		objects: &[RenderObject<'_>],
		pixels: [u32; 4],
	) -> Result<DrawStatus, String> {
		self.draw_region(camera, objects, Some(pixels), None, true)
	}

	/// Draw editor geometry and overlays in physical [x, y, width, height].
	pub fn draw_editor_in_rect(
		&mut self,
		camera: &CameraUniform,
		objects: &[RenderObject<'_>],
		pixels: [u32; 4],
		mode: EditorShadingMode,
	) -> Result<DrawStatus, String> {
		self.draw_region(camera, objects, Some(pixels), Some(mode), true)
	}

	/// Draw a Prop preview with a per-draw normal detail toggle.
	pub fn draw_prop_preview_in_rect(
		&mut self,
		camera: &CameraUniform,
		objects: &[RenderObject<'_>],
		pixels: [u32; 4],
		shading: EditorShadingMode,
		normal_maps: bool,
	) -> Result<DrawStatus, String> {
		self.draw_region(camera, objects, Some(pixels), Some(shading), normal_maps)
	}

	fn draw_region(
		&mut self,
		camera: &CameraUniform,
		objects: &[RenderObject<'_>],
		pixels: Option<[u32; 4]>,
		shading: Option<EditorShadingMode>,
		normal_maps: bool,
	) -> Result<DrawStatus, String> {
		let Some(extent) = self.prepare()? else {
			return Ok(DrawStatus::Suspended);
		};
		let resources = &mut **self.resources;
		let wireframe = shading == Some(EditorShadingMode::Wireframe);
		let markers = shading.is_some() && objects.iter().any(RenderObject::shows_vertex_markers);
		if wireframe {
			for object in objects {
				object
					.mesh()
					.prepare_editor_edges(resources.instance.raw(), &resources.device);
			}
		}
		let pipelines = resources.pipelines.as_mut().expect("prepared pipelines");
		pipelines.prepare_editor_geometry(
			resources.device.raw(),
			resources.camera.descriptor_set_layout(),
			resources.material_layout.raw(),
			wireframe
				&& objects
				.iter()
				.any(|object| object.mesh().editor_edges().is_some()),
			markers,
		)?;
		let region = validated_region(
			pixels.unwrap_or([0, 0, extent[0], extent[1]]),
			extent,
			resources.max_viewport_dimensions,
			resources.viewport_bounds,
		)?;
		let targets = resources.targets.as_mut().expect("prepared render targets");
		let sky = if let Some(gradient) = self.sky {
			let color = targets.swapchain.format();
			let depth = targets.depth.format();
			let texture = resources.sky_panorama.as_deref();
			let pass_slot = if texture.is_some() {
				&mut resources.sky_panorama_pass
			} else {
				&mut resources.sky
			};
			if pass_slot
				.as_ref()
				.is_none_or(|pass| !pass.supports_formats(color, depth))
			{
				*pass_slot = Some(crate::vulkan::sky::SkyPass::new(
					resources.device.raw(),
					color,
					depth,
					texture.map(|_| resources.material_layout.raw()),
				)?);
			}
			Some(crate::vulkan::frame::FrameSky {
				pass: pass_slot.as_ref().expect("sky pass initialized"),
				gradient,
				panorama: texture,
			})
		} else {
			None
		};
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
		let mut lighting = self.lighting;
		lighting.set_fog_camera(camera);
		targets.frame.set_lighting(lighting);
		let result = targets.frame.draw_with_editor(
			&resources.device,
			FrameTargets {
				swapchain: &targets.swapchain,
				depth_buffer: &targets.depth,
			},
			pipelines,
			&resources.camera,
			objects,
			crate::vulkan::frame::FrameView {
				uniform: camera,
				region,
				editor,
				sky,
				shading,
				vertex_markers: markers,
				normal_maps,
			},
		);
		finish_draw(&mut self.dirty, result)
	}

	pub fn upload_mesh(
		&self,
		vertices: &[Vertex],
		indices: &[u32],
	) -> Result<VulkanMesh, VulkanMeshError> {
		let mut mesh = VulkanMesh::new(
			self.resources.instance.raw(),
			&self.resources.device,
			vertices,
			indices,
		)?;
		if let Some(budget) = &self.editor_topology {
			mesh.retain_editor_topology(budget, indices);
			mesh.retain_editor_pick_geometry(budget, vertices, indices);
		}
		Ok(mesh)
	}

	pub fn upload_skinned_mesh(
		&self,
		vertices: &[Vertex],
		indices: &[u32],
		influences: &[crate::vulkan::mesh::SkinInfluence],
	) -> Result<VulkanMesh, VulkanMeshError> {
		let mut mesh = VulkanMesh::new_skinned(
			self.resources.instance.raw(),
			&self.resources.device,
			vertices,
			indices,
			influences,
		)?;
		if let Some(budget) = &self.editor_topology {
			mesh.retain_editor_topology(budget, indices);
		}
		Ok(mesh)
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

	/// Upload color or data without changing the established sRGB upload path.
	pub fn upload_rgba8_with_color_space(
		&self,
		width: u32,
		height: u32,
		pixels: &[u8],
		color_space: TextureColorSpace,
	) -> Result<VulkanTexture, VulkanTextureError> {
		VulkanTexture::from_rgba8_with_color_space(
			self.resources.instance.raw(),
			&self.resources.device,
			Arc::clone(&self.resources.material_layout),
			width,
			height,
			pixels,
			color_space,
		)
	}

	/// Wait for the current target's submitted draw to release its mesh and
	/// texture references. The presentation engine retains the swapchain image.
	pub fn wait_for_scene_draw(&self) -> Result<(), String> {
		if let Some(targets) = &self.resources.targets {
			targets
				.frame
				.wait_complete()
				.map_err(|error| error.to_string())?;
		}
		Ok(())
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
