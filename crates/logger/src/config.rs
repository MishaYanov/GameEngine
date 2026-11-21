use std::{error::Error, path::PathBuf};

use crate::{LevelFilter, StorageConfig};

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
	pub(super) fn validate(&self) -> Result<(), Box<dyn Error + Send + Sync>> {
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

/// File-backed defaults for applications that need to customize configuration before init.
/// ENGINE_LOG_MODE controls the level and ENGINE_LOG_DIR overrides the storage root.
pub fn application_config(application: &str) -> Result<Config, Box<dyn Error + Send + Sync>> {
	let mut config = Config::new(application);
	config.level = match std::env::var("ENGINE_LOG_MODE") {
		Err(std::env::VarError::NotPresent) => LevelFilter::Info,
		Ok(mode) if mode == "regular" => LevelFilter::Info,
		Ok(mode) if mode == "debug" => LevelFilter::Debug,
		_ => return Err("ENGINE_LOG_MODE must be 'regular' or 'debug'".into()),
	};
	config.console = !std::env::var("ENGINE_LOG_CONSOLE").is_ok_and(|v| v == "false" || v == "0");
	config.storage = Some(StorageConfig::new(log_directory()));
	Ok(config)
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
