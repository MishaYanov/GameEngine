//! One bounded CPU importer worker. Submission and draining never wait for
//! decoding, channel capacity, or the worker's completion. GPU upload belongs
//! to the render owner after it accepts a current-generation result.
use std::{
	collections::HashMap,
	fmt,
	sync::{
		Arc, Mutex, TryLockError,
		atomic::{AtomicBool, AtomicU64, Ordering},
		mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
	},
	thread::JoinHandle,
};

use serde::{Deserialize, Serialize};

use crate::asset_registry::{AssetMetadata, AssetRef};

pub const MAX_ASSET_ERROR_BYTES: usize = 2048;
static NEXT_JOB_ID: AtomicU64 = AtomicU64::new(1);
fn bounded_import_error(mut message: String) -> String {
	if message.len() > MAX_ASSET_ERROR_BYTES {
		let mut end = MAX_ASSET_ERROR_BYTES;
		while !message.is_char_boundary(end) {
			end -= 1;
		}
		message.truncate(end);
	}
	// Truncation alone would retain the original potentially large allocation.
	message.into_boxed_str().into_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AssetJobId(u64);
impl AssetJobId {
	pub fn value(self) -> u64 {
		self.0
	}
}

#[derive(Debug, Clone, Copy)]
pub struct AssetJobLimits {
	pub max_pending_jobs: usize,
	pub max_reserved_bytes: usize,
	pub max_job_bytes: usize,
}
impl Default for AssetJobLimits {
	fn default() -> Self {
		Self {
			max_pending_jobs: 8,
			max_reserved_bytes: 256 * 1024 * 1024,
			max_job_bytes: 256 * 1024 * 1024,
		}
	}
}

pub struct ImportedAsset<T> {
	pub value: T,
	pub metadata: AssetMetadata,
	pub bytes: usize,
}

/// Implementations must return CPU data only, account their retained payload in
/// `bytes`, honor the context's byte limit before allocation, and periodically
/// check cancellation while decoding. The broker checks the result again.
pub trait CpuAssetImporter: Send + 'static {
	type Output: Send + 'static;
	fn import(
		&self,
		reference: &AssetRef,
		context: &AssetJobContext,
	) -> Result<ImportedAsset<Self::Output>, String>;
}

pub struct AssetJobContext {
	cancelled: Arc<AtomicBool>,
	current_generation: Arc<AtomicU64>,
	shutdown: Arc<AtomicBool>,
	generation: u64,
	byte_limit: usize,
}
impl AssetJobContext {
	pub fn byte_limit(&self) -> usize {
		self.byte_limit
	}
	pub fn generation(&self) -> u64 {
		self.generation
	}
	pub fn is_cancelled(&self) -> bool {
		self.cancelled.load(Ordering::Acquire)
			|| self.shutdown.load(Ordering::Acquire)
			|| self.current_generation.load(Ordering::Acquire) != self.generation
	}
	pub fn check_cancelled(&self) -> Result<(), String> {
		if self.is_cancelled() {
			Err("Asset import cancelled".into())
		} else {
			Ok(())
		}
	}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetJobError {
	Cancelled,
	Import(String),
	ByteLimit { bytes: usize, limit: usize },
}
pub struct AssetJobResult<T> {
	pub id: AssetJobId,
	pub generation: u64,
	pub reference: AssetRef,
	pub result: Result<ImportedAsset<T>, AssetJobError>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetSubmitError {
	InvalidReference(String),
	QueueFull,
	ByteBudget,
	StateBusy,
	WorkerStopped,
	IdExhausted,
}
impl fmt::Display for AssetSubmitError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::InvalidReference(error) => write!(f, "Invalid asset reference: {error}"),
			Self::QueueFull => write!(f, "Asset job queue is full"),
			Self::ByteBudget => write!(f, "Asset job byte reservation exceeds its budget"),
			Self::StateBusy => write!(f, "Asset job state is busy; retry later"),
			Self::WorkerStopped => write!(f, "Asset importer worker has stopped"),
			Self::IdExhausted => write!(f, "Asset job identity or generation exhausted"),
		}
	}
}
impl std::error::Error for AssetSubmitError {}

#[derive(Debug, Clone, Copy)]
pub struct AssetJobStats {
	pub generation: u64,
	pub pending_jobs: usize,
	pub reserved_bytes: usize,
	pub worker_finished: bool,
}
struct Reservation {
	bytes: usize,
	cancelled: Arc<AtomicBool>,
}
struct Broker {
	jobs: HashMap<AssetJobId, Reservation>,
	reserved_bytes: usize,
}
struct Request {
	id: AssetJobId,
	reference: AssetRef,
	context: AssetJobContext,
}

pub struct AssetJobs<T> {
	requests: Option<SyncSender<Request>>,
	results: Receiver<AssetJobResult<T>>,
	broker: Arc<Mutex<Broker>>,
	current_generation: Arc<AtomicU64>,
	shutdown: Arc<AtomicBool>,
	limits: AssetJobLimits,
	worker: JoinHandle<()>,
}

impl<T: Send + 'static> AssetJobs<T> {
	pub fn new<I: CpuAssetImporter<Output = T>>(
		importer: I,
		limits: AssetJobLimits,
	) -> Result<Self, String> {
		Self::new_with_notifier(importer, limits, || {})
	}

	/// Notify the owner's event loop after a completion is queued. This keeps
	/// adoption event-driven; the callback must be brief and must not upload GPU data.
	pub fn new_with_notifier<I: CpuAssetImporter<Output = T>>(
		importer: I,
		limits: AssetJobLimits,
		on_ready: impl Fn() + Send + 'static,
	) -> Result<Self, String> {
		if limits.max_pending_jobs == 0
			|| limits.max_pending_jobs > 64
			|| limits.max_reserved_bytes == 0
			|| limits.max_reserved_bytes > 1024 * 1024 * 1024
			|| limits.max_job_bytes == 0
			|| limits.max_job_bytes > limits.max_reserved_bytes
		{
			return Err("Invalid asset job count or byte limits".into());
		}
		let (requests, incoming) = mpsc::sync_channel::<Request>(limits.max_pending_jobs);
		let (completed, results) = mpsc::sync_channel(limits.max_pending_jobs);
		let broker = Arc::new(Mutex::new(Broker {
			jobs: HashMap::new(),
			reserved_bytes: 0,
		}));
		let current_generation = Arc::new(AtomicU64::new(1));
		let shutdown = Arc::new(AtomicBool::new(false));
		let worker_shutdown = Arc::clone(&shutdown);
		let worker = std::thread::Builder::new()
			.name("engine-asset-import".into())
			.spawn(move || {
				while let Ok(request) = incoming.recv() {
					if worker_shutdown.load(Ordering::Acquire) {
						break;
					}
					let result = if request.context.is_cancelled() {
						Err(AssetJobError::Cancelled)
					} else {
						let imported =
							std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
								importer.import(&request.reference, &request.context)
							}));
						if request.context.is_cancelled() {
							Err(AssetJobError::Cancelled)
						} else {
							match imported {
								Ok(Ok(asset)) if asset.bytes <= request.context.byte_limit() => {
									Ok(asset)
								}
								Ok(Ok(asset)) => Err(AssetJobError::ByteLimit {
									bytes: asset.bytes,
									limit: request.context.byte_limit(),
								}),
								Ok(Err(error)) => {
									Err(AssetJobError::Import(bounded_import_error(error)))
								}
								Err(_) => {
									Err(AssetJobError::Import("CPU asset importer panicked".into()))
								}
							}
						}
					};
					// The global pending count includes completed results, so this
					// channel has a slot for every accepted reservation.
					if completed
						.send(AssetJobResult {
							id: request.id,
							generation: request.context.generation(),
							reference: request.reference,
							result,
						})
						.is_err()
					{
						break;
					}
					let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(&on_ready));
				}
			})
			.map_err(|error| format!("Cannot start CPU asset importer: {error}"))?;
		Ok(Self {
			requests: Some(requests),
			results,
			broker,
			current_generation,
			shutdown,
			limits,
			worker,
		})
	}

	pub fn try_submit(
		&self,
		reference: AssetRef,
		reserved_bytes: usize,
	) -> Result<AssetJobId, AssetSubmitError> {
		let reference = reference
			.canonical()
			.map_err(AssetSubmitError::InvalidReference)?;
		let requests = self
			.requests
			.as_ref()
			.ok_or(AssetSubmitError::WorkerStopped)?;
		if self.shutdown.load(Ordering::Acquire) || self.worker.is_finished() {
			return Err(AssetSubmitError::WorkerStopped);
		}
		if reserved_bytes == 0 || reserved_bytes > self.limits.max_job_bytes {
			return Err(AssetSubmitError::ByteBudget);
		}
		let mut broker = match self.broker.try_lock() {
			Ok(broker) => broker,
			Err(TryLockError::Poisoned(error)) => error.into_inner(),
			Err(TryLockError::WouldBlock) => return Err(AssetSubmitError::StateBusy),
		};
		if broker.jobs.len() >= self.limits.max_pending_jobs {
			return Err(AssetSubmitError::QueueFull);
		}
		if broker
			.reserved_bytes
			.checked_add(reserved_bytes)
			.is_none_or(|bytes| bytes > self.limits.max_reserved_bytes)
		{
			return Err(AssetSubmitError::ByteBudget);
		}
		let id = AssetJobId(
			NEXT_JOB_ID
				.fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
					value.checked_add(1)
				})
				.map_err(|_| AssetSubmitError::IdExhausted)?,
		);
		let cancelled = Arc::new(AtomicBool::new(false));
		let context = AssetJobContext {
			cancelled: Arc::clone(&cancelled),
			current_generation: Arc::clone(&self.current_generation),
			shutdown: Arc::clone(&self.shutdown),
			generation: self.generation(),
			byte_limit: reserved_bytes,
		};
		broker.jobs.insert(
			id,
			Reservation {
				bytes: reserved_bytes,
				cancelled,
			},
		);
		broker.reserved_bytes += reserved_bytes;
		let request = Request {
			id,
			reference,
			context,
		};
		let result = requests.try_send(request);
		if let Err(error) = result {
			release(&mut broker, id);
			return Err(match error {
				TrySendError::Full(_) => AssetSubmitError::QueueFull,
				TrySendError::Disconnected(_) => AssetSubmitError::WorkerStopped,
			});
		}
		Ok(id)
	}

	pub fn cancel(&self, id: AssetJobId) -> Result<bool, AssetSubmitError> {
		let broker = match self.broker.try_lock() {
			Ok(broker) => broker,
			Err(TryLockError::Poisoned(error)) => error.into_inner(),
			Err(TryLockError::WouldBlock) => return Err(AssetSubmitError::StateBusy),
		};
		Ok(broker.jobs.get(&id).is_some_and(|job| {
			job.cancelled.store(true, Ordering::Release);
			true
		}))
	}

	pub fn generation(&self) -> u64 {
		self.current_generation.load(Ordering::Acquire)
	}
	/// Invalidate all older imports immediately; their reservations remain held
	/// until results are drained, preventing memory growth across scene reloads.
	pub fn advance_generation(&self) -> Result<u64, AssetSubmitError> {
		self.current_generation
			.fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
				value.checked_add(1)
			})
			.map(|previous| previous + 1)
			.map_err(|_| AssetSubmitError::IdExhausted)
	}

	pub fn try_drain(&mut self) -> Option<AssetJobResult<T>> {
		let mut broker = match self.broker.try_lock() {
			Ok(broker) => broker,
			Err(TryLockError::Poisoned(error)) => error.into_inner(),
			Err(TryLockError::WouldBlock) => return None,
		};
		for _ in 0..self.limits.max_pending_jobs {
			let mut result = match self.results.try_recv() {
				Ok(result) => result,
				Err(TryRecvError::Empty | TryRecvError::Disconnected) => return None,
			};
			let cancelled = broker
				.jobs
				.get(&result.id)
				.is_some_and(|job| job.cancelled.load(Ordering::Acquire));
			release(&mut broker, result.id);
			if result.generation == self.generation() {
				if cancelled {
					result.result = Err(AssetJobError::Cancelled);
				}
				return Some(result);
			}
		}
		None
	}

	pub fn try_stats(&self) -> Option<AssetJobStats> {
		let broker = self.broker.try_lock().ok()?;
		Some(AssetJobStats {
			generation: self.generation(),
			pending_jobs: broker.jobs.len(),
			reserved_bytes: broker.reserved_bytes,
			worker_finished: self.worker.is_finished(),
		})
	}
}

impl<T> Drop for AssetJobs<T> {
	fn drop(&mut self) {
		self.shutdown.store(true, Ordering::Release);
		self.requests.take();
		// Dropping a JoinHandle detaches: UI destruction never waits on CPU
		// decoding. Cooperative context checks let the worker retire promptly.
	}
}

fn release(broker: &mut Broker, id: AssetJobId) {
	if let Some(reservation) = broker.jobs.remove(&id) {
		broker.reserved_bytes -= reservation.bytes;
	}
}
