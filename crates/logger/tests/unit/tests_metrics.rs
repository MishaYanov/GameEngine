use super::*;

fn resource(bytes: u64) -> ResourceRecord {
	ResourceRecord {
		id: 0,
		kind: "buffer".into(),
		handle: "0x1".into(),
		label: "mesh vertices".into(),
		details: "VERTEX_BUFFER".into(),
		payload_bytes: 17,
		allocation_bytes: bytes,
		memory_type: Some(0),
		memory_heap: Some(0),
		device_local: true,
		host_visible: true,
		created_ms: 0,
		released_ms: None,
	}
}

#[test]
fn allocation_totals_include_alignment_and_shared_owners_count_once() {
	let metrics = ResourceMetrics::new("test GPU").unwrap();
	let first = Arc::new(metrics.track(resource(256)));
	let clone = Arc::clone(&first);
	let second = metrics.track(resource(512));
	let snapshot = metrics.snapshot();
	assert_eq!(snapshot.totals.live_resources, 2);
	assert_eq!(snapshot.totals.payload_bytes, 34);
	assert_eq!(snapshot.totals.allocation_bytes, 768);
	assert_eq!(snapshot.totals.device_local_bytes, 768);
	assert_eq!(snapshot.totals.host_visible_bytes, 768);
	drop(clone);
	assert_eq!(metrics.snapshot().totals.live_resources, 2);
	drop(second);
	drop(first);
	metrics.close();
	let snapshot = metrics.snapshot();
	assert!(snapshot.closed);
	assert_eq!(snapshot.totals.live_resources, 0);
	assert_eq!(snapshot.totals.allocation_bytes, 0);
	assert_eq!(snapshot.totals.peak_allocation_bytes, 768);
	assert_eq!(snapshot.totals.allocated_bytes_total, 768);
	assert_eq!(snapshot.totals.released_resources, 2);
	assert!(
		snapshot
			.recent_released
			.iter()
			.all(|r| r.released_ms.is_some())
	);
}

#[test]
fn detail_limits_do_not_change_exact_aggregate_accounting() {
	let metrics = ResourceMetrics::new("test GPU").unwrap();
	let guards: Vec<_> = (0..MAX_RESOURCES + 7)
		.map(|_| metrics.track(resource(256)))
		.collect();
	let snapshot = metrics.snapshot();
	assert!(snapshot.inventory_truncated);
	assert_eq!(snapshot.resources.len(), MAX_RESOURCES);
	assert_eq!(snapshot.totals.live_resources, (MAX_RESOURCES + 7) as u64);
	assert_eq!(
		snapshot.totals.allocation_bytes,
		(MAX_RESOURCES + 7) as u64 * 256
	);
	drop(guards);
	let snapshot = metrics.snapshot();
	assert_eq!(snapshot.totals.allocation_bytes, 0);
	assert_eq!(snapshot.totals.live_resources, 0);
	assert_eq!(snapshot.recent_released.len(), MAX_RELEASED);
	metrics.close();
}

#[test]
fn owned_metric_names_are_strict_and_text_and_frame_samples_are_bounded() {
	assert!(owned_file("vulkan-metrics-1.json"));
	assert!(owned_file("vulkan-metrics-123.tmp"));
	for name in [
		"vulkan-metrics-.json",
		"vulkan-metrics-../other.json",
		"other.json",
		"vulkan-metrics-1.json.bak",
	] {
		assert!(!owned_file(name));
	}
	let metrics = ResourceMetrics::new(&"🎮".repeat(200)).unwrap();
	let guard = metrics.track(resource(256));
	guard.set_label(&"é".repeat(200));
	metrics.frame(3, 6, 9, Some(f64::NAN));
	let snapshot = metrics.snapshot();
	assert!(snapshot.gpu_name.len() <= 256);
	assert!(snapshot.resources[0].label.len() <= 256);
	assert_eq!(snapshot.frame.gpu_time_ms, None);
	assert_eq!(snapshot.frame.frames, 1);
	drop(guard);
	metrics.close();
}
