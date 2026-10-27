//! Optional instrumentation shared by renderer owners and raw-device helpers.
use ash::{Device, vk};
use engine_logger::metrics::{MetricsSnapshot, ResourceGuard, ResourceMetrics, ResourceRecord};
use std::{
	collections::HashMap,
	sync::{Mutex, OnceLock},
};

static TRACKERS: OnceLock<Mutex<HashMap<u64, ResourceMetrics>>> = OnceLock::new();

fn tracker(device: &Device) -> Option<ResourceMetrics> {
	use vk::Handle;
	TRACKERS
		.get()?
		.lock()
		.ok()?
		.get(&device.handle().as_raw())
		.cloned()
}

pub(crate) fn enabled(
	debug_build: bool,
	log_mode: Option<&str>,
	override_mode: Option<&str>,
) -> Result<bool, &'static str> {
	match override_mode {
		Some("1") => Ok(true),
		Some("0") => Ok(false),
		None => Ok(debug_build || log_mode == Some("debug")),
		_ => Err("ENGINE_METRICS must be '0' or '1'; resource metrics disabled"),
	}
}

pub(crate) fn register(device: &Device, gpu_name: &str) -> ResourceGuard {
	use vk::Handle;
	let enabled = match enabled(
		cfg!(debug_assertions),
		std::env::var("ENGINE_LOG_MODE").ok().as_deref(),
		std::env::var("ENGINE_METRICS").ok().as_deref(),
	) {
		Ok(value) => value,
		Err(message) => {
			engine_logger::warn!(target: "vulkan_metrics", "{message}");
			false
		}
	};
	if !enabled {
		return ResourceGuard::disabled();
	}
	let Some(metrics) = ResourceMetrics::new(gpu_name) else {
		engine_logger::warn!(target: "vulkan_metrics", "Resource metrics device limit reached; this device will not be recorded");
		return ResourceGuard::disabled();
	};
	TRACKERS
		.get_or_init(|| Mutex::new(HashMap::new()))
		.lock()
		.unwrap_or_else(|e| e.into_inner())
		.insert(device.handle().as_raw(), metrics);
	engine_logger::info!(target: "vulkan_metrics", "Vulkan resource tracking enabled; snapshots are written beside session logs");
	object(
		device,
		"device",
		device.handle(),
		"Logical Vulkan device",
		"",
	)
}
pub(crate) fn finish(device: &Device) {
	use vk::Handle;
	if let Some(trackers) = TRACKERS.get()
		&& let Some(metrics) = trackers
			.lock()
			.unwrap_or_else(|e| e.into_inner())
			.remove(&device.handle().as_raw())
	{
		metrics.close();
	}
}
pub(crate) fn snapshot(device: &Device) -> Option<MetricsSnapshot> {
	tracker(device).map(|t| t.snapshot())
}

pub(crate) fn object<H: vk::Handle>(
	device: &Device,
	kind: &str,
	handle: H,
	label: &str,
	details: &str,
) -> ResourceGuard {
	let Some(metrics) = tracker(device) else {
		return ResourceGuard::disabled();
	};
	metrics.track(ResourceRecord {
		id: 0,
		kind: kind.into(),
		handle: format!("0x{:x}", handle.as_raw()),
		label: label.into(),
		details: details.into(),
		payload_bytes: 0,
		allocation_bytes: 0,
		memory_type: None,
		memory_heap: None,
		device_local: false,
		host_visible: false,
		created_ms: 0,
		released_ms: None,
	})
}
pub(crate) fn allocation<H: vk::Handle>(
	device: &super::vulkan::VulkanDevice,
	kind: &str,
	handle: H,
	payload: u64,
	bytes: u64,
	memory_type: u32,
	details: &str,
) -> ResourceGuard {
	let Some(metrics) = tracker(device.raw()) else {
		return ResourceGuard::disabled();
	};
	let memory = device.memory_properties().memory_types[memory_type as usize];
	metrics.track(ResourceRecord {
		id: 0,
		kind: kind.into(),
		handle: format!("0x{:x}", handle.as_raw()),
		label: kind.into(),
		details: details.into(),
		payload_bytes: payload,
		allocation_bytes: bytes,
		memory_type: Some(memory_type),
		memory_heap: Some(memory.heap_index),
		device_local: memory
			.property_flags
			.contains(vk::MemoryPropertyFlags::DEVICE_LOCAL),
		host_visible: memory
			.property_flags
			.contains(vk::MemoryPropertyFlags::HOST_VISIBLE),
		created_ms: 0,
		released_ms: None,
	})
}
pub(crate) fn frame(device: &Device, metrics: super::vulkan::DrawMetrics) {
	if let Some(tracker) = tracker(device) {
		tracker.frame(
			metrics.draw_calls,
			metrics.instances,
			metrics.triangles,
			metrics.gpu_time_ms,
		);
	}
}

#[cfg(test)]
mod tests {
	use super::enabled;
	#[test]
	fn selection_is_explicit_and_independent_of_logging_filter() {
		assert_eq!(enabled(true, None, None), Ok(true));
		assert_eq!(enabled(false, Some("debug"), None), Ok(true));
		assert_eq!(enabled(false, None, None), Ok(false));
		assert_eq!(enabled(true, Some("debug"), Some("0")), Ok(false));
		assert_eq!(enabled(false, None, Some("1")), Ok(true));
		assert!(enabled(true, None, Some("typo")).is_err());
	}
}
