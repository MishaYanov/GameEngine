//! Synchronous transfer submission with ownership spanning GPU completion.
use ash::{Device, vk};

use super::{VulkanBuffer, VulkanDevice};

struct Submission<R> {
	device: Device,
	pool: vk::CommandPool,
	fence: vk::Fence,
	resources: Option<R>,
	_metrics: Vec<engine_logger::metrics::ResourceGuard>,
}

impl<R> Drop for Submission<R> {
	fn drop(&mut self) {
		unsafe {
			if self.fence != vk::Fence::null() {
				self.device.destroy_fence(self.fence, None);
			}
			if self.pool != vk::CommandPool::null() {
				self.device.destroy_command_pool(self.pool, None);
			}
		}
	}
}

/// No submitted resource leaves this owner before completion. If both waiting
/// methods fail without device loss, the device retains the whole submission
/// and refuses another upload until its outstanding work can be drained.
pub(super) fn submit<R: Send + 'static>(
	device: &VulkanDevice,
	resources: R,
	record: impl FnOnce(&R, &Device, vk::CommandBuffer),
) -> Result<R, vk::Result> {
	let raw = device.raw();
	let mut retired = device
		.upload_retirements
		.lock()
		.unwrap_or_else(|error| error.into_inner());
	if !retired.is_empty() {
		match unsafe { raw.device_wait_idle() } {
			Ok(()) => retired.clear(),
			Err(vk::Result::ERROR_DEVICE_LOST) => {
				retired.clear();
				return Err(vk::Result::ERROR_DEVICE_LOST);
			}
			Err(error) => return Err(error),
		}
	}
	let mut submission = Submission {
		device: raw.clone(),
		pool: vk::CommandPool::null(),
		fence: vk::Fence::null(),
		resources: Some(resources),
		_metrics: Vec::new(),
	};
	let pool_info = vk::CommandPoolCreateInfo::default()
		.flags(vk::CommandPoolCreateFlags::TRANSIENT)
		.queue_family_index(device.queue_families().graphics);
	submission.pool = unsafe { raw.create_command_pool(&pool_info, None)? };
	submission._metrics.push(crate::metrics::object(
		raw,
		"command_pool",
		submission.pool,
		"Upload command pool",
		"transient",
	));
	let allocate = vk::CommandBufferAllocateInfo::default()
		.command_pool(submission.pool)
		.level(vk::CommandBufferLevel::PRIMARY)
		.command_buffer_count(1);
	let command = unsafe { raw.allocate_command_buffers(&allocate)? }[0];
	submission._metrics.push(crate::metrics::object(
		raw,
		"command_buffer",
		command,
		"Upload commands",
		"transient",
	));
	unsafe {
		raw.begin_command_buffer(
			command,
			&vk::CommandBufferBeginInfo::default()
				.flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
		)?;
	}
	record(
		submission
			.resources
			.as_ref()
			.expect("upload resources exist"),
		raw,
		command,
	);
	unsafe {
		raw.end_command_buffer(command)?;
	}
	submission.fence = unsafe { raw.create_fence(&vk::FenceCreateInfo::default(), None)? };
	submission._metrics.push(crate::metrics::object(
		raw,
		"fence",
		submission.fence,
		"Upload completion",
		"",
	));
	let commands = [command];
	let info = vk::SubmitInfo::default().command_buffers(&commands);
	unsafe {
		raw.queue_submit(device.graphics_queue(), &[info], submission.fence)?;
	}
	if let Err(error) = unsafe { raw.wait_for_fences(&[submission.fence], true, u64::MAX) } {
		if error != vk::Result::ERROR_DEVICE_LOST {
			match unsafe { raw.queue_wait_idle(device.graphics_queue()) } {
				Ok(()) | Err(vk::Result::ERROR_DEVICE_LOST) => {}
				Err(_) => retired.push(Box::new(submission)),
			}
		}
		return Err(error);
	}
	Ok(submission
		.resources
		.take()
		.expect("completed upload owns its resources"))
}

pub(super) fn copy_buffer(
	raw: &Device,
	command: vk::CommandBuffer,
	source: &VulkanBuffer,
	destination: &VulkanBuffer,
) {
	unsafe {
		raw.cmd_copy_buffer(
			command,
			source.raw(),
			destination.raw(),
			&[vk::BufferCopy::default().size(source.size())],
		);
		let barrier = vk::BufferMemoryBarrier::default()
			.src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
			.dst_access_mask(vk::AccessFlags::VERTEX_ATTRIBUTE_READ | vk::AccessFlags::INDEX_READ)
			.src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
			.dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
			.buffer(destination.raw())
			.offset(0)
			.size(vk::WHOLE_SIZE);
		raw.cmd_pipeline_barrier(
			command,
			vk::PipelineStageFlags::TRANSFER,
			vk::PipelineStageFlags::VERTEX_INPUT,
			vk::DependencyFlags::empty(),
			&[],
			&[barrier],
			&[],
		);
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::vulkan::test_driver::{device, reset, state};
	use std::sync::{
		Arc,
		atomic::{AtomicUsize, Ordering},
	};
	struct Resource(Arc<AtomicUsize>);
	impl Drop for Resource {
		fn drop(&mut self) {
			self.0.fetch_add(1, Ordering::SeqCst);
		}
	}

	#[test]
	fn ready_on_return_uses_one_submission_and_drops_only_transient_handles() {
		reset();
		let device = device();
		let dropped = Arc::new(AtomicUsize::new(0));
		let result = submit(&device, Resource(Arc::clone(&dropped)), |_, _, _| {}).unwrap();
		assert_eq!(dropped.load(Ordering::SeqCst), 0);
		state(|state| {
			assert_eq!(state.calls["submit"], 1);
			assert_eq!(state.calls["wait"], 1);
			assert!(!state.calls.contains_key("queue_idle"));
			assert_eq!(state.released, ["fence", "pool"]);
		});
		drop(result);
		assert_eq!(dropped.load(Ordering::SeqCst), 1);
	}

	#[test]
	fn every_presubmission_failure_rolls_back_resources_and_handles() {
		for call in [
			"create_pool",
			"allocate_commands",
			"begin_commands",
			"end_commands",
			"create_fence",
			"submit",
		] {
			reset();
			state(|state| state.fail = Some((call, 1)));
			let device = device();
			let dropped = Arc::new(AtomicUsize::new(0));
			assert!(submit(&device, Resource(Arc::clone(&dropped)), |_, _, _| {}).is_err());
			assert_eq!(dropped.load(Ordering::SeqCst), 1);
			state(|state| {
				assert_eq!(
					state
						.released
						.iter()
						.filter(|&&name| name == "pool")
						.count(),
					usize::from(call != "create_pool")
				);
				assert_eq!(
					state
						.released
						.iter()
						.filter(|&&name| name == "fence")
						.count(),
					usize::from(call == "submit")
				);
				assert!(!state.calls.contains_key("wait"));
			});
		}
	}

	#[test]
	fn uncertain_completion_retains_resources_and_blocks_uploads_until_idle_recovers() {
		reset();
		let device = device();
		state(|state| {
			state.wait_error = Some(vk::Result::ERROR_OUT_OF_HOST_MEMORY);
			state.idle_error = state.wait_error;
		});
		let dropped = Arc::new(AtomicUsize::new(0));
		assert!(submit(&device, Resource(Arc::clone(&dropped)), |_, _, _| {}).is_err());
		assert_eq!(dropped.load(Ordering::SeqCst), 0);
		state(|state| assert!(state.released.is_empty()));
		assert!(
			submit(&device, (), |_, _, _| panic!(
				"retired work must block new submissions"
			))
			.is_err()
		);
		state(|state| assert_eq!(state.calls["submit"], 1));
		assert_eq!(dropped.load(Ordering::SeqCst), 0);
		state(|state| {
			state.wait_error = None;
			state.idle_error = None;
		});
		submit(&device, (), |_, _, _| {}).unwrap();
		assert_eq!(dropped.load(Ordering::SeqCst), 1);
		assert!(device.upload_retirements.lock().unwrap().is_empty());
	}

	#[test]
	fn wait_error_with_confirmed_idle_or_device_loss_can_release_resources() {
		for error in [
			vk::Result::ERROR_OUT_OF_HOST_MEMORY,
			vk::Result::ERROR_DEVICE_LOST,
		] {
			reset();
			let device = device();
			state(|state| state.wait_error = Some(error));
			let dropped = Arc::new(AtomicUsize::new(0));
			assert!(submit(&device, Resource(Arc::clone(&dropped)), |_, _, _| {}).is_err());
			assert_eq!(dropped.load(Ordering::SeqCst), 1);
			assert!(device.upload_retirements.lock().unwrap().is_empty());
		}
	}
}
