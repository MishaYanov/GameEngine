use ash::vk::Handle;

use super::*;
use crate::vulkan::test_driver::{device, reset, state};

#[test]
fn resized_frames_keep_warmed_instance_storage_with_fresh_image_synchronization() {
	reset();
	let device = device();
	let mut frame = VulkanFrame::with_image_count(&device, 3).unwrap();
	let instances = frame.instances.as_mut().unwrap();
	instances.buffer =
		VulkanBuffer::host_vertex(&device, (512 * size_of::<InstanceData>()) as u64).unwrap();
	instances.capacity = 512;
	instances.data.reserve(512);
	instances.order.reserve(512);
	instances.batches.reserve(512);
	let buffer = instances.buffer.raw();
	let scratch = (
		instances.data.as_ptr(),
		instances.order.as_ptr(),
		instances.batches.as_ptr(),
	);
	let allocations = state(|state| state.calls["allocate_memory"]);
	for image_count in [2, 4, 3] {
		let acquisition = frame.image_available;
		let fence = frame.in_flight;
		frame = VulkanFrame::with_storage(&device, image_count, Some(frame)).unwrap();
		let instances = frame.instances.as_ref().unwrap();
		assert_eq!(instances.buffer.raw(), buffer);
		assert_eq!(instances.capacity, 512);
		assert_eq!(
			(
				instances.data.as_ptr(),
				instances.order.as_ptr(),
				instances.batches.as_ptr()
			),
			scratch
		);
		assert_eq!(frame.render_finished.len(), image_count);
		assert_ne!(frame.image_available, acquisition);
		assert_ne!(frame.in_flight, fence);
		frame.wait_complete().unwrap();
		state(|state| {
			assert_eq!(state.calls["allocate_memory"], allocations);
			assert!(
				!state.calls.contains_key("wait"),
				"new synchronization has no pending work"
			);
		});
	}
	drop(frame);
	state(|state| assert!(state.memory.is_empty()));
}

#[test]
fn failed_resize_construction_releases_transferred_storage_and_can_retry() {
	for (call, offset) in [
		("create_pool", 1),
		("create_semaphore", 2),
		("create_fence", 2),
	] {
		reset();
		let device = device();
		let frame = VulkanFrame::with_image_count(&device, 3).unwrap();
		state(|state| state.fail = Some((call, state.calls[call] + offset)));
		assert!(VulkanFrame::with_storage(&device, 2, Some(frame)).is_err());
		state(|state| {
			assert!(state.memory.is_empty(), "{call}");
			state.fail = None;
		});
		let frame = VulkanFrame::with_image_count(&device, 2).unwrap();
		assert_eq!(frame.render_finished.len(), 2);
		drop(frame);
		state(|state| assert!(state.memory.is_empty()));
	}
}

#[test]
fn successful_completion_is_reused_between_updates_but_failed_waits_are_retried() {
	fn assert_send_sync<T: Send + Sync>() {}
	assert_send_sync::<VulkanFrame>();
	reset();
	let device = device();
	let swapchain = VulkanSwapchain::for_test(device.raw());
	let camera = VulkanCamera::for_test(&device);
	let depth = VulkanDepthBuffer::for_test(device.raw());
	let pipelines = VulkanMaterialPipelines::for_test(device.raw());
	let mut frame = VulkanFrame::new(&device, &swapchain).unwrap();
	let uniform = CameraUniform {
		view_projection: glam::Mat4::IDENTITY.to_cols_array(),
	};
	for completed in 0..3 {
		frame
			.draw(
				&device,
				FrameTargets {
					swapchain: &swapchain,
					depth_buffer: &depth,
				},
				&pipelines,
				&camera,
				&uniform,
				&[],
			)
			.unwrap();
		state(|state| assert_eq!(state.calls.get("wait").copied().unwrap_or(0), completed * 2));
		state(|state| state.wait_error = Some(vk::Result::ERROR_OUT_OF_HOST_MEMORY));
		assert!(frame.wait_complete().is_err());
		state(|state| state.wait_error = None);
		frame.wait_complete().unwrap();
		frame.wait_complete().unwrap();
		state(|state| assert_eq!(state.calls["wait"], (completed + 1) * 2));
	}
}

#[test]
fn successive_draws_wait_and_reset_acquisition_and_render_fences_together() {
	reset();
	let device = device();
	let swapchain = VulkanSwapchain::for_test(device.raw());
	let camera = VulkanCamera::for_test(&device);
	let depth = VulkanDepthBuffer::for_test(device.raw());
	let pipelines = VulkanMaterialPipelines::for_test(device.raw());
	let mut frame = VulkanFrame::new(&device, &swapchain).unwrap();
	let uniform = CameraUniform {
		view_projection: glam::Mat4::IDENTITY.to_cols_array(),
	};
	for _ in 0..2 {
		assert_eq!(
			frame
				.draw(
					&device,
					FrameTargets {
						swapchain: &swapchain,
						depth_buffer: &depth
					},
					&pipelines,
					&camera,
					&uniform,
					&[]
				)
				.unwrap(),
			FrameStatus::Rendered
		);
	}
	state(|state| {
		let pair = vec![frame.acquisition_fence.as_raw(), frame.in_flight.as_raw()];
		assert_eq!(
			state.waited_fences.as_slice(),
			std::slice::from_ref(&pair),
			"one combined wait before the second frame"
		);
		assert_eq!(
			state.reset_fence_groups,
			[pair.clone(), pair],
			"one combined reset per frame"
		);
		assert_eq!(state.calls["acquire"], 2);
		assert_eq!(state.calls["submit"], 2);
		assert_eq!(state.calls["present"], 2);
	});
	frame.wait_complete().unwrap();
}

#[test]
fn failed_public_draw_never_reuses_an_acquired_semaphore_or_unsubmitted_fence() {
	for call in [
		"acquire",
		"reset_pool",
		"begin_commands",
		"end_commands",
		"reset_fences",
		"submit",
		"present",
	] {
		reset();
		let device = device();
		let swapchain = VulkanSwapchain::for_test(device.raw());
		let camera = VulkanCamera::for_test(&device);
		let depth = VulkanDepthBuffer::for_test(device.raw());
		let pipelines = VulkanMaterialPipelines::for_test(device.raw());
		let mut frame = VulkanFrame::new(&device, &swapchain).unwrap();
		let uniform = CameraUniform {
			view_projection: glam::Mat4::IDENTITY.to_cols_array(),
		};
		let draw = |frame: &mut VulkanFrame| {
			frame.draw(
				&device,
				FrameTargets {
					swapchain: &swapchain,
					depth_buffer: &depth,
				},
				&pipelines,
				&camera,
				&uniform,
				&[],
			)
		};
		state(|state| state.fail = Some((call, 1)));
		assert!(
			matches!(
				draw(&mut frame),
				Err(VulkanFrameError::Vulkan(
					vk::Result::ERROR_OUT_OF_HOST_MEMORY
				))
			),
			"{call}"
		);
		let before_retry = state(|state| state.calls.clone());
		assert_eq!(
			draw(&mut frame).unwrap(),
			FrameStatus::SwapchainNeedsRebuild
		);
		state(|state| {
			assert_eq!(
				state.calls, before_retry,
				"invalidated frame must not issue more Vulkan calls after {call}"
			)
		});
		frame.wait_complete().unwrap();
		state(|state| {
			let expected = match call {
				"acquire" | "reset_fences" => vec![],
				"present" => vec![vec![
					frame.acquisition_fence.as_raw(),
					frame.in_flight.as_raw(),
				]],
				_ => vec![vec![frame.acquisition_fence.as_raw()]],
			};
			assert_eq!(
				state.waited_fences, expected,
				"wait only for genuinely pending acquisition/submission after {call}"
			);
		});
		// WindowRenderer waits device-idle before replacing failed targets.
		unsafe {
			device.raw().device_wait_idle().unwrap();
		}
		let previous_buffer = frame.instances.as_ref().unwrap().buffer.raw();
		let mut fresh = VulkanFrame::recreate(&device, &swapchain, frame).unwrap();
		assert_ne!(
			fresh.instances.as_ref().unwrap().buffer.raw(),
			previous_buffer,
			"failed-frame recovery must keep its fresh allocation path"
		);
		assert_eq!(
			draw(&mut fresh).unwrap(),
			FrameStatus::Rendered,
			"fresh synchronization must recover after {call}"
		);
		fresh.wait_complete().unwrap();
	}
}

#[test]
fn every_partial_frame_allocation_releases_preceding_handles() {
	for (call, nth, semaphores, pools) in [
		("create_pool", 1, 0, 0),
		("allocate_commands", 1, 0, 1),
		("create_semaphore", 1, 0, 1),
		("create_semaphore", 2, 1, 1),
		("create_semaphore", 3, 2, 1),
		("create_semaphore", 4, 3, 1),
		("create_fence", 1, 4, 1),
		("create_fence", 2, 4, 1),
	] {
		reset();
		state(|state| state.fail = Some((call, nth)));
		let device = device();
		assert!(
			VulkanFrame::with_image_count(&device, 3).is_err(),
			"{call} {nth}"
		);
		state(|state| {
			assert_eq!(
				state
					.released
					.iter()
					.filter(|&&name| name == "semaphore")
					.count(),
				semaphores
			);
			assert_eq!(
				state
					.released
					.iter()
					.filter(|&&name| name == "pool")
					.count(),
				pools
			);
			assert!(state.memory.is_empty());
			assert_eq!(
				state
					.released
					.iter()
					.filter(|&&name| name == "fence")
					.count(),
				usize::from(call == "create_fence" && nth == 2)
			);
		});
	}
}
