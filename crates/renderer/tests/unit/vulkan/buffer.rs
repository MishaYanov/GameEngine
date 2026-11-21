use super::*;
use crate::vulkan::test_driver::{device, reset, state};

#[test]
fn coherent_buffer_maps_once_keeps_writes_bounded_and_unmaps_before_free() {
	reset();
	let device = device();
	let buffer = VulkanBuffer::host_vertex(&device, 4).unwrap();
	buffer.write(&[1u8, 2, 3, 4]).unwrap();
	buffer.write(&[9u8, 8]).unwrap();
	buffer.write::<u8>(&[]).unwrap();
	assert!(matches!(
		buffer.write(&[0u8; 5]),
		Err(VulkanBufferError::DataTooLarge { .. })
	));
	state(|state| {
		assert_eq!(state.calls["map_memory"], 1);
		assert_eq!(
			state.memory.values().next().unwrap().as_ref(),
			&[9, 8, 3, 4]
		);
		assert!(state.released.is_empty());
	});
	drop(buffer);
	state(|state| {
		assert_eq!(state.released, ["unmap", "buffer", "memory"]);
		assert!(state.memory.is_empty());
	});
	fn assert_send_sync<T: Send + Sync>() {}
	assert_send_sync::<VulkanBuffer>();
}

#[test]
fn failed_persistent_map_releases_the_bound_buffer_and_memory() {
	reset();
	state(|state| state.fail = Some(("map_memory", 1)));
	let device = device();
	assert!(VulkanBuffer::host_vertex(&device, 4).is_err());
	state(|state| {
		assert_eq!(state.released, ["buffer", "memory"]);
		assert!(state.memory.is_empty());
	});
}
