use std::{
	error::Error,
	fmt::{
		Display,
		Formatter,
	},
};

use ash::{
	vk,
	Device,
};

use super::{
	VulkanDevice,
	VulkanSwapchain,
};

pub struct VulkanFrame {
	device: Device,

	command_pool: vk::CommandPool,
	command_buffer: vk::CommandBuffer,

	image_available: vk::Semaphore,

	/*
	 * One presentation semaphore per swapchain image.
	 *
	 * This avoids reusing a semaphore while an earlier
	 * presentation operation may still be using it.
	 */
	render_finished: Vec<vk::Semaphore>,

	in_flight: vk::Fence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameStatus {
	Rendered,
	SwapchainNeedsRebuild,
}

#[derive(Debug)]
pub enum VulkanFrameError {
	Vulkan(vk::Result),
}

impl Display for VulkanFrameError {
	fn fmt(
		&self,
		formatter: &mut Formatter<'_>,
	) -> std::fmt::Result {
		match self {
			Self::Vulkan(error) => {
				write!(
					formatter,
					"Vulkan frame error: {error:?}",
				)
			}
		}
	}
}

impl Error for VulkanFrameError {}

impl From<vk::Result> for VulkanFrameError {
	fn from(value: vk::Result) -> Self {
		Self::Vulkan(value)
	}
}

impl VulkanFrame {
	pub fn new(
		device: &VulkanDevice,
		swapchain: &VulkanSwapchain,
	) -> Result<Self, VulkanFrameError> {
		let ash_device =
			device.raw().clone();

		let pool_create_info =
			vk::CommandPoolCreateInfo::default()
				.flags(
					vk::CommandPoolCreateFlags::
					RESET_COMMAND_BUFFER,
				)
				.queue_family_index(
					device
						.queue_families()
						.graphics,
				);

		let command_pool = unsafe {
			ash_device.create_command_pool(
				&pool_create_info,
				None,
			)?
		};

		let allocate_info =
			vk::CommandBufferAllocateInfo::default()
				.command_pool(
					command_pool,
				)
				.level(
					vk::CommandBufferLevel::PRIMARY,
				)
				.command_buffer_count(1);

		let command_buffers = unsafe {
			ash_device.allocate_command_buffers(
				&allocate_info,
			)?
		};

		let command_buffer =
			command_buffers[0];

		let semaphore_info =
			vk::SemaphoreCreateInfo::default();

		let image_available = unsafe {
			ash_device.create_semaphore(
				&semaphore_info,
				None,
			)?
		};

		let mut render_finished =
			Vec::with_capacity(
				swapchain.images().len(),
			);

		for _ in swapchain.images() {
			let semaphore = unsafe {
				ash_device.create_semaphore(
					&semaphore_info,
					None,
				)?
			};

			render_finished.push(
				semaphore,
			);
		}

		/*
		 * Start signaled so our very first frame
		 * doesn't block waiting for work that
		 * never existed.
		 */
		let fence_info =
			vk::FenceCreateInfo::default()
				.flags(
					vk::FenceCreateFlags::SIGNALED,
				);

		let in_flight = unsafe {
			ash_device.create_fence(
				&fence_info,
				None,
			)?
		};

		Ok(Self {
			device: ash_device,

			command_pool,
			command_buffer,

			image_available,
			render_finished,

			in_flight,
		})
	}

	pub fn draw_clear(
		&mut self,
		device: &VulkanDevice,
		swapchain: &VulkanSwapchain,
	) -> Result<
		FrameStatus,
		VulkanFrameError,
	> {
		unsafe {
			self.device.wait_for_fences(
				&[self.in_flight],
				true,
				u64::MAX,
			)?;
		}

		let (
			image_index,
			acquire_suboptimal,
		) = match swapchain
			.acquire_next_image(
				u64::MAX,
				self.image_available,
				vk::Fence::null(),
			)
		{
			// TODO: extract error handling
			Ok(result) => result,

			Err(
				vk::Result::ERROR_OUT_OF_DATE_KHR,
			) => {
				return Ok(
					FrameStatus::
					SwapchainNeedsRebuild,
				);
			}

			Err(error) => {
				return Err(error.into());
			}
		};

		/*
		 * Previous GPU work is finished, so the
		 * command pool can be reused.
		 */
		unsafe {
			self.device.reset_command_pool(
				self.command_pool,
				vk::CommandPoolResetFlags::empty(),
			)?;
		}

		self.record_clear_commands(
			swapchain.images()[
				image_index as usize
				],
		)?;

		/*
		 * Only reset after acquisition and command
		 * recording succeeded.
		 *
		 * Otherwise we'd leave an unsignaled fence
		 * with nothing scheduled to signal it.
		 */
		unsafe {
			self.device.reset_fences(
				&[self.in_flight],
			)?;
		}

		let wait_semaphores = [
			self.image_available,
		];

		/*
		 * The first operation touching the image is
		 * our transfer/layout-transition path.
		 */
		let wait_stages = [
			vk::PipelineStageFlags::TRANSFER,
		];

		let command_buffers = [
			self.command_buffer,
		];

		let render_finished =
			self.render_finished[
				image_index as usize
				];

		let signal_semaphores = [
			render_finished,
		];

		let submit_info =
			vk::SubmitInfo::default()
				.wait_semaphores(
					&wait_semaphores,
				)
				.wait_dst_stage_mask(
					&wait_stages,
				)
				.command_buffers(
					&command_buffers,
				)
				.signal_semaphores(
					&signal_semaphores,
				);

		unsafe {
			self.device.queue_submit(
				device.graphics_queue(),
				&[submit_info],
				self.in_flight,
			)?;
		}

		let present_suboptimal =
			match swapchain.present(
				device.present_queue(),
				&signal_semaphores,
				image_index,
			) {
				Ok(suboptimal) => suboptimal,

				Err(
					vk::Result::
					ERROR_OUT_OF_DATE_KHR,
				) => {
					return Ok(
						FrameStatus::
						SwapchainNeedsRebuild,
					);
				}

				Err(error) => {
					return Err(error.into());
				}
			};

		if acquire_suboptimal
			|| present_suboptimal
		{
			return Ok(
				FrameStatus::
				SwapchainNeedsRebuild,
			);
		}

		Ok(FrameStatus::Rendered)
	}

	fn record_clear_commands(
		&self,
		image: vk::Image,
	) -> Result<(), VulkanFrameError> {
		let begin_info =
			vk::CommandBufferBeginInfo::default()
				.flags(
					vk::CommandBufferUsageFlags::
					ONE_TIME_SUBMIT,
				);

		unsafe {
			self.device.begin_command_buffer(
				self.command_buffer,
				&begin_info,
			)?;
		}

		let range =
			vk::ImageSubresourceRange::default()
				.aspect_mask(
					vk::ImageAspectFlags::COLOR,
				)
				.base_mip_level(0)
				.level_count(1)
				.base_array_layer(0)
				.layer_count(1);

		/*
		 * We clear the entire image, so preserving
		 * previous presentation contents serves no
		 * purpose.
		 *
		 * Vulkan permits UNDEFINED here when the
		 * previous image contents can be discarded.
		 */
		let to_transfer =
			vk::ImageMemoryBarrier::default()
				.src_access_mask(
					vk::AccessFlags::empty(),
				)
				.dst_access_mask(
					vk::AccessFlags::
					TRANSFER_WRITE,
				)
				.old_layout(
					vk::ImageLayout::UNDEFINED,
				)
				.new_layout(
					vk::ImageLayout::
					TRANSFER_DST_OPTIMAL,
				)
				.src_queue_family_index(
					vk::QUEUE_FAMILY_IGNORED,
				)
				.dst_queue_family_index(
					vk::QUEUE_FAMILY_IGNORED,
				)
				.image(image)
				.subresource_range(range);

		unsafe {
			self.device.cmd_pipeline_barrier(
				self.command_buffer,

				vk::PipelineStageFlags::TRANSFER,
				vk::PipelineStageFlags::TRANSFER,

				vk::DependencyFlags::empty(),

				&[],
				&[],
				&[to_transfer],
			);
		}

		/*
		 * This is our first actual rendered output.
		 *
		 * No shaders, render pass or pipeline yet:
		 * simply clear the swapchain image.
		 */
		let clear_color =
			vk::ClearColorValue {
				float32: [
					0.04,
					0.08,
					0.16,
					1.0,
				],
			};

		unsafe {
			self.device.cmd_clear_color_image(
				self.command_buffer,
				image,
				vk::ImageLayout::
				TRANSFER_DST_OPTIMAL,
				&clear_color,
				&[range],
			);
		}

		let to_present =
			vk::ImageMemoryBarrier::default()
				.src_access_mask(
					vk::AccessFlags::
					TRANSFER_WRITE,
				)
				.dst_access_mask(
					vk::AccessFlags::empty(),
				)
				.old_layout(
					vk::ImageLayout::
					TRANSFER_DST_OPTIMAL,
				)
				.new_layout(
					vk::ImageLayout::
					PRESENT_SRC_KHR,
				)
				.src_queue_family_index(
					vk::QUEUE_FAMILY_IGNORED,
				)
				.dst_queue_family_index(
					vk::QUEUE_FAMILY_IGNORED,
				)
				.image(image)
				.subresource_range(range);

		unsafe {
			self.device.cmd_pipeline_barrier(
				self.command_buffer,

				vk::PipelineStageFlags::TRANSFER,
				vk::PipelineStageFlags::
				BOTTOM_OF_PIPE,

				vk::DependencyFlags::empty(),

				&[],
				&[],
				&[to_present],
			);

			self.device.end_command_buffer(
				self.command_buffer,
			)?;
		}

		Ok(())
	}
}

impl Drop for VulkanFrame {
	fn drop(&mut self) {
		unsafe {
			for semaphore in
				self.render_finished.drain(..)
			{
				self.device.destroy_semaphore(
					semaphore,
					None,
				);
			}

			self.device.destroy_semaphore(
				self.image_available,
				None,
			);

			self.device.destroy_fence(
				self.in_flight,
				None,
			);

			/*
			 * Destroying the pool also releases
			 * its command buffers.
			 */
			self.device.destroy_command_pool(
				self.command_pool,
				None,
			);
		}
	}
}