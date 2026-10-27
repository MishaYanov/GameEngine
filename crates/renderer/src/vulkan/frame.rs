use std::{
	error::Error,
	fmt::{Display, Formatter},
};

use ash::vk::Handle;
use ash::{Device, vk};

use super::{
	CameraUniform, DrawMetrics, LightingUniform, ModelPushConstants, RenderObject, VulkanBuffer,
	VulkanBufferError, VulkanCamera, VulkanCameraError, VulkanDepthBuffer, VulkanDevice,
	VulkanMaterialPipelines, VulkanSwapchain, instancing::InstanceData,
};

// Bounds both CPU scratch storage and the reusable GPU allocation (80 MiB).
const MAX_FRAME_INSTANCES: usize = 1_048_576;

struct DrawBatch {
	object: usize,
	first_instance: u32,
	count: u32,
	instanced: bool,
}

struct GpuTimer {
	_metrics: engine_logger::metrics::ResourceGuard,
	device: Device,
	pool: vk::QueryPool,
	valid_bits: u32,
	period_ns: f32,
	pending: bool,
	last_ms: Option<f64>,
}

impl GpuTimer {
	fn new(device: &VulkanDevice) -> Result<Option<Self>, vk::Result> {
		let Some((valid_bits, period_ns)) = device.timestamp_properties() else {
			return Ok(None);
		};
		let info = vk::QueryPoolCreateInfo::default()
			.query_type(vk::QueryType::TIMESTAMP)
			.query_count(2);
		let pool = unsafe { device.raw().create_query_pool(&info, None)? };
		Ok(Some(Self {
			_metrics: crate::metrics::object(
				device.raw(),
				"query_pool",
				pool,
				"GPU timestamp queries",
				"count=2",
			),
			device: device.raw().clone(),
			pool,
			valid_bits,
			period_ns,
			pending: false,
			last_ms: None,
		}))
	}

	fn collect(&mut self) -> Result<(), vk::Result> {
		if !self.pending {
			return Ok(());
		}
		// [timestamp, availability] per query. Never use WAIT: profiling must
		// not add synchronization to the renderer's existing frame fence.
		let mut samples = [[0_u64; 2]; 2];
		match unsafe {
			self.device.get_query_pool_results(
				self.pool,
				0,
				&mut samples,
				vk::QueryResultFlags::TYPE_64 | vk::QueryResultFlags::WITH_AVAILABILITY,
			)
		} {
			Ok(()) | Err(vk::Result::NOT_READY) => {}
			Err(error) => return Err(error),
		}
		if samples.iter().all(|sample| sample[1] != 0) {
			let mask = u64::MAX >> (64 - self.valid_bits);
			let ticks = samples[1][0].wrapping_sub(samples[0][0]) & mask;
			self.last_ms = Some(ticks as f64 * self.period_ns as f64 / 1_000_000.0);
		}
		self.pending = false;
		Ok(())
	}
}

impl Drop for GpuTimer {
	fn drop(&mut self) {
		unsafe {
			self.device.destroy_query_pool(self.pool, None);
		}
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
	instance_buffer: VulkanBuffer,
	instance_capacity: usize,
	instance_data: Vec<InstanceData>,
	order: Vec<usize>,
	batches: Vec<DrawBatch>,
	timer: Option<GpuTimer>,
	metrics: DrawMetrics,
	lighting: LightingUniform,
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
		}
	}
}

impl Error for VulkanFrameError {
	fn source(&self) -> Option<&(dyn Error + 'static)> {
		match self {
			Self::Vulkan(error) => Some(error),
			Self::Camera(error) => Some(error),
			Self::Buffer(error) => Some(error),
			Self::TooManyInstances => None,
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
		unsafe {
			self.device
				.wait_for_fences(&[self.in_flight], true, u64::MAX)?;
		}
		Ok(())
	}
	pub fn new(
		device: &VulkanDevice,
		swapchain: &VulkanSwapchain,
	) -> Result<Self, VulkanFrameError> {
		let ash_device = device.raw().clone();
		let instance_capacity = 64;
		let instance_buffer = VulkanBuffer::host_vertex(
			device,
			(instance_capacity * size_of::<InstanceData>()) as u64,
		)?;
		let timer = GpuTimer::new(device)?;
		let metrics = DrawMetrics {
			gpu_timestamps_supported: timer.is_some(),
			..DrawMetrics::default()
		};

		let pool_create_info = vk::CommandPoolCreateInfo::default()
			.flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
			.queue_family_index(device.queue_families().graphics);

		let command_pool = unsafe { ash_device.create_command_pool(&pool_create_info, None)? };

		let allocate_info = vk::CommandBufferAllocateInfo::default()
			.command_pool(command_pool)
			.level(vk::CommandBufferLevel::PRIMARY)
			.command_buffer_count(1);

		let command_buffers = unsafe { ash_device.allocate_command_buffers(&allocate_info)? };

		let command_buffer = command_buffers[0];

		let semaphore_info = vk::SemaphoreCreateInfo::default();

		let image_available = unsafe { ash_device.create_semaphore(&semaphore_info, None)? };

		let mut render_finished = Vec::with_capacity(swapchain.images().len());

		for _ in swapchain.images() {
			let semaphore = unsafe { ash_device.create_semaphore(&semaphore_info, None)? };

			render_finished.push(semaphore);
		}

		let fence_info = vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED);

		let in_flight = unsafe { ash_device.create_fence(&fence_info, None)? };

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
			instance_buffer,
			instance_capacity,
			instance_data: Vec::with_capacity(instance_capacity),
			order: Vec::with_capacity(instance_capacity),
			batches: Vec::with_capacity(instance_capacity),
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
		swapchain: &VulkanSwapchain,
		depth_buffer: &VulkanDepthBuffer,
		pipelines: &VulkanMaterialPipelines,
		camera: &VulkanCamera,
		camera_uniform: &CameraUniform,
		objects: &[RenderObject<'_>],
	) -> Result<FrameStatus, VulkanFrameError> {
		unsafe {
			self.device
				.wait_for_fences(&[self.in_flight], true, u64::MAX)?;
		}

		if let Some(timer) = &mut self.timer {
			timer.collect()?;
		}

		self.prepare_instances(device, objects)?;

		camera.update(camera_uniform)?;
		camera.update_lighting(&self.lighting)?;

		let (image_index, acquire_suboptimal) =
			match swapchain.acquire_next_image(u64::MAX, self.image_available, vk::Fence::null()) {
				Ok(result) => result,

				Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
					return Ok(FrameStatus::SwapchainNeedsRebuild);
				}

				Err(error) => {
					return Err(error.into());
				}
			};

		unsafe {
			self.device
				.reset_command_pool(self.command_pool, vk::CommandPoolResetFlags::empty())?;
		}

		self.record_draw_commands(
			swapchain.images()[image_index as usize],
			swapchain.image_views()[image_index as usize],
			swapchain.extent(),
			depth_buffer.image(),
			depth_buffer.view(),
			pipelines,
			camera.descriptor_set(),
			objects,
		)?;

		unsafe {
			self.device.reset_fences(&[self.in_flight])?;
		}

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
		if let Some(timer) = &mut self.timer {
			timer.pending = true;
		}
		self.metrics = DrawMetrics {
			draw_calls: self.batches.len() as u64,
			instances: objects.len() as u64,
			triangles: self
				.batches
				.iter()
				.map(|batch| {
					objects[batch.object].mesh().index_count() as u64 / 3 * batch.count as u64
				})
				.sum(),
			instanced_draw_calls: self.batches.iter().filter(|batch| batch.count > 1).count()
				as u64,
			gpu_time_ms: self.timer.as_ref().and_then(|timer| timer.last_ms),
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
		if objects.len() > self.instance_capacity {
			let capacity = objects.len().next_power_of_two().min(MAX_FRAME_INSTANCES);
			self.instance_buffer =
				VulkanBuffer::host_vertex(device, (capacity * size_of::<InstanceData>()) as u64)?;
			self.instance_capacity = capacity;
		}
		self.order.clear();
		self.order.extend(0..objects.len());
		// Reorder only opaque runs. Every overlay/unlit/alpha object is an
		// ordering barrier, so editor UI and debug draws retain submission order.
		let mut start = 0;
		while start < objects.len() {
			if !batchable(&objects[start]) {
				start += 1;
				continue;
			}
			let end = (start..objects.len())
				.find(|&i| !batchable(&objects[i]))
				.unwrap_or(objects.len());
			self.order[start..end].sort_unstable_by_key(|&i| (batch_key(&objects[i]), i));
			start = end;
		}
		self.instance_data.clear();
		self.batches.clear();
		for &index in &self.order {
			let object = &objects[index];
			let instanced = batchable(object);
			let first_instance = self.instance_data.len() as u32;
			self.instance_data.push(InstanceData::from_object(object));
			if let Some(batch) = self.batches.last_mut()
				&& instanced && batch.instanced
				&& batch_key(&objects[batch.object]) == batch_key(object)
			{
				batch.count += 1;
			} else {
				self.batches.push(DrawBatch {
					object: index,
					first_instance,
					count: 1,
					instanced,
				});
			}
		}
		self.instance_buffer.write(&self.instance_data)?;
		Ok(())
	}

	fn record_draw_commands(
		&self,
		image: vk::Image,
		image_view: vk::ImageView,
		extent: vk::Extent2D,
		depth_image: vk::Image,
		depth_view: vk::ImageView,
		pipelines: &VulkanMaterialPipelines,
		camera_descriptor_set: vk::DescriptorSet,
		objects: &[RenderObject<'_>],
	) -> Result<(), VulkanFrameError> {
		let begin_info = vk::CommandBufferBeginInfo::default()
			.flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);

		unsafe {
			self.device
				.begin_command_buffer(self.command_buffer, &begin_info)?;
			if let Some(timer) = &self.timer {
				self.device
					.cmd_reset_query_pool(self.command_buffer, timer.pool, 0, 2);
				self.device.cmd_write_timestamp(
					self.command_buffer,
					vk::PipelineStageFlags::TOP_OF_PIPE,
					timer.pool,
					0,
				);
			}
		}

		let range = vk::ImageSubresourceRange::default()
			.aspect_mask(vk::ImageAspectFlags::COLOR)
			.base_mip_level(0)
			.level_count(1)
			.base_array_layer(0)
			.layer_count(1);

		let to_color_attachment = vk::ImageMemoryBarrier::default()
			.src_access_mask(vk::AccessFlags::empty())
			.dst_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)
			.old_layout(vk::ImageLayout::UNDEFINED)
			.new_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
			.src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
			.dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
			.image(image)
			.subresource_range(range);

		unsafe {
			self.device.cmd_pipeline_barrier(
				self.command_buffer,
				vk::PipelineStageFlags::TOP_OF_PIPE,
				vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
				vk::DependencyFlags::empty(),
				&[],
				&[],
				&[to_color_attachment],
			);
		}

		let depth_range = vk::ImageSubresourceRange::default()
			.aspect_mask(vk::ImageAspectFlags::DEPTH)
			.base_mip_level(0)
			.level_count(1)
			.base_array_layer(0)
			.layer_count(1);

		let to_depth_attachment = vk::ImageMemoryBarrier::default()
			.src_access_mask(vk::AccessFlags::empty())
			.dst_access_mask(vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE)
			.old_layout(vk::ImageLayout::UNDEFINED)
			.new_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL)
			.src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
			.dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
			.image(depth_image)
			.subresource_range(depth_range);

		unsafe {
			self.device.cmd_pipeline_barrier(
				self.command_buffer,
				vk::PipelineStageFlags::TOP_OF_PIPE,
				vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS
					| vk::PipelineStageFlags::LATE_FRAGMENT_TESTS,
				vk::DependencyFlags::empty(),
				&[],
				&[],
				&[to_depth_attachment],
			);
		}

		let clear_value = vk::ClearValue {
			color: vk::ClearColorValue {
				float32: [0.04, 0.08, 0.16, 1.0],
			},
		};

		let color_attachment = vk::RenderingAttachmentInfo::default()
			.image_view(image_view)
			.image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
			.load_op(vk::AttachmentLoadOp::CLEAR)
			.store_op(vk::AttachmentStoreOp::STORE)
			.clear_value(clear_value);

		let color_attachments = [color_attachment];

		let depth_clear_value = vk::ClearValue {
			depth_stencil: vk::ClearDepthStencilValue {
				depth: 1.0,
				stencil: 0,
			},
		};

		let depth_attachment = vk::RenderingAttachmentInfo::default()
			.image_view(depth_view)
			.image_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL)
			.load_op(vk::AttachmentLoadOp::CLEAR)
			.store_op(vk::AttachmentStoreOp::DONT_CARE)
			.clear_value(depth_clear_value);

		let render_area = vk::Rect2D {
			offset: vk::Offset2D { x: 0, y: 0 },

			extent,
		};

		let rendering_info = vk::RenderingInfo::default()
			.render_area(render_area)
			.layer_count(1)
			.color_attachments(&color_attachments)
			.depth_attachment(&depth_attachment);

		unsafe {
			self.device
				.cmd_begin_rendering(self.command_buffer, &rendering_info);
		}

		let viewport = vk::Viewport {
			x: 0.0,
			y: 0.0,

			width: extent.width as f32,

			height: extent.height as f32,

			min_depth: 0.0,
			max_depth: 1.0,
		};

		let scissor = vk::Rect2D {
			offset: vk::Offset2D { x: 0, y: 0 },

			extent,
		};

		unsafe {
			self.device
				.cmd_set_viewport(self.command_buffer, 0, &[viewport]);

			self.device
				.cmd_set_scissor(self.command_buffer, 0, &[scissor]);

			/*
			 * Each render object may reference
			 * a completely different mesh.
			 */
			let mut bound_pipeline = vk::Pipeline::null();
			let mut bound_material = vk::DescriptorSet::null();
			let mut bound_mesh = None;
			let mut camera_bound = false;
			for batch in &self.batches {
				let object = &objects[batch.object];
				let mesh = object.mesh();

				let material = object.material();

				let pipeline = pipelines.for_object(
					material.double_sided(),
					object.transform().flags[0] & (2 | 8) != 0,
				);
				let pipeline_layout = pipeline.layout();
				if bound_pipeline != pipeline.raw() {
					self.device.cmd_bind_pipeline(
						self.command_buffer,
						vk::PipelineBindPoint::GRAPHICS,
						pipeline.raw(),
					);
					bound_pipeline = pipeline.raw();
				}
				// All material pipeline variants have compatible set layouts.
				if !camera_bound {
					self.device.cmd_bind_descriptor_sets(
						self.command_buffer,
						vk::PipelineBindPoint::GRAPHICS,
						pipeline_layout,
						0,
						&[camera_descriptor_set],
						&[],
					);
					camera_bound = true;
				}

				/*
				 * Set 1 contains this object's material.
				 */
				let material_sets = [material.descriptor_set()];

				if bound_material != material_sets[0] {
					self.device.cmd_bind_descriptor_sets(
						self.command_buffer,
						vk::PipelineBindPoint::GRAPHICS,
						pipeline_layout,
						1,
						&material_sets,
						&[],
					);
					bound_material = material_sets[0];
				}

				let vertex_buffers = [mesh.vertex_buffer().raw(), self.instance_buffer.raw()];

				let vertex_offsets = [0, 0];

				let mesh_buffers = [mesh.vertex_buffer().raw(), mesh.index_buffer().raw()];
				if bound_mesh != Some(mesh_buffers) {
					self.device.cmd_bind_vertex_buffers(
						self.command_buffer,
						0,
						&vertex_buffers,
						&vertex_offsets,
					);
					self.device.cmd_bind_index_buffer(
						self.command_buffer,
						mesh.index_buffer().raw(),
						0,
						vk::IndexType::UINT32,
					);
					bound_mesh = Some(mesh_buffers);
				}

				let mut push_constants = *object.transform();
				for (component, factor) in push_constants
					.base_color_factor
					.iter_mut()
					.zip(material.base_color_factor())
				{
					*component *= factor;
				}
				if batch.instanced {
					push_constants.flags[0] |= 16;
				}

				self.device.cmd_push_constants(
					self.command_buffer,
					pipeline_layout,
					vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
					ModelPushConstants::OFFSET,
					push_constants.as_bytes(),
				);

				self.device.cmd_draw_indexed(
					self.command_buffer,
					mesh.index_count(),
					batch.count,
					0,
					0,
					batch.first_instance,
				);
			}

			self.device.cmd_end_rendering(self.command_buffer);
		}

		let to_present = vk::ImageMemoryBarrier::default()
			.src_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)
			.dst_access_mask(vk::AccessFlags::empty())
			.old_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
			.new_layout(vk::ImageLayout::PRESENT_SRC_KHR)
			.src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
			.dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
			.image(image)
			.subresource_range(range);

		unsafe {
			self.device.cmd_pipeline_barrier(
				self.command_buffer,
				vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
				vk::PipelineStageFlags::BOTTOM_OF_PIPE,
				vk::DependencyFlags::empty(),
				&[],
				&[],
				&[to_present],
			);
			if let Some(timer) = &self.timer {
				self.device.cmd_write_timestamp(
					self.command_buffer,
					vk::PipelineStageFlags::BOTTOM_OF_PIPE,
					timer.pool,
					1,
				);
			}

			self.device.end_command_buffer(self.command_buffer)?;
		}

		Ok(())
	}
}

fn batchable(object: &RenderObject<'_>) -> bool {
	object.transform().flags[0] == 0
		&& object.transform().base_color_factor[3] >= 1.0
		&& object.material().base_color_factor()[3] >= 1.0
}

fn batch_key(object: &RenderObject<'_>) -> [u64; 8] {
	let color = object.material().base_color_factor();
	[
		object.mesh().vertex_buffer().raw().as_raw(),
		object.mesh().index_buffer().raw().as_raw(),
		object.material().descriptor_set().as_raw(),
		object.material().double_sided() as u64,
		color[0].to_bits() as u64,
		color[1].to_bits() as u64,
		color[2].to_bits() as u64,
		color[3].to_bits() as u64,
	]
}

impl Drop for VulkanFrame {
	fn drop(&mut self) {
		unsafe {
			for semaphore in self.render_finished.drain(..) {
				self.device.destroy_semaphore(semaphore, None);
			}

			self.device.destroy_semaphore(self.image_available, None);

			self.device.destroy_fence(self.in_flight, None);

			self.device.destroy_command_pool(self.command_pool, None);
		}
	}
}
