//! Debug resource accounting. Producers never perform file I/O; the existing
//! log writer publishes complete, bounded snapshots beside the session logs.
use serde::{Deserialize, Serialize};
use std::{
	collections::{BTreeMap, VecDeque},
	fs::{self, OpenOptions},
	io::{self, Write},
	path::Path,
	sync::{
		Arc, Mutex, OnceLock,
		atomic::{AtomicBool, AtomicU64, Ordering},
	},
	time::{SystemTime, UNIX_EPOCH},
};

pub const MAX_RESOURCES: usize = 2048;
pub const MAX_RELEASED: usize = 128;
pub const MAX_SNAPSHOT_BYTES: usize = 4 * 1024 * 1024;
const MAX_DEVICES: usize = 64;
static NEXT_DEVICE: AtomicU64 = AtomicU64::new(1);
static DEVICES: OnceLock<Mutex<Vec<Arc<Inner>>>> = OnceLock::new();

fn now_ms() -> u64 {
	SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.unwrap_or_default()
		.as_millis() as u64
}
fn compact(value: &str, limit: usize) -> String {
	if let Some(logger) = super::LOGGER.get() {
		return logger.text(format_args!("{value}"), limit);
	}
	let mut end = value.len().min(limit);
	while !value.is_char_boundary(end) {
		end -= 1;
	}
	value[..end].to_owned()
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct FrameMetrics {
	pub frames: u64,
	pub draw_calls: u64,
	pub instances: u64,
	pub triangles: u64,
	pub gpu_time_ms: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResourceRecord {
	pub id: u64,
	pub kind: String,
	pub handle: String,
	pub label: String,
	pub details: String,
	pub payload_bytes: u64,
	pub allocation_bytes: u64,
	pub memory_type: Option<u32>,
	pub memory_heap: Option<u32>,
	pub device_local: bool,
	pub host_visible: bool,
	pub created_ms: u64,
	pub released_ms: Option<u64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ResourceTotals {
	pub live_resources: u64,
	pub created_resources: u64,
	pub released_resources: u64,
	pub live_allocations: u64,
	pub allocation_bytes: u64,
	pub peak_allocation_bytes: u64,
	pub allocated_bytes_total: u64,
	pub payload_bytes: u64,
	pub device_local_bytes: u64,
	pub host_visible_bytes: u64,
	pub by_kind: BTreeMap<String, u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MetricsSnapshot {
	pub schema_version: u32,
	pub device_id: u64,
	pub application: String,
	pub session_id: String,
	pub process_id: u32,
	pub gpu_name: String,
	pub started_ms: u64,
	pub updated_ms: u64,
	pub closed: bool,
	/// Aggregates include all instrumented resources even if the detail cap is reached.
	pub inventory_truncated: bool,
	pub totals: ResourceTotals,
	pub frame: FrameMetrics,
	pub resources: Vec<ResourceRecord>,
	pub recent_released: Vec<ResourceRecord>,
}

struct State {
	next_id: u64,
	totals: ResourceTotals,
	live: BTreeMap<u64, ResourceRecord>,
	released: VecDeque<ResourceRecord>,
	truncated: bool,
	frame: FrameMetrics,
}
struct Inner {
	id: u64,
	name: String,
	started_ms: u64,
	closed: AtomicBool,
	dirty: AtomicBool,
	state: Mutex<State>,
}

#[derive(Clone)]
pub struct ResourceMetrics(Arc<Inner>);
pub struct ResourceGuard {
	owner: Option<Arc<Inner>>,
	id: u64,
	kind: String,
	payload: u64,
	allocation: u64,
	device_local: bool,
	host_visible: bool,
}
impl ResourceGuard {
	pub fn set_label(&self, label: &str) {
		let Some(owner) = &self.owner else {
			return;
		};
		let label = compact(label, 256);
		let mut state = owner.state.lock().unwrap_or_else(|e| e.into_inner());
		if let Some(record) = state.live.get_mut(&self.id) {
			record.label = label;
		}
		owner.dirty.store(true, Ordering::Release);
	}
	pub fn disabled() -> Self {
		Self {
			owner: None,
			id: 0,
			kind: String::new(),
			payload: 0,
			allocation: 0,
			device_local: false,
			host_visible: false,
		}
	}
}

impl ResourceMetrics {
	pub fn new(gpu_name: &str) -> Option<Self> {
		let devices = DEVICES.get_or_init(|| Mutex::new(Vec::new()));
		let mut devices = devices.lock().unwrap_or_else(|e| e.into_inner());
		if devices.len() == MAX_DEVICES {
			return None;
		}
		let inner = Arc::new(Inner {
			id: NEXT_DEVICE.fetch_add(1, Ordering::Relaxed),
			name: compact(gpu_name, 256),
			started_ms: now_ms(),
			closed: AtomicBool::new(false),
			dirty: AtomicBool::new(true),
			state: Mutex::new(State {
				next_id: 1,
				totals: ResourceTotals::default(),
				live: BTreeMap::new(),
				released: VecDeque::new(),
				truncated: false,
				frame: FrameMetrics::default(),
			}),
		});
		devices.push(Arc::clone(&inner));
		Some(Self(inner))
	}

	pub fn track(&self, mut record: ResourceRecord) -> ResourceGuard {
		record.kind = compact(&record.kind, 48);
		record.handle = compact(&record.handle, 32);
		record.label = compact(&record.label, 256);
		record.details = compact(&record.details, 512);
		let mut state = self.0.state.lock().unwrap_or_else(|e| e.into_inner());
		let id = state.next_id;
		state.next_id += 1;
		record.id = id;
		record.created_ms = now_ms();
		record.released_ms = None;
		let guard = ResourceGuard {
			owner: Some(Arc::clone(&self.0)),
			id,
			kind: record.kind.clone(),
			payload: record.payload_bytes,
			allocation: record.allocation_bytes,
			device_local: record.device_local,
			host_visible: record.host_visible,
		};
		let totals = &mut state.totals;
		totals.live_resources += 1;
		totals.created_resources += 1;
		totals.payload_bytes += record.payload_bytes;
		totals.allocation_bytes += record.allocation_bytes;
		totals.allocated_bytes_total += record.allocation_bytes;
		totals.live_allocations += u64::from(record.allocation_bytes > 0);
		totals.peak_allocation_bytes = totals.peak_allocation_bytes.max(totals.allocation_bytes);
		if record.device_local {
			totals.device_local_bytes += record.allocation_bytes;
		}
		if record.host_visible {
			totals.host_visible_bytes += record.allocation_bytes;
		}
		*totals.by_kind.entry(record.kind.clone()).or_default() += 1;
		if state.live.len() < MAX_RESOURCES {
			state.live.insert(id, record);
		} else {
			state.truncated = true;
		}
		self.0.dirty.store(true, Ordering::Release);
		guard
	}

	pub fn frame(&self, draw_calls: u64, instances: u64, triangles: u64, gpu_time_ms: Option<f64>) {
		let mut state = self.0.state.lock().unwrap_or_else(|e| e.into_inner());
		let frames = state.frame.frames + 1;
		state.frame = FrameMetrics {
			frames,
			draw_calls,
			instances,
			triangles,
			gpu_time_ms: gpu_time_ms.filter(|v| v.is_finite() && *v >= 0.0),
		};
		self.0.dirty.store(true, Ordering::Release);
	}
	pub fn close(&self) {
		self.0.closed.store(true, Ordering::Release);
		self.0.dirty.store(true, Ordering::Release);
	}
	pub fn snapshot(&self) -> MetricsSnapshot {
		let logger = super::LOGGER.get();
		self.0.snapshot(
			logger.map_or("uninitialized", |v| v.config.application.as_str()),
			logger.map_or("uninitialized", |v| v.session.as_str()),
		)
	}
}
impl Inner {
	fn snapshot(&self, application: &str, session: &str) -> MetricsSnapshot {
		let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
		// Tracking may start before logger initialization. Apply the current
		// redaction policy again at the snapshot boundary before publishing it.
		let record = |record: &ResourceRecord| {
			let mut record = record.clone();
			record.kind = compact(&record.kind, 48);
			record.handle = compact(&record.handle, 32);
			record.label = compact(&record.label, 256);
			record.details = compact(&record.details, 512);
			record
		};
		let mut totals = state.totals.clone();
		totals.by_kind.clear();
		for (kind, count) in &state.totals.by_kind {
			*totals.by_kind.entry(compact(kind, 48)).or_default() += count;
		}
		MetricsSnapshot {
			schema_version: 1,
			device_id: self.id,
			application: application.into(),
			session_id: session.into(),
			process_id: std::process::id(),
			gpu_name: compact(&self.name, 256),
			started_ms: self.started_ms,
			updated_ms: now_ms(),
			closed: self.closed.load(Ordering::Acquire),
			inventory_truncated: state.truncated,
			totals,
			frame: state.frame.clone(),
			resources: state.live.values().map(record).collect(),
			recent_released: state.released.iter().map(record).collect(),
		}
	}
}
impl Drop for ResourceGuard {
	fn drop(&mut self) {
		let Some(owner) = &self.owner else {
			return;
		};
		let mut state = owner.state.lock().unwrap_or_else(|e| e.into_inner());
		let totals = &mut state.totals;
		totals.live_resources -= 1;
		totals.released_resources += 1;
		totals.payload_bytes -= self.payload;
		totals.allocation_bytes -= self.allocation;
		totals.live_allocations -= u64::from(self.allocation > 0);
		if self.device_local {
			totals.device_local_bytes -= self.allocation;
		}
		if self.host_visible {
			totals.host_visible_bytes -= self.allocation;
		}
		if let Some(count) = totals.by_kind.get_mut(&self.kind) {
			*count -= 1;
		}
		if let Some(mut record) = state.live.remove(&self.id) {
			record.released_ms = Some(now_ms());
			if state.released.len() == MAX_RELEASED {
				state.released.pop_front();
			}
			state.released.push_back(record);
		}
		owner.dirty.store(true, Ordering::Release);
	}
}

pub(crate) fn owned_file(name: &str) -> bool {
	name.strip_prefix("vulkan-metrics-")
		.and_then(|s| s.strip_suffix(".json").or_else(|| s.strip_suffix(".tmp")))
		.is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
}

/// Called only by the existing background writer. A full snapshot is retried
/// after failure, so dropped log entries cannot corrupt resource accounting.
pub(crate) fn persist(directory: &Path, application: &str, session: &str) -> io::Result<()> {
	let Some(devices) = DEVICES.get() else {
		return Ok(());
	};
	let devices = devices.lock().unwrap_or_else(|e| e.into_inner()).clone();
	let mut failure = None;
	for device in devices {
		let dirty = device.dirty.swap(false, Ordering::AcqRel);
		if !dirty && device.closed.load(Ordering::Acquire) {
			continue;
		}
		let snapshot = device.snapshot(application, session);
		let path = directory
			.join(session)
			.join(format!("vulkan-metrics-{}.json", device.id));
		let temporary = path.with_extension("tmp");
		let result = (|| -> io::Result<()> {
			let bytes = serde_json::to_vec(&snapshot)?;
			if bytes.len() > MAX_SNAPSHOT_BYTES {
				return Err(io::Error::other(
					"Resource metrics snapshot exceeds its byte limit",
				));
			}
			let mut file = OpenOptions::new()
				.write(true)
				.create_new(true)
				.open(&temporary)?;
			file.write_all(&bytes)?;
			drop(file);
			fs::rename(&temporary, &path)
		})();
		if let Err(error) = result {
			let _ = fs::remove_file(&temporary);
			device.dirty.store(true, Ordering::Release);
			failure = Some(error);
		}
	}
	failure.map_or(Ok(()), Err)
}

#[cfg(test)]
#[path = "tests_metrics.rs"]
mod tests;
