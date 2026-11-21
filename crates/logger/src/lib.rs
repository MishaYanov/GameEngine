//! Shared process-wide diagnostics. Applications initialize; libraries only emit.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

mod config;
pub mod metrics;
mod process;
mod storage;

pub use config::{Config, application_config, log_directory};
pub use log::{LevelFilter, debug, error, info, trace, warn};
use process::{LOGGER, bounded};
pub use process::{LoggingGuard, ResultLogExt, event, init, init_application, shutdown, snapshot};
pub use storage::{StorageConfig, read_jsonl};

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum Level {
	Error,
	Warning,
	Info,
	Debug,
	Trace,
}
impl Level {
	fn facade(self) -> log::Level {
		match self {
			Self::Error => log::Level::Error,
			Self::Warning => log::Level::Warn,
			Self::Info => log::Level::Info,
			Self::Debug => log::Level::Debug,
			Self::Trace => log::Level::Trace,
		}
	}
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Correlation {
	pub project_id: Option<String>,
	pub operation_id: Option<String>,
	pub build_id: Option<String>,
	pub test_run_id: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ErrorDetails {
	pub category: String,
	pub causes: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StackTrace {
	pub capture_kind: String,
	pub text: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LogEntry {
	pub schema_version: u32,
	pub session_id: String,
	pub sequence: u64,
	pub timestamp_ms: u64,
	pub application: String,
	pub component: String,
	pub process_id: u32,
	pub thread: String,
	pub level: Level,
	pub message: String,
	pub file: Option<String>,
	pub line: Option<u32>,
	pub module: Option<String>,
	pub error: Option<ErrorDetails>,
	pub stack_trace: Option<StackTrace>,
	pub correlation: Correlation,
	pub attributes: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default)]
pub struct Context {
	pub correlation: Correlation,
	pub attributes: BTreeMap<String, String>,
}

/// Explicit component health; an ERROR diagnostic alone does not change health.
pub fn component_health(component: &str, health: &str, heartbeat_expected: bool, message: &str) {
	if !matches!(
		health,
		"UNKNOWN" | "HEALTHY" | "DEGRADED" | "FAILED" | "STOPPED"
	) {
		return;
	}
	let context = Context {
		attributes: [
			("event".into(), "component_health".into()),
			("health".into(), health.into()),
			("heartbeat_expected".into(), heartbeat_expected.to_string()),
		]
		.into(),
		..Default::default()
	};
	event(Level::Info, component, format_args!("{message}"), context);
}
pub fn heartbeat(component: &str) {
	let context = Context {
		attributes: [("event".into(), "heartbeat".into())].into(),
		..Default::default()
	};
	event(
		Level::Info,
		component,
		format_args!("Application heartbeat"),
		context,
	);
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct LoggerHealth {
	pub persistence_error: Option<String>,
	pub dropped_persistence: u64,
	pub dropped_memory: u64,
	pub writer_stopped: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotRequest {
	pub session_id: Option<String>,
	pub after_sequence: u64,
	pub limit: usize,
}
impl Default for SnapshotRequest {
	fn default() -> Self {
		Self {
			session_id: None,
			after_sequence: 0,
			limit: 200,
		}
	}
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Snapshot {
	pub session_id: Option<String>,
	pub entries: Vec<LogEntry>,
	pub oldest_sequence: u64,
	pub latest_sequence: u64,
	pub next_cursor: u64,
	pub history_gap: bool,
	pub session_reset: bool,
	pub has_more: bool,
	pub health: LoggerHealth,
}
