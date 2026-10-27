use super::*;
use collector::{Collector, LogFilter};
use engine_logger::{Level, LogEntry};
use health::{HealthState, Registry};
use processes::{ProcessState, Processes};
use std::{
	io::Write,
	process::Command,
	time::{Duration, Instant},
};
struct Fixture(PathBuf);
pub(crate) fn test_directory(label: &str) -> PathBuf {
	static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
	// Windows sandbox Temp may disallow atomic file replacement. Keep isolated
	// persistence/child-Cargo fixtures in the writable checkout instead.
	let root = Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("../../../target")
		.canonicalize()
		.unwrap();
	let path = root.join(format!(
		"qa-admin-{label}-{}-{}",
		std::process::id(),
		NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
	));
	fs::create_dir(&path).unwrap();
	path
}
impl Fixture {
	fn new() -> Self {
		Self(test_directory("service"))
	}
	fn logs(&self) -> PathBuf {
		let path = self.0.join("logs");
		fs::create_dir_all(&path).unwrap();
		path
	}
	fn data(&self) -> PathBuf {
		self.0.join("data")
	}
	fn write(&self, entry: &LogEntry) -> PathBuf {
		let path = self.logs().join(&entry.session_id);
		fs::create_dir_all(&path).unwrap();
		let path = path.join("000000.jsonl");
		let mut file = fs::OpenOptions::new()
			.create(true)
			.append(true)
			.open(&path)
			.unwrap();
		serde_json::to_writer(&mut file, entry).unwrap();
		file.write_all(b"\n").unwrap();
		path
	}
}
impl Drop for Fixture {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.0);
	}
}
pub(super) fn record(session: &str, sequence: u64) -> LogEntry {
	LogEntry {
		schema_version: 1,
		session_id: session.into(),
		sequence,
		timestamp_ms: 100 + sequence,
		application: session.split('-').next().unwrap().into(),
		component: "game".into(),
		process_id: 123,
		thread: "main".into(),
		level: Level::Info,
		message: format!("record {sequence}"),
		file: None,
		line: None,
		module: None,
		error: None,
		stack_trace: None,
		correlation: Default::default(),
		attributes: Default::default(),
	}
}
#[test]
fn incremental_collection_deduplication_and_restart() {
	let fixture = Fixture::new();
	fixture.write(&record("game-123-1", 1));
	fixture.write(&record("compiler-124-2", 1));
	let mut collector = Collector::new(fixture.logs(), &fixture.data());
	assert_eq!(collector.scan().len(), 2);
	assert!(collector.scan().is_empty());
	fixture.write(&record("game-123-1", 2));
	assert_eq!(collector.scan().len(), 1);
	collector.persist();
	let first = collector.page(0, 1, &LogFilter::default());
	assert!(first.has_more);
	assert_eq!(first.next_cursor, 1);
	let mut restored = Collector::new(fixture.logs(), &fixture.data());
	assert!(restored.scan().is_empty());
	assert_eq!(
		restored.page(0, 500, &LogFilter::default()).entries.len(),
		3
	);
	assert_eq!(
		first.collector_id,
		restored.page(0, 1, &LogFilter::default()).collector_id
	);
	fixture.write(&record("game-123-1", 3));
	assert_eq!(restored.scan().len(), 1);
}
#[test]
fn partial_lines_malformed_lines_and_rotated_segments() {
	let fixture = Fixture::new();
	let path = fixture.write(&record("game-123-1", 1));
	let second = serde_json::to_vec(&record("game-123-1", 2)).unwrap();
	let split = second.len() / 2;
	let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
	file.write_all(&second[..split]).unwrap();
	let mut collector = Collector::new(fixture.logs(), &fixture.data());
	assert_eq!(collector.scan().len(), 1);
	assert!(collector.scan().is_empty());
	file.write_all(&second[split..]).unwrap();
	file.write_all(b"\ninvalid\n").unwrap();
	assert_eq!(collector.scan().len(), 1);
	assert_eq!(collector.health.malformed_lines, 1);
	drop(file);
	let rotated = path.with_file_name("000001.jsonl");
	fs::write(
		&rotated,
		format!(
			"{}\n",
			serde_json::to_string(&record("game-123-1", 4)).unwrap()
		),
	)
	.unwrap();
	fs::remove_file(path).unwrap();
	assert_eq!(collector.scan().len(), 1);
	assert_eq!(collector.health.history_gaps, 1);
}
#[test]
fn missing_directory_and_oversized_lines_are_visible() {
	let fixture = Fixture::new();
	let mut collector = Collector::new(fixture.0.join("missing"), &fixture.data());
	collector.scan();
	assert!(collector.health.storage_error.is_some());
	collector.directory = fixture.logs();
	let path = fixture.write(&record("game-123-1", 1));
	let mut file = fs::OpenOptions::new().append(true).open(path).unwrap();
	file.write_all(&vec![b'x'; 512 * 1024 + 10]).unwrap();
	file.write_all(b"\n").unwrap();
	collector.scan();
	assert!(collector.health.storage_error.is_none());
	assert_eq!(collector.health.oversized_lines, 1);
}
#[test]
fn filtered_pagination_retention_and_export() {
	let fixture = Fixture::new();
	let mut collector = Collector::new(fixture.logs(), &fixture.data());
	for i in 1..=5005 {
		let mut entry = record("game-123-1", i);
		entry.level = if i % 2 == 0 {
			Level::Error
		} else {
			Level::Info
		};
		collector.ingest(entry);
	}
	let filter = LogFilter {
		severity: Some(Level::Error),
		search: Some("record".into()),
		..Default::default()
	};
	let page = collector.page(0, 1, &filter);
	assert_eq!(page.matching_count, 2500);
	assert!(page.history_gap);
	assert!(page.has_more);
	assert_eq!(page.entries[0].entry.level, Level::Error);
	let next = collector.page(page.next_cursor, 1, &filter);
	assert!(next.entries[0].cursor > page.next_cursor);
	let (path, count) = collector.export(&fixture.data(), &filter).unwrap();
	assert!(count > 0);
	let exported = engine_logger::read_jsonl(path, 512 * 1024).unwrap();
	assert_eq!(exported.len(), count);
	assert!(exported.iter().all(|e| e.level == Level::Error));
	assert_eq!(collector.page(0, usize::MAX, &filter).entries.len(), 500);
}
#[test]
fn pagination_preserves_filtered_counts_cursor_boundaries_and_unicode_search() {
	let fixture = Fixture::new();
	let mut collector = Collector::new(fixture.logs(), &fixture.data());
	assert_eq!(collector.page(0, 0, &LogFilter::default()).next_cursor, 0);
	for sequence in 1..=7 {
		let mut entry = record("game-123-1", sequence);
		if sequence % 2 == 0 {
			entry.message = "Scene ÄbC loaded".into();
			entry.level = Level::Error;
		}
		entry.correlation.operation_id = Some("load".into());
		collector.ingest(entry);
	}
	let filter = LogFilter {
		search: Some("ÄBC".into()),
		severity: Some(Level::Error),
		application: Some("game".into()),
		component: Some("game".into()),
		session_id: Some("game-123-1".into()),
		operation_id: Some("load".into()),
		from_ms: Some(102),
		to_ms: Some(106),
	};
	let first = collector.page(0, 0, &filter);
	assert_eq!(first.matching_count, 3);
	assert_eq!(first.entries.len(), 1);
	assert_eq!(first.next_cursor, 2);
	assert!(first.has_more);
	let tail = collector.page(first.next_cursor, 2, &filter);
	assert_eq!(tail.matching_count, 3);
	assert_eq!(
		tail.entries.iter().map(|e| e.cursor).collect::<Vec<_>>(),
		[4, 6]
	);
	assert!(!tail.has_more);
	assert_eq!(tail.next_cursor, 7);
	assert_eq!(tail.latest_cursor, 7);
	let end = collector.page(tail.next_cursor, 2, &filter);
	assert!(end.entries.is_empty());
	assert!(!end.has_more);
	assert_eq!(end.matching_count, 3);
	assert_eq!(end.next_cursor, 7);
	let no_matches = collector.page(
		0,
		2,
		&LogFilter {
			search: Some("not found".into()),
			..Default::default()
		},
	);
	assert!(no_matches.entries.is_empty());
	assert_eq!(no_matches.matching_count, 0);
	assert_eq!(no_matches.next_cursor, 7);
	let (path, count) = collector.export(&fixture.data(), &filter).unwrap();
	assert_eq!(count, 3);
	assert!(
		engine_logger::read_jsonl(path, 512 * 1024)
			.unwrap()
			.iter()
			.all(|e| filter.matches(e))
	);
}
#[test]
fn registry_skips_unchanged_writes_and_retries_failed_changes() {
	let fixture = Fixture::new();
	let path = fixture.data().join("components.json");
	let mut registry = Registry::new();
	let mut entry = record("game-123-1", 1);
	entry.attributes = [
		("event".into(), "component_health".into()),
		("health".into(), "HEALTHY".into()),
	]
	.into();
	registry.accept(&entry);
	registry.persist(&path);
	let original = fs::read(&path).unwrap();
	// Block any new write without relying on filesystem timestamp resolution.
	let temporary = path.with_extension("tmp");
	fs::create_dir(&temporary).unwrap();
	registry.accept(&entry);
	registry.persist(&path);
	assert!(registry.persistence_error().is_none());
	assert_eq!(fs::read(&path).unwrap(), original);
	registry.stopped(&entry.session_id, 200, false);
	registry.persist(&path);
	assert!(registry.persistence_error().is_some());
	assert_eq!(fs::read(&path).unwrap(), original);
	fs::remove_dir(&temporary).unwrap();
	registry.persist(&path);
	assert!(registry.persistence_error().is_none());
	let restored = fs::read(&path).unwrap();
	assert_ne!(restored, original);
	fs::create_dir(&temporary).unwrap();
	registry.stopped(&entry.session_id, 200, false);
	registry.persist(&path);
	assert!(registry.persistence_error().is_none());
	let mut registry: Registry = serde_json::from_slice(&restored).unwrap();
	registry.persist(&path);
	assert!(registry.persistence_error().is_none());
	assert_eq!(fs::read(&path).unwrap(), restored);
}
#[test]
fn failed_log_checkpoint_retries_and_retains_restart_deduplication() {
	let fixture = Fixture::new();
	let path = fixture.data().join("logs.json");
	fs::create_dir_all(&path).unwrap();
	let mut collector = Collector::new(fixture.logs(), &fixture.data());
	collector.ingest(record("game-123-1", 1));
	collector.persist();
	assert!(collector.health.checkpoint_error.is_some());
	fs::remove_dir(&path).unwrap();
	collector.persist();
	assert!(collector.health.checkpoint_error.is_none());
	let mut restored = Collector::new(fixture.logs(), &fixture.data());
	assert!(!restored.ingest(record("game-123-1", 1)));
	assert!(restored.ingest(record("game-123-1", 2)));
	assert_eq!(restored.page(0, 2, &LogFilter::default()).entries.len(), 2);
}
#[test]
fn registry_checkpoint_recovery_preserves_independent_collector_failure() {
	let fixture = Fixture::new();
	let mut service = AdminService::new(fixture.0.clone(), fixture.logs(), fixture.data());
	let temporary = fixture.data().join("components.tmp");
	fs::create_dir(&temporary).unwrap();
	let mut entry = record("game-123-1", 1);
	entry.attributes = [("event".into(), "heartbeat".into())].into();
	service.registry.accept(&entry);
	service.tick();
	let registry_error = service.registry.persistence_error().unwrap().to_owned();
	assert_eq!(
		service.snapshot().collector.checkpoint_error,
		Some(registry_error)
	);
	service.collector.health.checkpoint_error = Some("Existing log checkpoint failure".into());
	fs::remove_dir(&temporary).unwrap();
	service.tick();
	assert!(service.registry.persistence_error().is_none());
	assert_eq!(
		service.snapshot().collector.checkpoint_error.as_deref(),
		Some("Existing log checkpoint failure")
	);
}
#[test]
fn health_uses_explicit_events_and_stale_heartbeats() {
	let mut registry = Registry::new();
	let mut entry = record("game-123-1", 1);
	entry.timestamp_ms = 100;
	entry.level = Level::Error;
	registry.accept(&entry);
	assert_eq!(registry.snapshot(100)[2].health, HealthState::Unknown);
	entry.attributes = [
		("event".into(), "component_health".into()),
		("health".into(), "HEALTHY".into()),
		("heartbeat_expected".into(), "true".into()),
	]
	.into();
	registry.accept(&entry);
	let game = |registry: &Registry, time| {
		registry
			.snapshot(time)
			.into_iter()
			.find(|c| c.id == "game")
			.unwrap()
	};
	assert_eq!(game(&registry, 100).health, HealthState::Healthy);
	assert!(game(&registry, 15101).stale);
	assert_eq!(game(&registry, 15101).health, HealthState::Degraded);
	entry.timestamp_ms = 16000;
	entry.attributes = [("event".into(), "heartbeat".into())].into();
	registry.accept(&entry);
	assert!(!game(&registry, 16001).stale);
	registry.stopped(&entry.session_id, 17000, true);
	assert_eq!(game(&registry, 17001).health, HealthState::Failed);
}
fn fixture_command(mode: &str) -> Command {
	#[cfg(windows)]
	{
		let mut command = Command::new("powershell.exe");
		command.args([
			"-NoProfile",
			"-NonInteractive",
			"-Command",
			match mode {
				"hang" => "Write-Output 'fixture started'; Start-Sleep -Seconds 30",
				"fail" => "Write-Output 'fixture failed'; exit 7",
				_ => "Write-Output 'fixture succeeded'; exit 0",
			},
		]);
		command
	}
	#[cfg(not(windows))]
	{
		let mut command = Command::new("sh");
		command.args([
			"-c",
			match mode {
				"hang" => "echo fixture; sleep 30",
				"fail" => "echo fixture; exit 7",
				_ => "echo fixture; exit 0",
			},
		]);
		command
	}
}
fn await_exit(processes: &mut Processes) {
	let started = Instant::now();
	while processes.history()[0].state == ProcessState::Running {
		processes.poll();
		assert!(started.elapsed() < Duration::from_secs(10));
		std::thread::sleep(Duration::from_millis(20));
	}
}
#[test]
fn managed_process_start_stop_restart_and_failure() {
	let fixture = Fixture::new();
	let mut processes = Processes::new(&fixture.data());
	let first = processes
		.start_command(
			"compiler",
			fixture_command("hang"),
			&fixture.0,
			&fixture.logs(),
		)
		.unwrap();
	assert!(first.pid > 0);
	assert!(
		processes
			.start_command(
				"compiler",
				fixture_command("hang"),
				&fixture.0,
				&fixture.logs()
			)
			.is_err()
	);
	processes.stop("compiler").unwrap();
	assert_eq!(processes.history()[0].state, ProcessState::Stopped);
	assert!(processes.history()[0].intentional_stop);
	let second = processes
		.start_command(
			"compiler",
			fixture_command("fail"),
			&fixture.0,
			&fixture.logs(),
		)
		.unwrap();
	assert_ne!(first.id, second.id);
	await_exit(&mut processes);
	assert_eq!(processes.history()[0].state, ProcessState::Failed);
	assert_eq!(processes.history()[0].exit_code, Some(7));
	processes
		.start_command(
			"compiler",
			fixture_command("success"),
			&fixture.0,
			&fixture.logs(),
		)
		.unwrap();
	await_exit(&mut processes);
	assert_eq!(processes.history()[0].state, ProcessState::Succeeded);
	assert!(processes.stop("unowned").is_err());
	let restored = Processes::new(&fixture.data());
	assert_eq!(restored.history().len(), 3);
}
#[test]
fn interrupted_history_is_not_adopted() {
	let fixture = Fixture::new();
	let mut owner = Processes::new(&fixture.data());
	owner
		.start_command(
			"compiler",
			fixture_command("hang"),
			&fixture.0,
			&fixture.logs(),
		)
		.unwrap();
	let mut restored = Processes::new(&fixture.data());
	assert_eq!(restored.history()[0].state, ProcessState::Interrupted);
	assert!(restored.stop("compiler").is_err());
	owner.stop("compiler").unwrap();
}
#[test]
fn source_navigation_rejects_files_outside_workspace() {
	let fixture = Fixture::new();
	let workspace = fixture.0.join("workspace");
	fs::create_dir(&workspace).unwrap();
	fs::write(workspace.join("Cargo.toml"), "[workspace]\n").unwrap();
	fs::write(workspace.join("source.rs"), "// fixture").unwrap();
	fs::write(fixture.0.join("outside.rs"), "// outside").unwrap();
	let mut service = AdminService::new(workspace.clone(), fixture.logs(), fixture.data());
	let mut entry = record("game-123-1", 1);
	entry.file = Some("source.rs".into());
	entry.line = Some(4);
	service.collector.ingest(entry.clone());
	assert_eq!(service.source(1).unwrap().1, 4);
	entry.sequence = 2;
	entry.file = Some("../outside.rs".into());
	service.collector.ingest(entry);
	assert!(service.source(2).is_err());
	assert!(service.configure("missing", "logs").is_err());
}
#[test]
fn process_exit_updates_session_health() {
	let fixture = Fixture::new();
	let mut service = AdminService::new(fixture.0.clone(), fixture.logs(), fixture.data());
	let run = service
		.processes
		.start_command(
			"compiler",
			fixture_command("hang"),
			&fixture.0,
			&fixture.logs(),
		)
		.unwrap();
	let mut entry = record("compiler-123-1", 1);
	entry.component = "compiler".into();
	entry.process_id = run.pid;
	entry.timestamp_ms = now_ms();
	entry.attributes = [
		("event".into(), "component_health".into()),
		("health".into(), "HEALTHY".into()),
	]
	.into();
	fixture.write(&entry);
	service.tick();
	assert_eq!(
		service
			.snapshot()
			.components
			.into_iter()
			.find(|c| c.id == "compiler")
			.unwrap()
			.health,
		HealthState::Healthy
	);
	service.processes.stop("compiler").unwrap();
	service.tick();
	assert_eq!(
		service
			.snapshot()
			.components
			.into_iter()
			.find(|c| c.id == "compiler")
			.unwrap()
			.health,
		HealthState::Stopped
	);
}
