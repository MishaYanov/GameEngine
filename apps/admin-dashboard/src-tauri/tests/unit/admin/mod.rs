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

mod health_registry;
mod log_collection;
mod managed_processes;
mod source_navigation;
