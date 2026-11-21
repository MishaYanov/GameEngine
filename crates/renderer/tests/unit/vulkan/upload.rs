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
