//! Shared process-wide diagnostics. Applications initialize; libraries only emit.
pub mod metrics;
mod storage;
pub use log::{LevelFilter, debug, error, info, trace, warn};
use serde::{Deserialize, Serialize};
use std::{
	backtrace::Backtrace,
	cell::Cell,
	collections::{BTreeMap, VecDeque},
	error::Error,
	fmt::{self, Write as _},
	path::PathBuf,
	sync::{Arc, Mutex, OnceLock, TryLockError},
	time::{Duration, SystemTime, UNIX_EPOCH},
};
pub use storage::StorageConfig;
pub use storage::read_jsonl;
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
#[derive(Debug, Clone)]
pub struct Config {
	pub application: String,
	pub level: LevelFilter,
	pub capacity: usize,
	pub storage: Option<StorageConfig>,
	pub console: bool,
	pub error_backtraces: bool,
	pub max_message_bytes: usize,
	pub max_trace_bytes: usize,
	/// Literal secrets removed before console output, memory retention, and persistence.
	pub redact: Vec<String>,
}
impl Config {
	pub fn new(application: impl Into<String>) -> Self {
		Self {
			application: application.into(),
			level: LevelFilter::Info,
			capacity: 2000,
			storage: None,
			console: true,
			error_backtraces: true,
			max_message_bytes: 8192,
			max_trace_bytes: 32768,
			redact: vec![],
		}
	}
	fn validate(&self) -> Result<(), Box<dyn Error + Send + Sync>> {
		if !safe_name(&self.application) {
			return Err("application must contain only ASCII letters, digits, '-' or '_'".into());
		}
		if self.capacity == 0 || self.max_message_bytes == 0 || self.max_trace_bytes == 0 {
			return Err("history capacity and text limits must be positive".into());
		}
		if let Some(storage) = &self.storage {
			storage.validate()?;
		}
		Ok(())
	}
}
fn safe_name(name: &str) -> bool {
	!name.is_empty()
		&& name.len() <= 64
		&& name
			.bytes()
			.all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
#[derive(Debug, Clone, Default)]
pub struct Context {
	pub correlation: Correlation,
	pub attributes: BTreeMap<String, String>,
}
struct State {
	entries: VecDeque<Arc<LogEntry>>,
	next: u64,
}
struct Logger {
	config: Config,
	session: String,
	state: Mutex<State>,
	writer: Option<storage::Writer>,
	health: Arc<storage::Health>,
}
static LOGGER: OnceLock<Logger> = OnceLock::new();
static INIT: Mutex<()> = Mutex::new(());
thread_local! { static EMITTING: Cell<bool> = const { Cell::new(false) }; }
struct EmissionGuard;
impl EmissionGuard {
	fn enter() -> Option<Self> {
		EMITTING.with(|active| {
			if active.replace(true) {
				None
			} else {
				Some(Self)
			}
		})
	}
}
impl Drop for EmissionGuard {
	fn drop(&mut self) {
		EMITTING.with(|active| active.set(false));
	}
}
/// Keep alive for the application lifetime. Flushes on scope exit and unwinding.
#[must_use = "keep the logging guard alive until application shutdown"]
pub struct LoggingGuard;
impl Drop for LoggingGuard {
	fn drop(&mut self) {
		shutdown(Duration::from_secs(2));
	}
}
/// Before initialization logging is a no-op. Competing global loggers are errors.
pub fn init(config: Config) -> Result<LoggingGuard, Box<dyn Error + Send + Sync>> {
	let _lock = INIT.lock().unwrap_or_else(|e| e.into_inner());
	if LOGGER.get().is_some() {
		return Err("engine logger already initialized".into());
	}
	let logger = Logger::new(config)?;
	LOGGER
		.set(logger)
		.map_err(|_| "engine logger already initialized")?;
	if let Err(error) = log::set_logger(LOGGER.get().unwrap()) {
		shutdown(Duration::from_secs(2));
		return Err(error.into());
	}
	log::set_max_level(LOGGER.get().unwrap().config.level);
	let previous = std::panic::take_hook();
	std::panic::set_hook(Box::new(move |panic| {
		if let Some(logger) = LOGGER.get() {
			if let Some(_guard) = EmissionGuard::enter() {
				let message = panic
					.payload()
					.downcast_ref::<&str>()
					.copied()
					.or_else(|| panic.payload().downcast_ref::<String>().map(String::as_str))
					.unwrap_or("non-string panic payload");
				let location = panic.location();
				logger.emit(Event {
					level: Level::Error,
					component: "panic",
					message: format_args!("{message}"),
					file: location.map(|p| p.file()),
					line: location.map(|p| p.line()),
					module: None,
					error: Some(ErrorDetails {
						category: "panic".into(),
						causes: vec![],
					}),
					context: Context::default(),
					panic: true,
				});
				// Never wait on disk from the hook. Writer flushes queued records best effort.
			} else {
				logger.health.drop_memory();
			}
		}
		previous(panic);
	}));
	Ok(LoggingGuard)
}
/// Defaults to INFO; ENGINE_LOG_MODE=debug enables DEBUG in any build profile.
/// ENGINE_LOG_DIR overrides the storage root. Console control is independent.
pub fn init_application(application: &str) -> Result<LoggingGuard, Box<dyn Error + Send + Sync>> {
	let mut config = Config::new(application);
	config.level = match std::env::var("ENGINE_LOG_MODE") {
		Err(std::env::VarError::NotPresent) => LevelFilter::Info,
		Ok(mode) if mode == "regular" => LevelFilter::Info,
		Ok(mode) if mode == "debug" => LevelFilter::Debug,
		_ => return Err("ENGINE_LOG_MODE must be 'regular' or 'debug'".into()),
	};
	config.console = !std::env::var("ENGINE_LOG_CONSOLE").is_ok_and(|v| v == "false" || v == "0");
	config.storage = Some(StorageConfig::new(log_directory()));
	init(config)
}
/// The shared storage root used by client emitters and development collectors.
pub fn log_directory() -> PathBuf {
	std::env::var_os("ENGINE_LOG_DIR")
		.map(PathBuf::from)
		.unwrap_or_else(|| {
			if let Some(root) =
				std::env::var_os("LOCALAPPDATA").or_else(|| std::env::var_os("XDG_STATE_HOME"))
			{
				PathBuf::from(root).join("GameEngine/logs")
			} else if let Some(home) = std::env::var_os("HOME") {
				PathBuf::from(home).join(".local/state/GameEngine/logs")
			} else {
				std::env::temp_dir().join("GameEngine/logs")
			}
		})
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
struct Event<'a> {
	level: Level,
	component: &'a str,
	message: fmt::Arguments<'a>,
	file: Option<&'a str>,
	line: Option<u32>,
	module: Option<&'a str>,
	error: Option<ErrorDetails>,
	context: Context,
	panic: bool,
}
/// Limit formatting allocation even for custom Display implementations emitting large text.
struct LimitedText {
	text: String,
	limit: usize,
}
impl fmt::Write for LimitedText {
	fn write_str(&mut self, value: &str) -> fmt::Result {
		let mut end = self.limit.saturating_sub(self.text.len()).min(value.len());
		while !value.is_char_boundary(end) {
			end -= 1;
		}
		self.text.push_str(&value[..end]);
		Ok(())
	}
}
fn bounded(arguments: fmt::Arguments<'_>, limit: usize) -> String {
	let mut output = LimitedText {
		text: String::new(),
		limit,
	};
	let _ = output.write_fmt(arguments);
	output.text
}
impl Logger {
	fn new(config: Config) -> Result<Self, Box<dyn Error + Send + Sync>> {
		config.validate()?;
		let session = format!(
			"{}-{}-{}",
			config.application,
			std::process::id(),
			SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
		);
		let health = Arc::new(storage::Health::default());
		let writer = config
			.storage
			.as_ref()
			.map(|settings| {
				storage::Writer::start(settings.clone(), session.clone(), health.clone())
			})
			.transpose()?;
		Ok(Self {
			config,
			session,
			state: Mutex::new(State {
				entries: VecDeque::new(),
				next: 1,
			}),
			writer,
			health,
		})
	}
	fn text(&self, arguments: fmt::Arguments<'_>, limit: usize) -> String {
		if self.config.redact.is_empty() {
			return bounded(arguments, limit);
		}
		// Look ahead far enough to redact a secret crossing the truncation boundary.
		let lookahead = self
			.config
			.redact
			.iter()
			.map(String::len)
			.max()
			.unwrap_or(0);
		let text = bounded(arguments, limit.saturating_add(lookahead));
		let mut output = LimitedText {
			text: String::new(),
			limit,
		};
		let mut position = 0;
		while position < text.len() && output.text.len() < limit {
			let remaining = &text[position..];
			if let Some(secret) = self
				.config
				.redact
				.iter()
				.filter(|s| !s.is_empty() && remaining.starts_with(s.as_str()))
				.max_by_key(|s| s.len())
			{
				let _ = output.write_str("[REDACTED]");
				position += secret.len();
			} else {
				let character = remaining.chars().next().unwrap();
				let _ = output.write_char(character);
				position += character.len_utf8();
			}
		}
		output.text
	}
	fn emit(&self, event: Event<'_>) {
		if !event.panic && event.level.facade() > self.config.level {
			return;
		}
		let thread = std::thread::current();
		let limit = self.config.max_message_bytes;
		let clean = |s: &str| self.text(format_args!("{s}"), limit);
		let entry = LogEntry {
			schema_version: SCHEMA_VERSION,
			session_id: self.session.clone(),
			sequence: 0,
			timestamp_ms: SystemTime::now()
				.duration_since(UNIX_EPOCH)
				.unwrap_or_default()
				.as_millis() as u64,
			application: self.config.application.clone(),
			component: clean(event.component),
			process_id: std::process::id(),
			thread: clean(&format!(
				"{}:{:?}",
				thread.name().unwrap_or("unnamed"),
				thread.id()
			)),
			level: event.level,
			message: self.text(event.message, limit),
			file: event.file.map(clean),
			line: event.line,
			module: event.module.map(clean),
			error: event.error.map(|error| ErrorDetails {
				category: clean(&error.category),
				causes: error.causes.iter().take(32).map(|s| clean(s)).collect(),
			}),
			stack_trace: (self.config.error_backtraces && event.level == Level::Error).then(|| {
				StackTrace {
					capture_kind: if event.panic {
						"panic_hook"
					} else {
						"reporting_boundary"
					}
					.into(),
					text: self.text(
						format_args!("{}", Backtrace::force_capture()),
						self.config.max_trace_bytes,
					),
				}
			}),
			correlation: Correlation {
				project_id: event.context.correlation.project_id.as_deref().map(clean),
				operation_id: event.context.correlation.operation_id.as_deref().map(clean),
				build_id: event.context.correlation.build_id.as_deref().map(clean),
				test_run_id: event.context.correlation.test_run_id.as_deref().map(clean),
			},
			attributes: event
				.context
				.attributes
				.iter()
				.take(32)
				.map(|(k, v)| (clean(k), clean(v)))
				.collect(),
		};
		// Panic hooks cannot wait on a lock held by the panicking thread or another thread.
		let mut state = if event.panic {
			match self.state.try_lock() {
				Ok(state) => state,
				Err(TryLockError::Poisoned(e)) => e.into_inner(),
				Err(TryLockError::WouldBlock) => {
					self.health.drop_memory();
					return;
				}
			}
		} else {
			self.state.lock().unwrap_or_else(|e| e.into_inner())
		};
		let mut entry = entry;
		entry.sequence = state.next;
		state.next += 1;
		let entry = Arc::new(entry);
		if let Some(writer) = &self.writer {
			writer.enqueue(&entry);
		}
		if state.entries.len() == self.config.capacity {
			state.entries.pop_front();
		}
		state.entries.push_back(Arc::clone(&entry));
		drop(state);
		if self.config.console && !event.panic {
			use std::io::Write;
			let _ = writeln!(
				std::io::stderr().lock(),
				"[{:?}] [{}] {} ({}:{})",
				entry.level,
				entry.component,
				entry.message,
				entry.file.as_deref().unwrap_or("?"),
				entry.line.unwrap_or(0)
			);
		}
	}
	fn snapshot(&self, request: SnapshotRequest) -> Snapshot {
		let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
		let reset = request
			.session_id
			.as_ref()
			.is_some_and(|id| *id != self.session);
		let after = if reset { 0 } else { request.after_sequence };
		let start = state
			.entries
			.partition_point(|entry| entry.sequence <= after);
		let retained: Vec<_> = state
			.entries
			.iter()
			.skip(start)
			.take(request.limit.min(2000))
			.cloned()
			.collect();
		let oldest = state.entries.front().map_or(0, |e| e.sequence);
		let latest = state.next - 1;
		drop(state);
		// Retained records survive eviction while DTO strings are copied outside
		// the producer lock. Cursors still describe the same captured history.
		let next_cursor = retained.last().map_or(after, |e| e.sequence);
		let entries = retained
			.iter()
			.map(|entry| entry.as_ref().clone())
			.collect();
		Snapshot {
			session_id: Some(self.session.clone()),
			oldest_sequence: oldest,
			latest_sequence: latest,
			history_gap: oldest > after.saturating_add(1),
			session_reset: reset,
			has_more: latest > next_cursor,
			next_cursor,
			entries,
			health: self.health.snapshot(),
		}
	}
}
impl log::Log for Logger {
	fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
		metadata.level() <= self.config.level
	}
	fn log(&self, record: &log::Record<'_>) {
		if !self.enabled(record.metadata()) {
			return;
		}
		let Some(_guard) = EmissionGuard::enter() else {
			self.health.drop_memory();
			return;
		};
		let level = match record.level() {
			log::Level::Error => Level::Error,
			log::Level::Warn => Level::Warning,
			log::Level::Info => Level::Info,
			log::Level::Debug => Level::Debug,
			log::Level::Trace => Level::Trace,
		};
		self.emit(Event {
			level,
			component: record.target(),
			message: *record.args(),
			file: record.file(),
			line: record.line(),
			module: record.module_path(),
			error: None,
			context: Context::default(),
			panic: false,
		});
	}
	fn flush(&self) {
		if let Some(writer) = &self.writer {
			writer.flush(Duration::from_secs(2));
		}
	}
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
pub fn snapshot(request: SnapshotRequest) -> Snapshot {
	LOGGER
		.get()
		.map_or_else(Snapshot::default, |logger| logger.snapshot(request))
}
/// Structured context with the caller's source location. Formatting is bounded after filtering.
#[track_caller]
pub fn event(level: Level, component: &str, message: fmt::Arguments<'_>, context: Context) {
	let Some(logger) = LOGGER.get() else {
		return;
	};
	if level.facade() > logger.config.level {
		return;
	}
	let Some(_guard) = EmissionGuard::enter() else {
		logger.health.drop_memory();
		return;
	};
	let location = std::panic::Location::caller();
	logger.emit(Event {
		level,
		component,
		message,
		file: Some(location.file()),
		line: Some(location.line()),
		module: None,
		error: None,
		context,
		panic: false,
	});
}
pub trait ResultLogExt<T, E> {
	/// Report once at a handling boundary, not at every propagation step.
	#[track_caller]
	fn log_error(self, context: &str) -> Self;
	#[track_caller]
	fn log_error_for(self, component: &str, context: &str) -> Self;
}
impl<T, E: Error> ResultLogExt<T, E> for Result<T, E> {
	#[track_caller]
	fn log_error(self, context: &str) -> Self {
		self.log_error_for("result", context)
	}
	#[track_caller]
	fn log_error_for(self, component: &str, context: &str) -> Self {
		if let (Err(error), Some(logger)) = (&self, LOGGER.get())
			&& Level::Error.facade() <= logger.config.level
			&& let Some(_guard) = EmissionGuard::enter()
		{
			let mut causes = vec![];
			let mut source = error.source();
			for _ in 0..32 {
				let Some(cause) = source else {
					break;
				};
				causes.push(bounded(
					format_args!("{cause}"),
					logger.config.max_message_bytes,
				));
				source = cause.source();
			}
			let location = std::panic::Location::caller();
			logger.emit(Event {
				level: Level::Error,
				component,
				message: format_args!("{context}: {error}"),
				file: Some(location.file()),
				line: Some(location.line()),
				module: None,
				error: Some(ErrorDetails {
					category: std::any::type_name::<E>().into(),
					causes,
				}),
				context: Context::default(),
				panic: false,
			});
		}
		self
	}
}
/// Stop accepting disk records and drain the queue, waiting no longer than timeout.
/// A timed-out writer may finish later; forced exits cannot guarantee persistence.
pub fn shutdown(timeout: Duration) -> bool {
	LOGGER
		.get()
		.and_then(|l| l.writer.as_ref())
		.is_none_or(|writer| writer.shutdown(timeout))
}
#[cfg(test)]
#[path = "tests_logger.rs"]
mod tests;
