use std::{
	backtrace::Backtrace,
	cell::Cell,
	collections::VecDeque,
	error::Error,
	fmt::{self, Write as _},
	sync::{Arc, Mutex, OnceLock, TryLockError},
	time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::{
	Config, Context, Correlation, ErrorDetails, Level, LogEntry, SCHEMA_VERSION, Snapshot,
	SnapshotRequest, StackTrace, storage,
};

mod installation;
pub use installation::{LoggingGuard, init, init_application, shutdown};

struct State {
	entries: VecDeque<Arc<LogEntry>>,
	next: u64,
}
pub(super) struct Logger {
	pub(super) config: Config,
	pub(super) session: String,
	state: Mutex<State>,
	pub(super) writer: Option<storage::Writer>,
	pub(super) health: Arc<storage::Health>,
}
pub(super) static LOGGER: OnceLock<Logger> = OnceLock::new();

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
pub(super) fn bounded(arguments: fmt::Arguments<'_>, limit: usize) -> String {
	let mut output = LimitedText {
		text: String::new(),
		limit,
	};
	let _ = output.write_fmt(arguments);
	output.text
}
impl Logger {
	pub(super) fn new(config: Config) -> Result<Self, Box<dyn Error + Send + Sync>> {
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
	pub(super) fn text(&self, arguments: fmt::Arguments<'_>, limit: usize) -> String {
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
	pub(super) fn snapshot(&self, request: SnapshotRequest) -> Snapshot {
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

#[cfg(test)]
#[path = "../tests/unit/tests_logger.rs"]
pub(super) mod tests;
