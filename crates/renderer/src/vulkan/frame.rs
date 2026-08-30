use std::{
    error::Error,
    fmt::{Display, Formatter},
};

use ash::{Device, vk};

use super::{
    ModelPushConstants,
    VulkanDevice,
    VulkanGraphicsPipeline,
    VulkanMesh,
    VulkanSwapchain,
};

pub struct VulkanFrame {
    device: Device,

    command_pool: vk::CommandPool,
    command_buffer: vk::CommandBuffer,

    image_available: vk::Semaphore,
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
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Vulkan(error) => {
                write!(formatter, "Vulkan frame error: {error:?}",)
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
        let ash_device = device.raw().clone();

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
            device: ash_device,

            command_pool,
            command_buffer,

            image_available,
            render_finished,

            in_flight,
        })
    }

    pub fn draw(
        &mut self,
        device: &VulkanDevice,
        swapchain: &VulkanSwapchain,
        pipeline: &VulkanGraphicsPipeline,
        mesh: &VulkanMesh,
        models: &[ModelPushConstants],
    ) -> Result<
        FrameStatus,
        VulkanFrameError,
    > {
        unsafe {
            self.device
                .wait_for_fences(&[self.in_flight], true, u64::MAX)?;
        }

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
            swapchain.images()[
                image_index as usize
                ],

            swapchain.image_views()[
                image_index as usize
                ],

            swapchain.extent(),

            pipeline.raw(),
            pipeline.layout(),

            mesh.vertex_buffer().raw(),
            mesh.index_buffer().raw(),
            mesh.index_count(),

            models,
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

    fn record_draw_commands(
        &self,
        image: vk::Image,
        image_view: vk::ImageView,
        extent: vk::Extent2D,

        pipeline: vk::Pipeline,
        pipeline_layout: vk::PipelineLayout,

        vertex_buffer: vk::Buffer,
        index_buffer: vk::Buffer,
        index_count: u32,

        models: &[ModelPushConstants],
    ) -> Result<
        (),
        VulkanFrameError,
    > {
        let begin_info = vk::CommandBufferBeginInfo::default()
            .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);

        unsafe {
            self.device
                .begin_command_buffer(self.command_buffer, &begin_info)?;
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

        let render_area = vk::Rect2D {
            offset: vk::Offset2D { x: 0, y: 0 },

            extent,
        };

        let rendering_info = vk::RenderingInfo::default()
            .render_area(render_area)
            .layer_count(1)
            .color_attachments(&color_attachments);

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
			* The pipeline and mesh are shared by
			* every object in this draw batch.
			*/
            self.device
                .cmd_bind_pipeline(
                    self.command_buffer,
                    vk::PipelineBindPoint::GRAPHICS,
                    pipeline,
                );

            let vertex_buffers = [
                vertex_buffer,
            ];

            let vertex_offsets = [
                0,
            ];

            self.device
                .cmd_bind_vertex_buffers(
                    self.command_buffer,
                    0,
                    &vertex_buffers,
                    &vertex_offsets,
                );

            self.device
                .cmd_bind_index_buffer(
                    self.command_buffer,
                    index_buffer,
                    0,
                    vk::IndexType::UINT16,
                );

            /*
			 * Each object gets different per-draw
			 * state while reusing the same mesh.
			 */
            for model in models {
                self.device
                    .cmd_push_constants(
                        self.command_buffer,

                        pipeline_layout,

                        vk::ShaderStageFlags::
                        VERTEX,

                        0,

                        model.as_bytes(),
                    );

                self.device
                    .cmd_draw_indexed(
                        self.command_buffer,

                        index_count,
                        1,

                        0,
                        0,
                        0,
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

            self.device.end_command_buffer(self.command_buffer)?;
        }

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

            self.device.destroy_command_pool(self.command_pool, None);
        }
    }
}
