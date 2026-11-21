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

#[test]
fn metadata_sort_keeps_newest_devices_and_observes_replacement() {
	let root = crate::tests::test_directory("metrics-order");
	let session = root.join("hyphenated-game-1-100");
	fs::create_dir(&session).unwrap();
	for id in 1..=18 {
		let path = session.join(format!("vulkan-metrics-{id}.json"));
		fs::write(&path, serde_json::to_vec(&snapshot(id, true)).unwrap()).unwrap();
		fs::File::options()
			.write(true)
			.open(&path)
			.unwrap()
			.set_times(fs::FileTimes::new().set_modified(
				std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_600_000_000 + id),
			))
			.unwrap();
	}
	let page = read(&root, None);
	assert!(page.truncated);
	assert_eq!(page.devices.len(), MAX_DEVICES);
	assert!(
		!page
			.devices
			.iter()
			.any(|device| device.id.ends_with(":1") || device.id.ends_with(":2"))
	);
	let path = session.join("vulkan-metrics-1.json");
	fs::write(path, serde_json::to_vec(&snapshot(1, false)).unwrap()).unwrap();
	let page = read(&root, None);
	assert_eq!(page.selected.unwrap().device_id, 1);
	assert_eq!(page.devices.len(), MAX_DEVICES);
	fs::remove_dir_all(root).unwrap();
}

#[test]
fn actual_reads_share_a_budget_even_when_files_grow_or_contain_invalid_json() {
	let root = crate::tests::test_directory("metrics-read-budget");
	let path = root.join("vulkan-metrics-1.json");
	let original = serde_json::to_vec(&snapshot(1, false)).unwrap();
	fs::write(&path, &original).unwrap();
	let mut remaining = original.len() + 32;
	assert_eq!(load(&path, &mut remaining).unwrap().device_id, 1);
	assert_eq!(remaining, 32);

	// Simulate a publication replacing a file after directory metadata was
	// cached: the loader must obey the remaining allowance, not that old size.
	fs::write(&path, vec![b' '; 1024]).unwrap();
	assert!(load(&path, &mut remaining).is_err());
	assert_eq!(remaining, 0);
	assert!(load(&path, &mut remaining).is_err());
	assert_eq!(remaining, 0);

	fs::write(&path, "{broken").unwrap();
	remaining = 32;
	assert!(load(&path, &mut remaining).is_err());
	assert_eq!(remaining, 32 - "{broken".len());
	fs::remove_dir_all(root).unwrap();
}
