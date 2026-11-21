//! Record the copy and layout transitions for a prepared texture upload.

use ash::{Device, vk};

pub(super) fn record_upload(
	ash_device: &Device,
	command_buffer: vk::CommandBuffer,
	staging_buffer: vk::Buffer,
	image: vk::Image,

	width: u32,
	height: u32,
) {
	let range = vk::ImageSubresourceRange::default()
		.aspect_mask(vk::ImageAspectFlags::COLOR)
		.base_mip_level(0)
		.level_count(1)
		.base_array_layer(0)
		.layer_count(1);

	let to_transfer = vk::ImageMemoryBarrier::default()
		.src_access_mask(vk::AccessFlags::empty())
		.dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
		.old_layout(vk::ImageLayout::UNDEFINED)
		.new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
		.src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
		.dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
		.image(image)
		.subresource_range(range);

	unsafe {
		ash_device.cmd_pipeline_barrier(
			command_buffer,
			vk::PipelineStageFlags::TOP_OF_PIPE,
			vk::PipelineStageFlags::TRANSFER,
			vk::DependencyFlags::empty(),
			&[],
			&[],
			&[to_transfer],
		);
	}

	let subresource = vk::ImageSubresourceLayers::default()
		.aspect_mask(vk::ImageAspectFlags::COLOR)
		.mip_level(0)
		.base_array_layer(0)
		.layer_count(1);

	let regions = [vk::BufferImageCopy::default()
		.buffer_offset(0)
		.buffer_row_length(0)
		.buffer_image_height(0)
		.image_subresource(subresource)
		.image_offset(vk::Offset3D { x: 0, y: 0, z: 0 })
		.image_extent(vk::Extent3D {
			width,
			height,
			depth: 1,
		})];

	unsafe {
		ash_device.cmd_copy_buffer_to_image(
			command_buffer,
			staging_buffer,
			image,
			vk::ImageLayout::TRANSFER_DST_OPTIMAL,
			&regions,
		);
	}

	let to_shader = vk::ImageMemoryBarrier::default()
		.src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
		.dst_access_mask(vk::AccessFlags::SHADER_READ)
		.old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
		.new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
		.src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
		.dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
		.image(image)
		.subresource_range(range);

	unsafe {
		ash_device.cmd_pipeline_barrier(
			command_buffer,
			vk::PipelineStageFlags::TRANSFER,
			vk::PipelineStageFlags::FRAGMENT_SHADER,
			vk::DependencyFlags::empty(),
			&[],
			&[],
			&[to_shader],
		);
	}
}
