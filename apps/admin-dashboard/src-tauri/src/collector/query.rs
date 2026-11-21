use super::{CollectedLog, Collector};
use crate::now_ms;
use engine_logger::{Level, LogEntry};
use serde::{Deserialize, Serialize};
use std::{
	fs,
	io::{self, Write},
	path::{Path, PathBuf},
};

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct LogFilter {
	pub severity: Option<Level>,
	pub application: Option<String>,
	pub component: Option<String>,
	pub session_id: Option<String>,
	pub search: Option<String>,
	pub operation_id: Option<String>,
	pub from_ms: Option<u64>,
	pub to_ms: Option<u64>,
}
struct PreparedFilter<'a> {
	filter: &'a LogFilter,
	search: Option<String>,
}

impl LogFilter {
	pub fn matches(&self, entry: &LogEntry) -> bool {
		self.prepare().matches(entry)
	}
	fn prepare(&self) -> PreparedFilter<'_> {
		PreparedFilter {
			filter: self,
			search: self.search.as_ref().map(|query| query.to_lowercase()),
		}
	}
}
impl PreparedFilter<'_> {
	fn matches(&self, entry: &LogEntry) -> bool {
		let filter = self.filter;
		filter
			.operation_id
			.as_ref()
			.is_none_or(|v| entry.correlation.operation_id.as_ref() == Some(v))
			&& filter.severity.is_none_or(|v| v == entry.level)
			&& filter
				.application
				.as_ref()
				.is_none_or(|v| v == &entry.application)
			&& filter
				.component
				.as_ref()
				.is_none_or(|v| v == &entry.component)
			&& filter
				.session_id
				.as_ref()
				.is_none_or(|v| v == &entry.session_id)
			&& filter.from_ms.is_none_or(|v| entry.timestamp_ms >= v)
			&& filter.to_ms.is_none_or(|v| entry.timestamp_ms <= v)
			&& self
				.search
				.as_ref()
				.is_none_or(|v| entry.message.to_lowercase().contains(v))
	}
}
#[derive(Clone, Debug, Serialize)]
pub struct LogPage {
	pub collector_id: String,
	pub entries: Vec<CollectedLog>,
	pub next_cursor: u64,
	pub latest_cursor: u64,
	pub history_gap: bool,
	pub has_more: bool,
	pub matching_count: usize,
}

impl Collector {
	pub fn page(&self, after: u64, limit: usize, filter: &LogFilter) -> LogPage {
		let filter = filter.prepare();
		let limit = limit.clamp(1, 500);
		let mut matching_count = 0;
		let mut entries = Vec::with_capacity(limit.min(self.entries.len()));
		let mut has_more = false;
		for entry in &self.entries {
			if !filter.matches(&entry.entry) {
				continue;
			}
			matching_count += 1;
			if entry.cursor > after {
				if entries.len() < limit {
					entries.push(entry.clone());
				} else {
					has_more = true;
				}
			}
		}
		let next_cursor = if has_more {
			entries.last().map_or(after, |e| e.cursor)
		} else {
			self.next - 1
		};
		LogPage {
			collector_id: self.id.clone(),
			entries,
			next_cursor,
			latest_cursor: self.next - 1,
			history_gap: self
				.entries
				.front()
				.is_some_and(|e| e.cursor > after.saturating_add(1)),
			has_more,
			matching_count,
		}
	}

	pub fn find(&self, cursor: u64) -> Option<&LogEntry> {
		self.entries
			.iter()
			.find(|entry| entry.cursor == cursor)
			.map(|e| &e.entry)
	}
	pub fn export(&self, directory: &Path, filter: &LogFilter) -> io::Result<(PathBuf, usize)> {
		let filter = filter.prepare();
		fs::create_dir_all(directory)?;
		let path = directory.join(format!("logs-{}-{}.jsonl", now_ms(), self.next));
		let file = fs::OpenOptions::new()
			.write(true)
			.create_new(true)
			.open(&path)?;
		let mut file = io::BufWriter::new(file);
		let mut count = 0;
		for entry in self.entries.iter().filter(|e| filter.matches(&e.entry)) {
			serde_json::to_writer(&mut file, &entry.entry)?;
			file.write_all(b"\n")?;
			count += 1;
		}
		file.flush()?;
		Ok((path, count))
	}
}
