use engine_logger::{
	Config, StorageConfig,
	metrics::{MetricsSnapshot, ResourceMetrics, ResourceRecord},
};
use std::{
	fs,
	path::Path,
	time::{Duration, SystemTime},
};

#[test]
fn regular_logging_persists_complete_metrics_and_flushes_final_releases() {
	// Producers can register before the application configures the logger.
	let early = ResourceMetrics::new("secret-asset GPU").unwrap();
	let early_record = || ResourceRecord {
		id: 0,
		kind: "secret-asset".into(),
		handle: "secret-asset".into(),
		label: "secret-asset".into(),
		details: "secret-asset".into(),
		payload_bytes: 0,
		allocation_bytes: 0,
		memory_type: None,
		memory_heap: None,
		device_local: false,
		host_visible: false,
		created_ms: 0,
		released_ms: None,
	};
	let early_guard = early.track(early_record());
	drop(early.track(early_record()));
	let root = Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("../../target")
		.join(format!(
			"qa-resource-metrics-{}-{}",
			std::process::id(),
			SystemTime::now()
				.duration_since(SystemTime::UNIX_EPOCH)
				.unwrap()
				.as_nanos()
		));
	let mut config = Config::new("metrics-test");
	config.console = false;
	config.storage = Some(StorageConfig::new(&root));
	config.redact = vec!["secret-asset".into()];
	let logging = engine_logger::init(config).unwrap();
	let early_snapshot = early.snapshot();
	assert!(
		!serde_json::to_string(&early_snapshot)
			.unwrap()
			.contains("secret-asset")
	);
	assert_eq!(early_snapshot.totals.by_kind["[REDACTED]"], 1);
	assert_eq!(early_snapshot.resources[0].label, "[REDACTED]");
	assert_eq!(early_snapshot.recent_released[0].label, "[REDACTED]");
	let early_path = root
		.join(&early_snapshot.session_id)
		.join(format!("vulkan-metrics-{}.json", early_snapshot.device_id));
	let metrics = ResourceMetrics::new("test GPU").unwrap();
	let guard = metrics.track(ResourceRecord {
		id: 0,
		kind: "buffer".into(),
		handle: "0x123".into(),
		label: "secret-asset".into(),
		details: "vertices".into(),
		payload_bytes: 10,
		allocation_bytes: 256,
		memory_type: Some(0),
		memory_heap: Some(0),
		device_local: true,
		host_visible: false,
		created_ms: 0,
		released_ms: None,
	});
	let id = metrics.snapshot().device_id;
	let session = metrics.snapshot().session_id;
	let path = root.join(session).join(format!("vulkan-metrics-{id}.json"));
	let deadline = std::time::Instant::now() + Duration::from_secs(5);
	while !path.is_file() && std::time::Instant::now() < deadline {
		std::thread::sleep(Duration::from_millis(20));
	}
	let live: MetricsSnapshot = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
	assert_eq!(live.totals.allocation_bytes, 256);
	assert_eq!(live.resources[0].label, "[REDACTED]");
	assert_eq!(live.application, "metrics-test");
	drop(guard);
	metrics.close();
	drop(early_guard);
	early.close();
	drop(logging);
	assert!(
		!fs::read_to_string(early_path)
			.unwrap()
			.contains("secret-asset")
	);
	let final_snapshot: MetricsSnapshot =
		serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
	assert!(final_snapshot.closed);
	assert_eq!(final_snapshot.totals.allocation_bytes, 0);
	assert_eq!(final_snapshot.totals.released_resources, 1);
	assert_eq!(final_snapshot.totals.peak_allocation_bytes, 256);
	assert!(
		engine_logger::snapshot(Default::default())
			.health
			.persistence_error
			.is_none()
	);
	fs::remove_dir_all(root).unwrap();
}
