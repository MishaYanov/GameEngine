//! Read bounded, complete resource snapshots. This app never initializes Vulkan.
use engine_logger::metrics::{MAX_RELEASED, MAX_RESOURCES, MAX_SNAPSHOT_BYTES, MetricsSnapshot};
use serde::Serialize;
use std::{
	fs,
	io::{self, Read},
	path::{Path, PathBuf},
	time::SystemTime,
};

const MAX_DEVICES: usize = 16;
const READ_BUDGET: usize = 16 * 1024 * 1024;

struct SnapshotFile {
	path: PathBuf,
	modified: Option<SystemTime>,
	bytes: usize,
}

#[derive(Serialize)]
pub struct DeviceSummary {
	pub id: String,
	pub application: String,
	pub session_id: String,
	pub process_id: u32,
	pub gpu_name: String,
	pub updated_ms: u64,
	pub closed: bool,
}
#[derive(Serialize)]
pub struct MetricsPage {
	pub devices: Vec<DeviceSummary>,
	pub selected: Option<MetricsSnapshot>,
	pub storage_error: Option<String>,
	pub truncated: bool,
}
fn identifier(snapshot: &MetricsSnapshot) -> String {
	format!("{}:{}", snapshot.session_id, snapshot.device_id)
}
fn snapshot_file(name: &str) -> bool {
	name.strip_prefix("vulkan-metrics-")
		.and_then(|s| s.strip_suffix(".json"))
		.is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
}
fn load(path: &Path, remaining: &mut usize) -> io::Result<MetricsSnapshot> {
	let file = fs::File::open(path)?;
	if file.metadata()?.len() > MAX_SNAPSHOT_BYTES as u64 {
		return Err(io::Error::other("Resource snapshot exceeds 4 MiB"));
	}
	let mut bytes = Vec::new();
	// Directory metadata is only a scheduling hint: a publisher can replace or
	// grow the file before it is opened. Bound and charge the actual read too,
	// including partial data from failed reads and invalid JSON.
	let limit = (*remaining).min(MAX_SNAPSHOT_BYTES + 1);
	let result = file.take(limit as u64).read_to_end(&mut bytes);
	*remaining -= bytes.len();
	result?;
	if bytes.len() > MAX_SNAPSHOT_BYTES {
		return Err(io::Error::other("Resource snapshot grew beyond its limit"));
	}
	let snapshot: MetricsSnapshot = serde_json::from_slice(&bytes)?;
	if snapshot.schema_version != 1
		|| snapshot.resources.len() > MAX_RESOURCES
		|| snapshot.recent_released.len() > MAX_RELEASED
	{
		return Err(io::Error::other(
			"Unsupported or oversized resource snapshot",
		));
	}
	Ok(snapshot)
}

pub fn read(directory: &Path, selected_id: Option<&str>) -> MetricsPage {
	let mut page = MetricsPage {
		devices: vec![],
		selected: None,
		storage_error: None,
		truncated: false,
	};
	let result = (|| -> io::Result<Vec<SnapshotFile>> {
		let mut sessions = Vec::new();
		for entry in fs::read_dir(directory)?.take(4096) {
			let entry = entry?;
			if entry.file_type()?.is_dir() {
				sessions.push(entry.path());
			}
		}
		// Select by timestamp suffix, including hyphenated application names.
		sessions.sort_by_cached_key(|path| {
			std::cmp::Reverse((
				path.file_name()
					.and_then(|n| n.to_str())
					.and_then(|n| n.rsplit('-').next())
					.and_then(|n| n.parse::<u128>().ok())
					.unwrap_or(0),
				path.file_name().map(|name| name.to_owned()),
			))
		});
		if sessions.len() > 64 {
			page.truncated = true;
			sessions.truncate(64);
		}
		let mut files = Vec::new();
		for session in sessions {
			for entry in fs::read_dir(session)?.take(256) {
				let entry = entry?;
				if entry.file_type()?.is_file()
					&& snapshot_file(&entry.file_name().to_string_lossy())
				{
					let metadata = entry.metadata().ok();
					files.push(SnapshotFile {
						path: entry.path(),
						modified: metadata.as_ref().and_then(|m| m.modified().ok()),
						bytes: metadata.map_or(MAX_SNAPSHOT_BYTES + 1, |m| m.len() as usize),
					});
				}
			}
		}
		files.sort_by_key(|file| std::cmp::Reverse(file.modified));
		Ok(files)
	})();
	let paths = match result {
		Ok(paths) => paths,
		Err(error) if error.kind() == io::ErrorKind::NotFound => return page,
		Err(error) => {
			page.storage_error = Some(error.to_string());
			return page;
		}
	};
	let mut remaining = READ_BUDGET;
	let mut snapshots = Vec::new();
	for file in paths {
		let size = file.bytes;
		if snapshots.len() == MAX_DEVICES || remaining == 0 || size > remaining {
			page.truncated = true;
			break;
		}
		match load(&file.path, &mut remaining) {
			Ok(snapshot) => snapshots.push(snapshot),
			Err(error) => {
				page.storage_error = Some(format!("Resource snapshot could not be read: {error}"))
			}
		}
	}
	// Prefer an active device when the user has not selected a session.
	snapshots.sort_by_key(|snapshot| (snapshot.closed, std::cmp::Reverse(snapshot.updated_ms)));
	let selected = selected_id
		.and_then(|id| snapshots.iter().position(|s| identifier(s) == id))
		.or_else(|| (!snapshots.is_empty()).then_some(0));
	page.devices = snapshots
		.iter()
		.map(|s| DeviceSummary {
			id: identifier(s),
			application: s.application.clone(),
			session_id: s.session_id.clone(),
			process_id: s.process_id,
			gpu_name: s.gpu_name.clone(),
			updated_ms: s.updated_ms,
			closed: s.closed,
		})
		.collect();
	page.selected = selected.map(|i| snapshots.swap_remove(i));
	page
}

#[cfg(test)]
#[path = "tests_metrics.rs"]
mod tests;
