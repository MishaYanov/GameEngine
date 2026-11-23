use ash::vk;

use super::{FrameTargets, FrameView, VulkanFrame, VulkanFrameError};
use crate::vulkan::{ModelPushConstants, RenderObject, VulkanMaterialPipelines, skinning};

impl VulkanFrame {
	pub(super) fn record_draw_commands(
		&self,
		targets: FrameTargets<'_>,
		image_index: u32,
		pipelines: &VulkanMaterialPipelines,
		camera_descriptor_set: vk::DescriptorSet,
		objects: &[RenderObject<'_>],
		view: &FrameView<'_>,
	) -> Result<(), VulkanFrameError> {
		let region = view.region;
		let image = targets.swapchain.images()[image_index as usize];
		let image_view = targets.swapchain.image_views()[image_index as usize];
		let extent = targets.swapchain.extent();
		let depth_image = targets.depth_buffer.image();
		let depth_view = targets.depth_buffer.view();

		let begin_info = vk::CommandBufferBeginInfo::default()
			.flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);

		unsafe {
			self.device
				.begin_command_buffer(self.command_buffer, &begin_info)?;
			if let Some(timer) = &self.timer {
				timer.record_begin(self.command_buffer);
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
				float32: if view.editor.is_some() {
					[0.08, 0.095, 0.12, 1.0]
				} else {
					[0.04, 0.08, 0.16, 1.0]
				},
			},
		};

		let color_attachment = vk::RenderingAttachmentInfo::default()
			.image_view(image_view)
			.image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
			.load_op(vk::AttachmentLoadOp::CLEAR)
			.store_op(vk::AttachmentStoreOp::STORE)
			.clear_value(if region.extent == extent {
				clear_value
			} else {
				vk::ClearValue {
					color: vk::ClearColorValue {
						float32: [0.0, 0.0, 0.0, 1.0],
					},
				}
			});

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
			if region != render_area {
				let color = vk::ClearAttachment::default()
					.aspect_mask(vk::ImageAspectFlags::COLOR)
					.color_attachment(0)
					.clear_value(clear_value);
				let content = vk::ClearRect::default().rect(region).layer_count(1);
				self.device
					.cmd_clear_attachments(self.command_buffer, &[color], &[content]);
			}
		}

		let viewport = vk::Viewport {
			x: region.offset.x as f32,
			y: region.offset.y as f32,
			width: region.extent.width as f32,
			height: region.extent.height as f32,

			min_depth: 0.0,
			max_depth: 1.0,
		};

		unsafe {
			self.device
				.cmd_set_viewport(self.command_buffer, 0, &[viewport]);

			self.device
				.cmd_set_scissor(self.command_buffer, 0, &[region]);

			if let Some(sky) = &view.sky {
				sky.pass.record(
					self.command_buffer,
					view.uniform,
					sky.gradient,
					sky.panorama,
				);
			}

			/*
			 * Each render object may reference
			 * a completely different mesh.
			 */
			let mut bound_pipeline = vk::Pipeline::null();
			let mut bound_material = vk::DescriptorSet::null();
			let mut bound_mesh = None;
			let mut camera_bound = false;
			let instances = self.instances.as_ref().expect("live frame storage");
			for batch in &instances.batches {
				let object = &objects[batch.object];
				let mesh = object.mesh();

				let material = object.material();

				let palette = object
					.skin_palette()
					.and(self.palettes.as_ref())
					.and_then(|storage| storage.ranges[batch.object]);
				let pipeline = if palette.is_some() {
					pipelines.for_skin(material.double_sided())
				} else {
					pipelines.for_object(
						material.double_sided(),
						object.transform().flags[0] & (2 | 8) != 0,
					)
				};
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

				let vertex_buffers = [mesh.vertex_buffer().raw(), instances.buffer.raw()];

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

				if palette.is_some() {
					self.device.cmd_bind_vertex_buffers(
						self.command_buffer,
						2,
						&[mesh.skin_buffer().expect("validated skin buffer").raw()],
						&[0],
					);
					self.device.cmd_bind_descriptor_sets(
						self.command_buffer,
						vk::PipelineBindPoint::GRAPHICS,
						pipeline_layout,
						2,
						&[self.palettes.as_ref().unwrap().set],
						&[],
					);
				}
				let mut push_constants = *object.transform();
				if let Some((base, count)) = palette {
					skinning::apply_palette_range(&mut push_constants, base, count);
				}
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

			if let Some(editor) = &view.editor {
				editor.pass.record(&self.device, self.command_buffer);
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
				timer.record_end(self.command_buffer);
			}

			self.device.end_command_buffer(self.command_buffer)?;
		}

		Ok(())
	}
}
