pub mod board;
pub mod builder;
pub mod collector;
#[cfg(feature = "desktop")]
pub mod desktop;
pub mod health;
#[cfg(windows)]
mod job;
pub mod jobs;
pub mod metrics;
pub mod processes;
mod service;
mod source_location;
#[cfg(any(feature = "desktop", test))]
mod workspace_config;

pub use service::{AdminService, DashboardSnapshot, start_worker};

use serde::Serialize;
use std::{
	fs,
	io::{self, Write},
	path::{Path, PathBuf},
	time::{SystemTime, UNIX_EPOCH},
};

pub fn comparable_path(path: &Path) -> PathBuf {
	#[cfg(windows)]
	{
		let text = path.to_string_lossy();
		if let Some(unc) = text.strip_prefix("\\\\?\\UNC\\") {
			return PathBuf::from(format!("\\\\{unc}"));
		}
		if let Some(normal) = text.strip_prefix("\\\\?\\") {
			return PathBuf::from(normal);
		}
	}
	path.to_owned()
}
pub fn relative_to(path: &Path, root: &Path) -> Option<PathBuf> {
	comparable_path(path)
		.strip_prefix(comparable_path(root))
		.ok()
		.map(Path::to_owned)
}
pub fn now_ms() -> u64 {
	SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.unwrap_or_default()
		.as_millis() as u64
}
pub fn save_json(path: &Path, value: &impl Serialize) -> io::Result<()> {
	let parent = path
		.parent()
		.ok_or_else(|| io::Error::other("Missing data directory"))?;
	fs::create_dir_all(parent)?;
	let temporary = path.with_extension("tmp");
	let mut file = io::BufWriter::new(fs::File::create(&temporary)?);
	serde_json::to_writer(&mut file, value)?;
	file.flush()?;
	drop(file);
	fs::rename(temporary, path)
}

#[cfg(test)]
#[path = "../tests/unit/admin/mod.rs"]
mod tests;
