use std::{
	error::Error,
	fmt::{Display, Formatter},
	sync::atomic::{AtomicBool, Ordering},
};

use ash::{Device, vk};

use super::{
	CameraUniform, DrawMetrics, LightingUniform, RenderObject, VulkanBuffer, VulkanBufferError,
	VulkanCamera, VulkanCameraError, VulkanDepthBuffer, VulkanDevice, VulkanMaterialPipelines,
	VulkanSwapchain, instancing::InstanceData,
};

use draw_plan::DrawBatch;
use timing::GpuTimer;

mod draw_plan;
mod recording;
mod timing;

// Bounds both CPU scratch storage and the reusable GPU allocation (80 MiB).
const MAX_FRAME_INSTANCES: usize = 1_048_576;

/// Extent-independent allocation and scratch retained between ordinary resizes.
struct InstanceStorage {
	buffer: VulkanBuffer,
	capacity: usize,
	data: Vec<InstanceData>,
	order: Vec<usize>,
	batches: Vec<DrawBatch>,
}

impl InstanceStorage {
	fn new(device: &VulkanDevice) -> Result<Self, VulkanFrameError> {
		let capacity = 64;
		Ok(Self {
			buffer: VulkanBuffer::host_vertex(
				device,
				(capacity * size_of::<InstanceData>()) as u64,
			)?,
			capacity,
			data: Vec::with_capacity(capacity),
			order: Vec::with_capacity(capacity),
			batches: Vec::with_capacity(capacity),
		})
	}
}

pub struct VulkanFrame {
	_metrics: Vec<engine_logger::metrics::ResourceGuard>,
	device: Device,

	command_pool: vk::CommandPool,
	command_buffer: vk::CommandBuffer,

	image_available: vk::Semaphore,
	render_finished: Vec<vk::Semaphore>,

	in_flight: vk::Fence,
	acquisition_fence: vk::Fence,
	acquisition_pending: bool,
	submission_pending: bool,
	completion_observed: AtomicBool,
	invalidated: bool,
	// None only while consuming a completed frame to transfer its storage.
	instances: Option<InstanceStorage>,
	palettes: Option<super::skinning::PaletteStorage>,
	timer: Option<GpuTimer>,
	metrics: DrawMetrics,
	lighting: LightingUniform,
}

/// Borrowed color and depth targets used together for a frame.
pub struct FrameTargets<'a> {
	pub swapchain: &'a VulkanSwapchain,
	pub depth_buffer: &'a VulkanDepthBuffer,
}

pub(crate) struct FrameEditor<'a> {
	pub pass: &'a mut super::editor::EditorPass,
	pub visuals: &'a crate::EditorVisuals,
}

pub(crate) struct FrameView<'a> {
	pub uniform: &'a CameraUniform,
	pub region: vk::Rect2D,
	pub editor: Option<FrameEditor<'a>>,
	pub sky: Option<(&'a super::sky::SkyPass, super::SkyGradient)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameStatus {
	Rendered,
	SwapchainNeedsRebuild,
}

#[derive(Debug)]
pub enum VulkanFrameError {
	Vulkan(vk::Result),
	Camera(VulkanCameraError),
	Buffer(VulkanBufferError),
	TooManyInstances,
	Editor(String),
}

impl Display for VulkanFrameError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Vulkan(error) => {
				write!(formatter, "Vulkan frame error: {error:?}",)
			}

			Self::Camera(error) => {
				write!(formatter, "Vulkan camera update error: {error}",)
			}
			Self::Buffer(error) => write!(formatter, "Vulkan instance buffer error: {error}"),
			Self::TooManyInstances => write!(
				formatter,
				"frame exceeds {MAX_FRAME_INSTANCES} render instances"
			),
			Self::Editor(error) => write!(formatter, "Editor visualization failed: {error}"),
		}
	}
}

impl Error for VulkanFrameError {
	fn source(&self) -> Option<&(dyn Error + 'static)> {
		match self {
			Self::Vulkan(error) => Some(error),
			Self::Camera(error) => Some(error),
			Self::Buffer(error) => Some(error),
			Self::TooManyInstances | Self::Editor(_) => None,
		}
	}
}

impl From<vk::Result> for VulkanFrameError {
	fn from(value: vk::Result) -> Self {
		Self::Vulkan(value)
	}
}

impl From<VulkanCameraError> for VulkanFrameError {
	fn from(value: VulkanCameraError) -> Self {
		Self::Camera(value)
	}
}

impl From<VulkanBufferError> for VulkanFrameError {
	fn from(value: VulkanBufferError) -> Self {
		Self::Buffer(value)
	}
}

impl VulkanFrame {
	pub fn wait_complete(&self) -> Result<(), VulkanFrameError> {
		self.wait_complete_raw().map_err(Into::into)
	}

	pub(crate) fn wait_complete_raw(&self) -> Result<(), vk::Result> {
		if self.completion_observed.load(Ordering::Acquire) {
			return Ok(());
		}
		let mut fences = [vk::Fence::null(); 2];
		let mut count = 0;
		if self.acquisition_pending {
			fences[count] = self.acquisition_fence;
			count += 1;
		}
		if self.submission_pending {
			fences[count] = self.in_flight;
			count += 1;
		}
		if count != 0 {
			unsafe {
				self.device
					.wait_for_fences(&fences[..count], true, u64::MAX)?;
			}
		}
		self.completion_observed.store(true, Ordering::Release);
		Ok(())
	}
	pub fn new(
		device: &VulkanDevice,
		swapchain: &VulkanSwapchain,
	) -> Result<Self, VulkanFrameError> {
		Self::with_image_count(device, swapchain.images().len())
	}

	fn with_image_count(
		device: &VulkanDevice,
		image_count: usize,
	) -> Result<Self, VulkanFrameError> {
		Self::with_storage(device, image_count, None)
	}

	/// The caller must establish device idle before retiring the previous frame.
	/// Synchronization handles are always fresh, including after failed draws.
	pub(crate) fn recreate(
		device: &VulkanDevice,
		swapchain: &VulkanSwapchain,
		previous: Self,
	) -> Result<Self, VulkanFrameError> {
		Self::with_storage(device, swapchain.images().len(), Some(previous))
	}

	fn with_storage(
		device: &VulkanDevice,
		image_count: usize,
		previous: Option<Self>,
	) -> Result<Self, VulkanFrameError> {
		let ash_device = device.raw().clone();
		let instances = match previous {
			Some(mut previous) if !previous.invalidated => {
				previous.instances.take().expect("live frame storage")
			}
			_ => InstanceStorage::new(device)?,
		};
		let timer = GpuTimer::new(device)?;
		let metrics = DrawMetrics {
			gpu_timestamps_supported: timer.is_some(),
			..DrawMetrics::default()
		};

		let pool_create_info = vk::CommandPoolCreateInfo::default()
			.flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
			.queue_family_index(device.queue_families().graphics);

		let command_pool = unsafe { ash_device.create_command_pool(&pool_create_info, None)? };
		let owned_pool = super::rollback::Rollback::new(command_pool, |pool| unsafe {
			ash_device.destroy_command_pool(pool, None);
		});

		let allocate_info = vk::CommandBufferAllocateInfo::default()
			.command_pool(command_pool)
			.level(vk::CommandBufferLevel::PRIMARY)
			.command_buffer_count(1);

		let command_buffers = unsafe { ash_device.allocate_command_buffers(&allocate_info)? };

		let command_buffer = command_buffers[0];

		let semaphore_info = vk::SemaphoreCreateInfo::default();

		let image_available = unsafe { ash_device.create_semaphore(&semaphore_info, None)? };
		let owned_available = super::rollback::Rollback::new(image_available, |semaphore| unsafe {
			ash_device.destroy_semaphore(semaphore, None);
		});

		let mut render_finished = super::rollback::Rollback::new(
			Vec::with_capacity(image_count),
			|semaphores: Vec<vk::Semaphore>| unsafe {
				for semaphore in semaphores {
					ash_device.destroy_semaphore(semaphore, None);
				}
			},
		);

		for _ in 0..image_count {
			let semaphore = unsafe { ash_device.create_semaphore(&semaphore_info, None)? };

			render_finished.value_mut().push(semaphore);
		}

		let fence_info = vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED);

		let in_flight = unsafe { ash_device.create_fence(&fence_info, None)? };
		let owned_in_flight = super::rollback::Rollback::new(in_flight, |fence| unsafe {
			ash_device.destroy_fence(fence, None);
		});
		let acquisition_fence =
			unsafe { ash_device.create_fence(&vk::FenceCreateInfo::default(), None)? };
		let in_flight = owned_in_flight.into_inner();
		let command_pool = owned_pool.into_inner();
		let image_available = owned_available.into_inner();
		let render_finished = render_finished.into_inner();

		Ok(Self {
			_metrics: vec![
				crate::metrics::object(
					&ash_device,
					"command_pool",
					command_pool,
					"Frame command pool",
					"",
				),
				crate::metrics::object(
					&ash_device,
					"command_buffer",
					command_buffer,
					"Frame command buffer",
					"",
				),
				crate::metrics::object(
					&ash_device,
					"semaphore",
					image_available,
					"Image acquisition semaphore",
					"",
				),
				crate::metrics::object(
					&ash_device,
					"fence",
					in_flight,
					"Frame completion fence",
					"",
				),
				crate::metrics::object(
					&ash_device,
					"fence",
					acquisition_fence,
					"Image acquisition completion fence",
					"",
				),
			]
			.into_iter()
			.chain(render_finished.iter().map(|&semaphore| {
				crate::metrics::object(
					&ash_device,
					"semaphore",
					semaphore,
					"Presentation semaphore",
					"",
				)
			}))
			.collect(),
			device: ash_device,

			command_pool,
			command_buffer,

			image_available,
			render_finished,

			in_flight,
			acquisition_fence,
			acquisition_pending: false,
			submission_pending: false,
			completion_observed: AtomicBool::new(true),
			invalidated: false,
			instances: Some(instances),
			palettes: None,
			timer,
			metrics,
			lighting: LightingUniform::default(),
		})
	}

	pub fn last_metrics(&self) -> DrawMetrics {
		self.metrics
	}

	pub(crate) fn set_lighting(&mut self, lighting: LightingUniform) {
		self.lighting = lighting;
	}

	pub fn draw(
		&mut self,
		device: &VulkanDevice,
		targets: FrameTargets<'_>,
		pipelines: &VulkanMaterialPipelines,
		camera: &VulkanCamera,
		camera_uniform: &CameraUniform,
		objects: &[RenderObject<'_>],
	) -> Result<FrameStatus, VulkanFrameError> {
		let region = vk::Rect2D::default().extent(targets.swapchain.extent());
		self.draw_with_editor(
			device,
			targets,
			pipelines,
			camera,
			objects,
			FrameView {
				uniform: camera_uniform,
				region,
				editor: None,
				sky: None,
			},
		)
	}

	pub(crate) fn draw_with_editor(
		&mut self,
		device: &VulkanDevice,
		targets: FrameTargets<'_>,
		pipelines: &VulkanMaterialPipelines,
		camera: &VulkanCamera,
		objects: &[RenderObject<'_>],
		view: FrameView<'_>,
	) -> Result<FrameStatus, VulkanFrameError> {
		if self.invalidated {
			return Ok(FrameStatus::SwapchainNeedsRebuild);
		}
		let result = self.draw_once(device, targets, pipelines, camera, objects, view);
		// Acquisition can leave its semaphore signaled even when recording or
		// submission fails. Only a fresh frame may acquire another image.
		if !matches!(result, Ok(FrameStatus::Rendered)) {
			self.invalidated = true;
		}
		result
	}

	fn draw_once(
		&mut self,
		device: &VulkanDevice,
		targets: FrameTargets<'_>,
		pipelines: &VulkanMaterialPipelines,
		camera: &VulkanCamera,
		objects: &[RenderObject<'_>],
		mut view: FrameView<'_>,
	) -> Result<FrameStatus, VulkanFrameError> {
		let swapchain = targets.swapchain;

		self.wait_complete()?;
		self.acquisition_pending = false;
		self.submission_pending = false;
		if let Some(editor) = &mut view.editor {
			let extent = view.region.extent;
			editor
				.pass
				.prepare(
					device,
					editor.visuals,
					view.uniform,
					[extent.width, extent.height],
				)
				.map_err(VulkanFrameError::Editor)?;
		}

		if let Some(timer) = &mut self.timer {
			timer.collect()?;
		}

		self.prepare_instances(device, objects)?;
		if objects.iter().any(|object| object.skin_palette().is_some()) {
			if self.palettes.is_none() {
				self.palettes = Some(super::skinning::PaletteStorage::new(device)?);
			}
			self.palettes.as_mut().unwrap().prepare(objects)?;
		} else if let Some(palettes) = &mut self.palettes {
			palettes.prepare(objects)?;
		}

		camera.update(view.uniform)?;
		camera.update_lighting(&self.lighting)?;
		unsafe {
			self.device
				.reset_fences(&[self.acquisition_fence, self.in_flight])?;
		}

		let (image_index, acquire_suboptimal) = match swapchain.acquire_next_image(
			u64::MAX,
			self.image_available,
			self.acquisition_fence,
		) {
			Ok(result) => result,

			Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
				return Ok(FrameStatus::SwapchainNeedsRebuild);
			}

			Err(error) => {
				return Err(error.into());
			}
		};
		self.acquisition_pending = true;
		*self.completion_observed.get_mut() = false;

		unsafe {
			self.device
				.reset_command_pool(self.command_pool, vk::CommandPoolResetFlags::empty())?;
		}

		self.record_draw_commands(
			targets,
			image_index,
			pipelines,
			camera.descriptor_set(),
			objects,
			&view,
		)?;

		let wait_semaphores = [self.image_available];

		let wait_stages = [vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];

		let command_buffers = [self.command_buffer];

		let render_finished = self.render_finished[image_index as usize];

		let signal_semaphores = [render_finished];

		let submit_info = vk::SubmitInfo::default()
			.wait_semaphores(&wait_semaphores)
			.wait_dst_stage_mask(&wait_stages)
			.command_buffers(&command_buffers)
			.signal_semaphores(&signal_semaphores);

		unsafe {
			self.device
				.queue_submit(device.graphics_queue(), &[submit_info], self.in_flight)?;
		}
		self.submission_pending = true;
		if let Some(timer) = &mut self.timer {
			timer.mark_submitted();
		}
		let (editor_draws, editor_triangles) = view
			.editor
			.as_ref()
			.map_or((0, 0), |editor| editor.pass.draw_counts());
		let instances = self.instances.as_ref().expect("live frame storage");
		self.metrics = DrawMetrics {
			skinned_draw_calls: objects
				.iter()
				.filter(|o| o.skin_palette().is_some())
				.count() as u64,
			skin_palette_matrices: self
				.palettes
				.as_ref()
				.map_or(0, |p| p.matrix_count() as u64),
			draw_calls: instances.batches.len() as u64 + editor_draws,
			instances: objects.len() as u64,
			triangles: instances
				.batches
				.iter()
				.map(|batch| {
					objects[batch.object].mesh().index_count() as u64 / 3 * batch.count as u64
				})
				.sum::<u64>()
				+ editor_triangles,
			instanced_draw_calls: instances
				.batches
				.iter()
				.filter(|batch| batch.count > 1)
				.count() as u64,
			gpu_time_ms: self.timer.as_ref().and_then(GpuTimer::last_ms),
			gpu_timestamps_supported: self.timer.is_some(),
		};

		crate::metrics::frame(&self.device, self.metrics);
		let present_suboptimal =
			match swapchain.present(device.present_queue(), &signal_semaphores, image_index) {
				Ok(suboptimal) => suboptimal,

				Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
					return Ok(FrameStatus::SwapchainNeedsRebuild);
				}

				Err(error) => {
					return Err(error.into());
				}
			};

		if acquire_suboptimal || present_suboptimal {
			return Ok(FrameStatus::SwapchainNeedsRebuild);
		}

		Ok(FrameStatus::Rendered)
	}

	fn prepare_instances(
		&mut self,
		device: &VulkanDevice,
		objects: &[RenderObject<'_>],
	) -> Result<(), VulkanFrameError> {
		if objects.len() > MAX_FRAME_INSTANCES {
			return Err(VulkanFrameError::TooManyInstances);
		}
		let instances = self.instances.as_mut().expect("live frame storage");
		if objects.len() > instances.capacity {
			let capacity = objects.len().next_power_of_two().min(MAX_FRAME_INSTANCES);
			instances.buffer =
				VulkanBuffer::host_vertex(device, (capacity * size_of::<InstanceData>()) as u64)?;
			instances.capacity = capacity;
		}
		draw_plan::build(
			objects,
			&mut instances.order,
			&mut instances.data,
			&mut instances.batches,
		);
		instances.buffer.write(&instances.data)?;
		Ok(())
	}
}

impl Drop for VulkanFrame {
	fn drop(&mut self) {
		unsafe {
			for semaphore in self.render_finished.drain(..) {
				self.device.destroy_semaphore(semaphore, None);
			}

			self.device.destroy_semaphore(self.image_available, None);

			self.device.destroy_fence(self.in_flight, None);
			self.device.destroy_fence(self.acquisition_fence, None);

			self.device.destroy_command_pool(self.command_pool, None);
		}
	}
}

#[cfg(test)]
#[path = "../../tests/unit/vulkan/frame.rs"]
mod construction_tests;
