use crate::{now_ms, save_json};
use engine_logger::LogEntry;
use serde::{Deserialize, Serialize};
use std::{
	collections::{BTreeMap, VecDeque},
	fs::{self, File},
	io::{self, BufRead, Read, Seek, SeekFrom},
	path::{Path, PathBuf},
};

mod query;

pub use query::{LogFilter, LogPage};

const CAPACITY: usize = 5000;
const RETAINED_BYTES: usize = 16 * 1024 * 1024;
const LINE_LIMIT: u64 = 512 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CollectedLog {
	pub cursor: u64,
	pub entry: LogEntry,
}
#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct CollectorHealth {
	pub storage_error: Option<String>,
	pub checkpoint_error: Option<String>,
	pub malformed_lines: u64,
	pub history_gaps: u64,
	pub oversized_lines: u64,
	pub last_scan_ms: Option<u64>,
	pub scanned_files: usize,
	pub session_limit_reached: bool,
}
#[derive(Serialize, Deserialize)]
struct Checkpoint {
	version: u32,
	id: String,
	next: u64,
	entries: VecDeque<CollectedLog>,
	highwater: BTreeMap<String, u64>,
}
#[derive(Serialize)]
struct CheckpointRef<'a> {
	version: u32,
	id: &'a str,
	next: u64,
	entries: &'a VecDeque<CollectedLog>,
	highwater: &'a BTreeMap<String, u64>,
}
#[derive(Default)]
struct FileCursor {
	offset: u64,
	discard: bool,
}
pub struct Collector {
	pub directory: PathBuf,
	checkpoint: PathBuf,
	id: String,
	next: u64,
	entries: VecDeque<CollectedLog>,
	retained_bytes: usize,
	highwater: BTreeMap<String, u64>,
	files: BTreeMap<PathBuf, FileCursor>,
	pub health: CollectorHealth,
	dirty: bool,
}
impl Collector {
	pub fn new(directory: PathBuf, data: &Path) -> Self {
		let checkpoint = data.join("logs.json");
		let mut collector = Self {
			directory,
			checkpoint,
			id: format!("collector-{}-{}", std::process::id(), now_ms()),
			next: 1,
			entries: VecDeque::new(),
			retained_bytes: 0,
			highwater: BTreeMap::new(),
			files: BTreeMap::new(),
			health: CollectorHealth::default(),
			dirty: false,
		};
		if collector.checkpoint.exists() {
			match File::open(&collector.checkpoint).and_then(|file| {
				serde_json::from_reader::<_, Checkpoint>(file.take(32 * 1024 * 1024))
					.map_err(io::Error::other)
			}) {
				Ok(state)
					if state.version == 1
						&& state.entries.len() <= CAPACITY
						&& state.highwater.len() <= 1024
						&& state.next > state.entries.back().map_or(0, |e| e.cursor) =>
				{
					collector.id = state.id;
					collector.next = state.next;
					collector.entries = state.entries;
					collector.highwater = state.highwater;
					collector.retained_bytes = collector
						.entries
						.iter()
						.map(|e| serde_json::to_vec(e).map_or(0, |v| v.len()))
						.sum();
				}
				Ok(_) => {
					collector.health.checkpoint_error =
						Some("Unsupported collector checkpoint".into())
				}
				Err(error) => collector.health.checkpoint_error = Some(error.to_string()),
			}
		}
		collector
	}
	pub fn scan(&mut self) -> Vec<LogEntry> {
		self.health.last_scan_ms = Some(now_ms());
		let paths = match self.paths() {
			Ok(paths) => {
				self.health.storage_error = None;
				paths
			}
			Err(error) => {
				self.health.storage_error = Some(error.to_string());
				return vec![];
			}
		};
		self.health.scanned_files = paths.len();
		self.files.retain(|path, _| paths.contains(path));
		let mut newly_collected = vec![];
		let mut budget = 8 * 1024 * 1024i64;
		for path in paths {
			if budget <= 0 {
				break;
			}
			let result = self.scan_file(&path, &mut newly_collected, &mut budget);
			if let Err(error) = result {
				self.health.storage_error = Some(format!("{}: {error}", path.display()));
			}
		}
		newly_collected
	}
	fn paths(&mut self) -> io::Result<Vec<PathBuf>> {
		let mut sessions = Vec::new();
		for session in fs::read_dir(&self.directory)? {
			let session = session?;
			if session.file_type()?.is_dir() {
				sessions.push(session.path());
			}
		}
		sessions.sort_by_key(|p| {
			p.file_name()
				.and_then(|v| v.to_str())
				.and_then(|v| v.rsplit('-').next())
				.and_then(|v| v.parse::<u128>().ok())
				.unwrap_or(0)
		});
		self.health.session_limit_reached = sessions.len() > 256;
		let selected = sessions.into_iter().rev().take(256).collect::<Vec<_>>();
		let selected_names = selected
			.iter()
			.filter_map(|p| p.file_name().map(|v| v.to_string_lossy().into_owned()))
			.collect::<Vec<_>>();
		self.highwater.retain(|session, _| {
			selected_names.contains(session) || session.starts_with("managed-")
		});
		let mut paths = vec![];
		for session in selected.into_iter().rev() {
			let mut segments = vec![];
			for segment in fs::read_dir(session)? {
				let segment = segment?;
				let name = segment.file_name().to_string_lossy().into_owned();
				if segment.file_type()?.is_file()
					&& name.ends_with(".jsonl")
					&& name
						.trim_end_matches(".jsonl")
						.bytes()
						.all(|b| b.is_ascii_digit())
				{
					segments.push(segment.path());
				}
			}
			segments.sort();
			paths.extend(segments.into_iter().take(64));
		}
		Ok(paths)
	}
	fn scan_file(
		&mut self,
		path: &Path,
		output: &mut Vec<LogEntry>,
		budget: &mut i64,
	) -> io::Result<()> {
		let mut file = File::open(path)?;
		let len = file.metadata()?.len();
		let cursor = self.files.entry(path.to_path_buf()).or_default();
		if len < cursor.offset {
			cursor.offset = 0;
			cursor.discard = false;
			self.health.history_gaps += 1;
		}
		file.seek(SeekFrom::Start(cursor.offset))?;
		let mut reader = io::BufReader::new(file);
		let mut records = vec![];
		while *budget > 0 {
			let mut line = vec![];
			let count = reader
				.by_ref()
				.take(LINE_LIMIT + 1)
				.read_until(b'\n', &mut line)?;
			if count == 0 {
				break;
			}
			*budget -= count as i64;
			let complete = line.ends_with(b"\n");
			if !complete && count <= LINE_LIMIT as usize && !cursor.discard {
				break;
			}
			cursor.offset += count as u64;
			if cursor.discard || count > LINE_LIMIT as usize {
				if !cursor.discard {
					self.health.oversized_lines += 1;
				}
				cursor.discard = !complete;
				continue;
			}
			match serde_json::from_slice::<LogEntry>(&line) {
				Ok(entry)
					if entry.schema_version == engine_logger::SCHEMA_VERSION
						&& path
							.parent()
							.and_then(|p| p.file_name())
							.is_some_and(|name| name.to_string_lossy() == entry.session_id) =>
				{
					records.push(entry)
				}
				_ => self.health.malformed_lines += 1,
			}
		}

		for entry in records {
			if self.ingest(entry.clone()) {
				output.push(entry);
			}
		}
		Ok(())
	}
	pub fn ingest(&mut self, entry: LogEntry) -> bool {
		let last = self.highwater.entry(entry.session_id.clone()).or_default();
		if entry.sequence <= *last {
			return false;
		}
		if entry.sequence > last.saturating_add(1) {
			self.health.history_gaps += 1;
		}
		*last = entry.sequence;
		let record = CollectedLog {
			cursor: self.next,
			entry,
		};
		let bytes = serde_json::to_vec(&record).map_or(RETAINED_BYTES + 1, |v| v.len());
		if bytes > RETAINED_BYTES {
			self.health.oversized_lines += 1;
			return false;
		}
		while self.entries.len() >= CAPACITY || self.retained_bytes + bytes > RETAINED_BYTES {
			let Some(old) = self.entries.pop_front() else {
				break;
			};
			self.retained_bytes = self
				.retained_bytes
				.saturating_sub(serde_json::to_vec(&old).map_or(0, |v| v.len()));
		}
		self.retained_bytes += bytes;
		self.entries.push_back(record);
		self.next += 1;
		self.dirty = true;
		if self.highwater.len() > 1024
			&& let Some(old) = self
				.highwater
				.keys()
				.find(|key| {
					key.starts_with("managed-")
						&& !self.entries.iter().any(|e| &e.entry.session_id == *key)
				})
				.cloned()
		{
			self.highwater.remove(&old);
		}
		true
	}
	pub fn persist(&mut self) {
		if !self.dirty {
			return;
		}
		let state = CheckpointRef {
			version: 1,
			id: &self.id,
			next: self.next,
			entries: &self.entries,
			highwater: &self.highwater,
		};
		match save_json(&self.checkpoint, &state) {
			Ok(()) => {
				self.dirty = false;
				self.health.checkpoint_error = None;
			}
			Err(error) => self.health.checkpoint_error = Some(error.to_string()),
		}
	}
}
