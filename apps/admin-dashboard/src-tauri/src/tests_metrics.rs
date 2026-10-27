use super::*;
use engine_logger::metrics::{FrameMetrics, ResourceTotals};

fn snapshot(device: u64, closed: bool) -> MetricsSnapshot {
	MetricsSnapshot {
		schema_version: 1,
		device_id: device,
		application: "game".into(),
		session_id: "game-1-100".into(),
		process_id: 1,
		gpu_name: "test GPU".into(),
		started_ms: 100,
		updated_ms: 200 + device,
		closed,
		inventory_truncated: false,
		totals: ResourceTotals::default(),
		frame: FrameMetrics::default(),
		resources: vec![],
		recent_released: vec![],
	}
}

#[test]
fn reader_separates_devices_prefers_active_and_honors_selection() {
	let root = crate::tests::test_directory("metrics");
	let directory = root.join("game-1-100");
	fs::create_dir(&directory).unwrap();
	for (id, closed) in [(1, false), (2, true)] {
		fs::write(
			directory.join(format!("vulkan-metrics-{id}.json")),
			serde_json::to_vec(&snapshot(id, closed)).unwrap(),
		)
		.unwrap();
	}
	let page = read(&root, None);
	assert_eq!(page.devices.len(), 2);
	assert_eq!(page.selected.unwrap().device_id, 1);
	assert_eq!(
		read(&root, Some("game-1-100:2"))
			.selected
			.unwrap()
			.device_id,
		2
	);
	// The supplied selector is compared with known IDs, never used as a path.
	assert_eq!(
		read(&root, Some("../../other")).selected.unwrap().device_id,
		1
	);
	fs::remove_dir_all(root).unwrap();
}

#[test]
fn corrupt_or_unsupported_snapshots_are_visible_without_hiding_valid_devices() {
	let root = crate::tests::test_directory("metrics-invalid");
	let directory = root.join("game-1-100");
	fs::create_dir(&directory).unwrap();
	fs::write(
		directory.join("vulkan-metrics-1.json"),
		serde_json::to_vec(&snapshot(1, false)).unwrap(),
	)
	.unwrap();
	let mut invalid = snapshot(2, false);
	invalid.schema_version = 999;
	fs::write(
		directory.join("vulkan-metrics-2.json"),
		serde_json::to_vec(&invalid).unwrap(),
	)
	.unwrap();
	fs::write(directory.join("vulkan-metrics-3.json"), "{incomplete").unwrap();
	fs::write(
		directory.join("vulkan-metrics-4.tmp"),
		"ignored incomplete write",
	)
	.unwrap();
	let page = read(&root, None);
	assert_eq!(page.devices.len(), 1);
	assert!(page.storage_error.is_some());
	assert_eq!(page.selected.unwrap().device_id, 1);
	assert!(read(&root.join("missing"), None).devices.is_empty());
	fs::remove_dir_all(root).unwrap();
}
