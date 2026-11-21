//! A small Vulkan dispatch stub for deterministic allocation/wait failures.
use super::VulkanDevice;
use ash::{
	Device,
	vk::{self, Handle},
};
use std::{cell::RefCell, collections::HashMap, ffi::c_void};

#[derive(Default)]
pub(super) struct State {
	pub calls: HashMap<&'static str, usize>,
	pub fail: Option<(&'static str, usize)>,
	pub released: Vec<&'static str>,
	pub memory: HashMap<u64, Box<[u8]>>,
	buffers: HashMap<u64, u64>,
	acquired: std::collections::HashSet<u64>,
	next: u64,
	pub wait_error: Option<vk::Result>,
	pub waited_fences: Vec<Vec<u64>>,
	pub reset_fence_groups: Vec<Vec<u64>>,
	pub idle_error: Option<vk::Result>,
}
thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }
pub(super) fn state<R>(f: impl FnOnce(&mut State) -> R) -> R {
	STATE.with(|state| f(&mut state.borrow_mut()))
}
pub(super) fn reset() {
	state(|state| *state = State::default());
}
fn called(name: &'static str) -> Result<(), vk::Result> {
	state(|state| {
		let count = state.calls.entry(name).or_default();
		*count += 1;
		if state.fail == Some((name, *count)) {
			Err(vk::Result::ERROR_OUT_OF_HOST_MEMORY)
		} else {
			Ok(())
		}
	})
}
fn handle() -> u64 {
	state(|state| {
		state.next += 1;
		state.next
	})
}
fn release(name: &'static str) {
	state(|state| state.released.push(name));
}

pub(super) fn raw() -> Device {
	unsafe {
		Device::load_with(
			|name| {
				match name.to_bytes() {
					b"vkCreateBuffer" => create_buffer as *const (),
					b"vkDestroyBuffer" => destroy_buffer as *const (),
					b"vkGetBufferMemoryRequirements" => buffer_requirements as *const (),
					b"vkAllocateMemory" => allocate_memory as *const (),
					b"vkFreeMemory" => free_memory as *const (),
					b"vkBindBufferMemory" => bind_memory as *const (),
					b"vkMapMemory" => map_memory as *const (),
					b"vkUnmapMemory" => unmap_memory as *const (),
					b"vkCreateCommandPool" => create_pool as *const (),
					b"vkDestroyCommandPool" => destroy_pool as *const (),
					b"vkAllocateCommandBuffers" => allocate_commands as *const (),
					b"vkCreateSemaphore" => create_semaphore as *const (),
					b"vkDestroySemaphore" => destroy_semaphore as *const (),
					b"vkCreateFence" => create_fence as *const (),
					b"vkDestroyFence" => destroy_fence as *const (),
					b"vkBeginCommandBuffer" => begin_commands as *const (),
					b"vkEndCommandBuffer" => end_commands as *const (),
					b"vkQueueSubmit" => submit as *const (),
					b"vkWaitForFences" => wait as *const (),
					b"vkQueueWaitIdle" => queue_idle as *const (),
					b"vkDeviceWaitIdle" => device_idle as *const (),
					b"vkDestroyDevice" => destroy_device as *const (),
					b"vkCreateImageView" => create_view as *const (),
					b"vkDestroyImageView" => destroy_view as *const (),
					b"vkResetCommandPool" => reset_pool as *const (),
					b"vkResetFences" => reset_fences as *const (),
					b"vkCmdPipelineBarrier" => barrier as *const (),
					b"vkCmdBeginRendering" => begin_rendering as *const (),
					b"vkCmdEndRendering" => end_rendering as *const (),
					b"vkCmdSetViewport" => viewport as *const (),
					b"vkCmdSetScissor" => scissor as *const (),
					b"vkDestroyDescriptorPool" => destroy_descriptor_pool as *const (),
					b"vkDestroyDescriptorSetLayout" => destroy_descriptor_layout as *const (),
					b"vkDestroyPipeline" => destroy_pipeline as *const (),
					b"vkDestroyPipelineLayout" => destroy_pipeline_layout as *const (),
					b"vkDestroyImage" => destroy_image as *const (),
					_ => std::ptr::null(),
				}
				.cast::<c_void>()
			},
			vk::Device::from_raw(1),
		)
	}
}
pub(super) fn device() -> VulkanDevice {
	VulkanDevice::for_test(raw())
}

unsafe extern "system" fn create_buffer(
	_: vk::Device,
	info: *const vk::BufferCreateInfo<'_>,
	_: *const vk::AllocationCallbacks<'_>,
	out: *mut vk::Buffer,
) -> vk::Result {
	if let Err(error) = called("create_buffer") {
		return error;
	}
	let id = handle();
	state(|state| {
		state.buffers.insert(id, unsafe { (*info).size });
	});
	unsafe {
		*out = vk::Buffer::from_raw(id);
	}
	vk::Result::SUCCESS
}
unsafe extern "system" fn destroy_buffer(
	_: vk::Device,
	buffer: vk::Buffer,
	_: *const vk::AllocationCallbacks<'_>,
) {
	state(|state| {
		state.buffers.remove(&buffer.as_raw());
	});
	release("buffer");
}
unsafe extern "system" fn buffer_requirements(
	_: vk::Device,
	buffer: vk::Buffer,
	out: *mut vk::MemoryRequirements,
) {
	unsafe {
		*out = vk::MemoryRequirements {
			size: state(|state| state.buffers[&buffer.as_raw()]),
			alignment: 1,
			memory_type_bits: 1,
		};
	}
}
unsafe extern "system" fn allocate_memory(
	_: vk::Device,
	info: *const vk::MemoryAllocateInfo<'_>,
	_: *const vk::AllocationCallbacks<'_>,
	out: *mut vk::DeviceMemory,
) -> vk::Result {
	if let Err(error) = called("allocate_memory") {
		return error;
	}
	let id = handle();
	state(|state| {
		state.memory.insert(
			id,
			vec![0; unsafe { (*info).allocation_size as usize }].into_boxed_slice(),
		);
	});
	unsafe {
		*out = vk::DeviceMemory::from_raw(id);
	}
	vk::Result::SUCCESS
}
unsafe extern "system" fn free_memory(
	_: vk::Device,
	memory: vk::DeviceMemory,
	_: *const vk::AllocationCallbacks<'_>,
) {
	state(|state| {
		state.memory.remove(&memory.as_raw());
	});
	release("memory");
}
unsafe extern "system" fn bind_memory(
	_: vk::Device,
	_: vk::Buffer,
	_: vk::DeviceMemory,
	_: u64,
) -> vk::Result {
	called("bind_memory").err().unwrap_or(vk::Result::SUCCESS)
}
unsafe extern "system" fn map_memory(
	_: vk::Device,
	memory: vk::DeviceMemory,
	offset: u64,
	_: u64,
	_: vk::MemoryMapFlags,
	out: *mut *mut c_void,
) -> vk::Result {
	if let Err(error) = called("map_memory") {
		return error;
	}
	unsafe {
		*out = state(|state| {
			state
				.memory
				.get_mut(&memory.as_raw())
				.unwrap()
				.as_mut_ptr()
				.add(offset as usize)
				.cast()
		});
	}
	vk::Result::SUCCESS
}
unsafe extern "system" fn unmap_memory(_: vk::Device, _: vk::DeviceMemory) {
	release("unmap");
}
unsafe extern "system" fn create_pool(
	_: vk::Device,
	_: *const vk::CommandPoolCreateInfo<'_>,
	_: *const vk::AllocationCallbacks<'_>,
	out: *mut vk::CommandPool,
) -> vk::Result {
	if let Err(error) = called("create_pool") {
		return error;
	}
	unsafe {
		*out = vk::CommandPool::from_raw(handle());
	}
	vk::Result::SUCCESS
}
unsafe extern "system" fn destroy_pool(
	_: vk::Device,
	_: vk::CommandPool,
	_: *const vk::AllocationCallbacks<'_>,
) {
	release("pool");
}
unsafe extern "system" fn allocate_commands(
	_: vk::Device,
	_: *const vk::CommandBufferAllocateInfo<'_>,
	out: *mut vk::CommandBuffer,
) -> vk::Result {
	if let Err(error) = called("allocate_commands") {
		return error;
	}
	unsafe {
		*out = vk::CommandBuffer::from_raw(handle());
	}
	vk::Result::SUCCESS
}
unsafe extern "system" fn create_semaphore(
	_: vk::Device,
	_: *const vk::SemaphoreCreateInfo<'_>,
	_: *const vk::AllocationCallbacks<'_>,
	out: *mut vk::Semaphore,
) -> vk::Result {
	if let Err(error) = called("create_semaphore") {
		return error;
	}
	unsafe {
		*out = vk::Semaphore::from_raw(handle());
	}
	vk::Result::SUCCESS
}
unsafe extern "system" fn destroy_semaphore(
	_: vk::Device,
	semaphore: vk::Semaphore,
	_: *const vk::AllocationCallbacks<'_>,
) {
	state(|state| {
		state.acquired.remove(&semaphore.as_raw());
	});
	release("semaphore");
}
unsafe extern "system" fn create_fence(
	_: vk::Device,
	_: *const vk::FenceCreateInfo<'_>,
	_: *const vk::AllocationCallbacks<'_>,
	out: *mut vk::Fence,
) -> vk::Result {
	if let Err(error) = called("create_fence") {
		return error;
	}
	unsafe {
		*out = vk::Fence::from_raw(handle());
	}
	vk::Result::SUCCESS
}
unsafe extern "system" fn destroy_fence(
	_: vk::Device,
	_: vk::Fence,
	_: *const vk::AllocationCallbacks<'_>,
) {
	release("fence");
}
unsafe extern "system" fn begin_commands(
	_: vk::CommandBuffer,
	_: *const vk::CommandBufferBeginInfo<'_>,
) -> vk::Result {
	called("begin_commands")
		.err()
		.unwrap_or(vk::Result::SUCCESS)
}
unsafe extern "system" fn end_commands(_: vk::CommandBuffer) -> vk::Result {
	called("end_commands").err().unwrap_or(vk::Result::SUCCESS)
}
unsafe extern "system" fn submit(
	_: vk::Queue,
	count: u32,
	info: *const vk::SubmitInfo<'_>,
	_: vk::Fence,
) -> vk::Result {
	if let Err(error) = called("submit") {
		return error;
	}
	for info in unsafe { std::slice::from_raw_parts(info, count as usize) } {
		for index in 0..info.wait_semaphore_count as usize {
			let semaphore = unsafe { *info.p_wait_semaphores.add(index) };
			state(|state| {
				state.acquired.remove(&semaphore.as_raw());
			});
		}
	}
	vk::Result::SUCCESS
}
unsafe extern "system" fn wait(
	_: vk::Device,
	count: u32,
	fences: *const vk::Fence,
	_: u32,
	_: u64,
) -> vk::Result {
	let _ = called("wait");
	state(|state| {
		state.waited_fences.push(
			unsafe { std::slice::from_raw_parts(fences, count as usize) }
				.iter()
				.map(|fence| fence.as_raw())
				.collect(),
		)
	});
	state(|state| state.wait_error.unwrap_or(vk::Result::SUCCESS))
}
unsafe extern "system" fn queue_idle(_: vk::Queue) -> vk::Result {
	let _ = called("queue_idle");
	state(|state| state.idle_error.unwrap_or(vk::Result::SUCCESS))
}
unsafe extern "system" fn device_idle(_: vk::Device) -> vk::Result {
	let _ = called("device_idle");
	state(|state| state.idle_error.unwrap_or(vk::Result::SUCCESS))
}
unsafe extern "system" fn destroy_device(_: vk::Device, _: *const vk::AllocationCallbacks<'_>) {
	release("device");
}
unsafe extern "system" fn create_view(
	_: vk::Device,
	_: *const vk::ImageViewCreateInfo<'_>,
	_: *const vk::AllocationCallbacks<'_>,
	out: *mut vk::ImageView,
) -> vk::Result {
	if let Err(error) = called("create_view") {
		return error;
	}
	unsafe {
		*out = vk::ImageView::from_raw(handle());
	}
	vk::Result::SUCCESS
}
unsafe extern "system" fn destroy_view(
	_: vk::Device,
	_: vk::ImageView,
	_: *const vk::AllocationCallbacks<'_>,
) {
	release("view");
}

pub(super) fn instance() -> ash::Instance {
	unsafe {
		ash::Instance::load_with(
			|name| {
				if name.to_bytes() == b"vkGetDeviceProcAddr" {
					get_device_proc as *const () as *const c_void
				} else {
					std::ptr::null()
				}
			},
			vk::Instance::from_raw(1),
		)
	}
}

unsafe extern "system" fn get_device_proc(
	_: vk::Device,
	name: *const std::ffi::c_char,
) -> vk::PFN_vkVoidFunction {
	let pointer = match unsafe { std::ffi::CStr::from_ptr(name) }.to_bytes() {
		b"vkAcquireNextImageKHR" => acquire as *const (),
		b"vkQueuePresentKHR" => present as *const (),
		b"vkDestroySwapchainKHR" => destroy_swapchain as *const (),
		_ => std::ptr::null(),
	};
	unsafe { std::mem::transmute::<*const (), vk::PFN_vkVoidFunction>(pointer) }
}

unsafe extern "system" fn acquire(
	_: vk::Device,
	_: vk::SwapchainKHR,
	_: u64,
	semaphore: vk::Semaphore,
	_: vk::Fence,
	index: *mut u32,
) -> vk::Result {
	if let Err(error) = called("acquire") {
		return error;
	}
	if !state(|state| state.acquired.insert(semaphore.as_raw())) {
		return vk::Result::ERROR_VALIDATION_FAILED_EXT;
	}
	unsafe {
		*index = 0;
	}
	vk::Result::SUCCESS
}
unsafe extern "system" fn present(_: vk::Queue, _: *const vk::PresentInfoKHR<'_>) -> vk::Result {
	called("present").err().unwrap_or(vk::Result::SUCCESS)
}
unsafe extern "system" fn destroy_swapchain(
	_: vk::Device,
	_: vk::SwapchainKHR,
	_: *const vk::AllocationCallbacks<'_>,
) {
	release("swapchain");
}
unsafe extern "system" fn reset_pool(
	_: vk::Device,
	_: vk::CommandPool,
	_: vk::CommandPoolResetFlags,
) -> vk::Result {
	called("reset_pool").err().unwrap_or(vk::Result::SUCCESS)
}
unsafe extern "system" fn reset_fences(
	_: vk::Device,
	count: u32,
	fences: *const vk::Fence,
) -> vk::Result {
	state(|state| {
		state.reset_fence_groups.push(
			unsafe { std::slice::from_raw_parts(fences, count as usize) }
				.iter()
				.map(|fence| fence.as_raw())
				.collect(),
		)
	});
	called("reset_fences").err().unwrap_or(vk::Result::SUCCESS)
}
unsafe extern "system" fn barrier(
	_: vk::CommandBuffer,
	_: vk::PipelineStageFlags,
	_: vk::PipelineStageFlags,
	_: vk::DependencyFlags,
	_: u32,
	_: *const vk::MemoryBarrier<'_>,
	_: u32,
	_: *const vk::BufferMemoryBarrier<'_>,
	_: u32,
	_: *const vk::ImageMemoryBarrier<'_>,
) {
}
unsafe extern "system" fn begin_rendering(_: vk::CommandBuffer, _: *const vk::RenderingInfo<'_>) {}
unsafe extern "system" fn end_rendering(_: vk::CommandBuffer) {}
unsafe extern "system" fn viewport(_: vk::CommandBuffer, _: u32, _: u32, _: *const vk::Viewport) {}
unsafe extern "system" fn scissor(_: vk::CommandBuffer, _: u32, _: u32, _: *const vk::Rect2D) {}
unsafe extern "system" fn destroy_descriptor_pool(
	_: vk::Device,
	_: vk::DescriptorPool,
	_: *const vk::AllocationCallbacks<'_>,
) {
	release("descriptor_pool");
}
unsafe extern "system" fn destroy_descriptor_layout(
	_: vk::Device,
	_: vk::DescriptorSetLayout,
	_: *const vk::AllocationCallbacks<'_>,
) {
	release("descriptor_layout");
}
unsafe extern "system" fn destroy_pipeline(
	_: vk::Device,
	_: vk::Pipeline,
	_: *const vk::AllocationCallbacks<'_>,
) {
	release("pipeline");
}
unsafe extern "system" fn destroy_pipeline_layout(
	_: vk::Device,
	_: vk::PipelineLayout,
	_: *const vk::AllocationCallbacks<'_>,
) {
	release("pipeline_layout");
}
unsafe extern "system" fn destroy_image(
	_: vk::Device,
	_: vk::Image,
	_: *const vk::AllocationCallbacks<'_>,
) {
	release("image");
}
