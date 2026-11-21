use super::{
	DrawStatus, FrameStatus, VulkanFrameError, finish_draw, release_after_idle, retire_external,
};
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};

#[test]
fn content_scissor_respects_attachment_and_device_limits_before_recording() {
	let checked =
		|pixels| super::validated_region(pixels, [4000, 1000], [3200, 1200], [-4096.0, 4000.0]);
	let valid = checked([500, 0, 3000, 1000]).unwrap();
	assert_eq!(
		(
			valid.offset.x,
			valid.offset.y,
			valid.extent.width,
			valid.extent.height
		),
		(500, 0, 3000, 1000)
	);
	for invalid in [
		[500, 0, 0, 1000],
		[500, 1, 3000, 1000],
		[u32::MAX, 0, 2, 1],
		[0, 0, 3201, 1000],
	] {
		assert!(checked(invalid).is_err());
	}
	assert!(
		super::validated_region(
			[900, 0, 3000, 1000],
			[4000, 1000],
			[4096; 2],
			[-3500.0, 3500.0]
		)
		.is_err()
	);
}
struct Resource<'a>(&'a RefCell<Vec<u8>>, u8);
impl Drop for Resource<'_> {
	fn drop(&mut self) {
		self.0.borrow_mut().push(self.1);
	}
}

#[test]
fn renderer_handle_stays_small_for_native_main_thread_startup() {
	// Debug builds retain several constructor/move temporaries on the
	// Windows main thread's 1 MiB stack. Keep the nested GPU ownership
	// tree out of the handle passed through application initialization.
	assert!(size_of::<super::WindowRenderer>() <= 4096);
}

#[test]
fn draw_errors_preserve_the_error_and_force_target_replacement_before_retry() {
	let mut dirty = false;
	let error = ash::vk::Result::ERROR_OUT_OF_HOST_MEMORY;
	assert_eq!(
		finish_draw(&mut dirty, Err(VulkanFrameError::Vulkan(error))).unwrap_err(),
		VulkanFrameError::Vulkan(error).to_string()
	);
	assert!(
		dirty,
		"the next public draw must enter prepare's idle/replacement path"
	);
	dirty = false;
	assert_eq!(
		finish_draw(&mut dirty, Ok(FrameStatus::SwapchainNeedsRebuild)).unwrap(),
		DrawStatus::NeedsRedraw
	);
	assert!(dirty);
	dirty = false;
	assert_eq!(
		finish_draw(&mut dirty, Ok(FrameStatus::Rendered)).unwrap(),
		DrawStatus::Rendered
	);
	assert!(!dirty);
}

#[test]
fn retained_external_children_keep_the_parent_retained_after_later_wait_success() {
	let retained = AtomicBool::new(false);
	let dropped = RefCell::new(Vec::new());
	assert!(
		retire_external(
			Resource(&dropped, 1),
			Err(ash::vk::Result::ERROR_OUT_OF_HOST_MEMORY),
			&retained
		)
		.is_err()
	);
	assert!(retained.load(Ordering::Acquire));
	assert!(dropped.borrow().is_empty());
	retire_external(Resource(&dropped, 2), Ok(()), &retained).unwrap();
	assert!(
		retained.load(Ordering::Acquire),
		"earlier forgotten children still own Vulkan handles"
	);
	assert_eq!(*dropped.borrow(), [2]);
	let retained = AtomicBool::new(false);
	retire_external(
		Resource(&dropped, 3),
		Err(ash::vk::Result::ERROR_DEVICE_LOST),
		&retained,
	)
	.unwrap();
	assert!(!retained.load(Ordering::Acquire));
	assert_eq!(*dropped.borrow(), [2, 3]);
}

#[test]
fn shutdown_releases_dependants_before_parents_only_after_confirmed_completion() {
	for result in [
		Ok(()),
		Err(ash::vk::Result::ERROR_DEVICE_LOST),
		Err(ash::vk::Result::ERROR_OUT_OF_HOST_MEMORY),
	] {
		let dropped = RefCell::new(Vec::new());
		let outcome = release_after_idle((Resource(&dropped, 1), Resource(&dropped, 2)), result);
		if outcome.is_ok() {
			assert_eq!(*dropped.borrow(), [1, 2]);
		} else {
			assert!(dropped.borrow().is_empty());
		}
	}
}
