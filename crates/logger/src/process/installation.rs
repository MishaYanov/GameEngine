use std::{error::Error, sync::Mutex, time::Duration};

use super::{EmissionGuard, Event, LOGGER, Logger};
use crate::{Config, Context, ErrorDetails, Level, application_config};

static INIT: Mutex<()> = Mutex::new(());

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

pub fn init_application(application: &str) -> Result<LoggingGuard, Box<dyn Error + Send + Sync>> {
	init(application_config(application)?)
}

/// Stop accepting disk records and drain the queue, waiting no longer than timeout.
/// A timed-out writer may finish later; forced exits cannot guarantee persistence.
pub fn shutdown(timeout: Duration) -> bool {
	LOGGER
		.get()
		.and_then(|l| l.writer.as_ref())
		.is_none_or(|writer| writer.shutdown(timeout))
}
