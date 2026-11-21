//! Single-worker speculative scene decoding. GPU and live state stay with the caller.
use std::{
	fs::{self, File},
	io::{self, Read},
	path::{Path, PathBuf},
	sync::{
		Arc, Condvar, Mutex,
		atomic::{AtomicBool, Ordering},
	},
};

use engine_core::{
	scene_catalog::SceneId,
	scene_data::{ParsedScene, parse_json},
};
use sha2::{Digest, Sha256};

use crate::scene_io::paths::{is_linked, resolve_from_canonical_root};

pub const MAX_PREFETCH_SOURCE_BYTES: usize = 2 * 1024 * 1024;
const MAX_ESTIMATED_RETAINED_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrefetchKey {
	pub asset_root: PathBuf,
	pub scene: SceneId,
	pub source_generation: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PrefetchOutcome {
	Disabled,
	Hit,
	Miss,
	Stale,
}

pub struct PrefetchedScene {
	digest: [u8; 32],
	parsed: ParsedScene,
	estimated_bytes: usize,
}

impl PrefetchedScene {
	pub fn load_or_refresh(
		self,
		path: &Path,
	) -> Result<(ParsedScene, PrefetchOutcome, usize), String> {
		match read_bounded(path) {
			Ok(Some(bytes)) if digest(&bytes) == self.digest => {
				Ok((self.parsed, PrefetchOutcome::Hit, self.estimated_bytes))
			}
			Ok(Some(bytes)) => {
				parse(&bytes).map(|fresh| (fresh, PrefetchOutcome::Stale, self.estimated_bytes))
			}
			Ok(None) => crate::scene_io::load_data(path)
				.map(|fresh| (fresh, PrefetchOutcome::Stale, self.estimated_bytes)),
			Err(error) => Err(format!(
				"Cannot verify the prefetched scene source: {error}"
			)),
		}
	}
}

struct Job {
	key: PrefetchKey,
	path: PathBuf,
	cancelled: Arc<AtomicBool>,
}

#[derive(Default)]
struct State {
	last: Option<PrefetchKey>,
	pending: Option<Job>,
	active: Option<Arc<AtomicBool>>,
	ready: Option<(PrefetchKey, PrefetchedScene)>,
	closed: bool,
}

impl State {
	fn cancel(&mut self) {
		if let Some(active) = self.active.take() {
			active.store(true, Ordering::Relaxed);
		}
		self.last = None;
		self.pending = None;
		self.ready = None;
	}
}

#[derive(Default)]
struct Mailbox {
	state: Mutex<State>,
	changed: Condvar,
}

pub struct ScenePrefetch {
	mailbox: Arc<Mailbox>,
}

impl ScenePrefetch {
	pub fn new() -> io::Result<Self> {
		Self::spawn(prepare)
	}

	fn spawn(
		loader: impl Fn(&Job) -> Option<PrefetchedScene> + Send + 'static,
	) -> io::Result<Self> {
		let mailbox = Arc::new(Mailbox::default());
		let worker = Arc::clone(&mailbox);
		std::thread::Builder::new()
			.name("scene-prefetch".into())
			.spawn(move || run_worker(&worker, loader))?;
		Ok(Self { mailbox })
	}

	/// Queue just the latest authored successor. The render caller never waits for the worker.
	pub fn queue(&self, key: PrefetchKey, path: PathBuf) -> bool {
		let Ok(mut state) = self.mailbox.state.try_lock() else {
			return false;
		};
		if !path
			.components()
			.all(|component| matches!(component, std::path::Component::Normal(_)))
			|| path.as_os_str().is_empty()
		{
			return false;
		}
		if state.last.as_ref() == Some(&key) {
			return true;
		}
		state.cancel();
		let cancelled = Arc::new(AtomicBool::new(false));
		state.last = Some(key.clone());
		state.pending = Some(Job {
			key,
			path,
			cancelled,
		});
		self.mailbox.changed.notify_one();
		true
	}

	/// Claim one matching result and retire all speculative work before foreground preparation.
	pub fn take(&self, key: &PrefetchKey) -> Option<PrefetchedScene> {
		let mut state = self.mailbox.state.lock().ok()?;
		let ready = state.ready.take().filter(|(candidate, _)| candidate == key);
		state.cancel();
		ready.map(|(_, scene)| scene)
	}

	pub fn cancel(&self) {
		if let Ok(mut state) = self.mailbox.state.lock() {
			state.cancel();
		}
	}
}

impl Drop for ScenePrefetch {
	fn drop(&mut self) {
		if let Ok(mut state) = self.mailbox.state.lock() {
			state.cancel();
			state.closed = true;
			self.mailbox.changed.notify_one();
		}
	}
}

fn run_worker(mailbox: &Mailbox, loader: impl Fn(&Job) -> Option<PrefetchedScene>) {
	loop {
		let Ok(state) = mailbox.state.lock() else {
			return;
		};
		let Ok(mut state) = mailbox
			.changed
			.wait_while(state, |state| state.pending.is_none() && !state.closed)
		else {
			return;
		};
		if state.closed {
			return;
		}
		let Some(job) = state.pending.take() else {
			continue;
		};
		state.active = Some(Arc::clone(&job.cancelled));
		drop(state);

		let prepared = loader(&job);

		let Ok(mut state) = mailbox.state.lock() else {
			return;
		};
		if !state.closed
			&& !job.cancelled.load(Ordering::Relaxed)
			&& state.last.as_ref() == Some(&job.key)
		{
			state.active = None;
			state.ready = prepared.map(|scene| (job.key, scene));
			#[cfg(test)]
			mailbox.changed.notify_all();
		}
	}
}

fn prepare(job: &Job) -> Option<PrefetchedScene> {
	let cancelled = || job.cancelled.load(Ordering::Relaxed);
	if cancelled() {
		return None;
	}
	let canonical = resolve_from_canonical_root(&job.key.asset_root, &job.path).ok()?;
	let bytes = read_bounded(&canonical).ok()??;
	if cancelled() {
		return None;
	}
	let parsed = parse(&bytes).ok()?;
	let estimated_bytes = parsed
		.data
		.serialized_size()
		.ok()?
		.checked_mul(4)?
		.checked_add(std::mem::size_of::<ParsedScene>())?;
	if cancelled() || estimated_bytes > MAX_ESTIMATED_RETAINED_BYTES {
		return None;
	}
	Some(PrefetchedScene {
		digest: digest(&bytes),
		parsed,
		estimated_bytes,
	})
}

fn read_bounded(path: &Path) -> io::Result<Option<Vec<u8>>> {
	let expected = fs::symlink_metadata(path)?;
	if !ordinary(&expected) {
		return Err(io::Error::new(
			io::ErrorKind::InvalidData,
			"Scene source must be an ordinary file",
		));
	}
	if expected.len() > MAX_PREFETCH_SOURCE_BYTES as u64 {
		return Ok(None);
	}
	let file = File::open(path)?;
	let observed = file.metadata()?;
	if !ordinary(&observed) || observed.len() != expected.len() {
		return Err(io::Error::new(
			io::ErrorKind::InvalidData,
			"Scene source changed before reading",
		));
	}
	let mut bytes = Vec::new();
	file.take(MAX_PREFETCH_SOURCE_BYTES as u64 + 1)
		.read_to_end(&mut bytes)?;
	if bytes.len() != observed.len() as usize {
		return Err(io::Error::new(
			io::ErrorKind::InvalidData,
			"Scene source changed while reading",
		));
	}
	Ok(Some(bytes))
}

fn ordinary(metadata: &fs::Metadata) -> bool {
	metadata.is_file() && !is_linked(metadata)
}

fn parse(bytes: &[u8]) -> Result<ParsedScene, String> {
	parse_json(std::str::from_utf8(bytes).map_err(|error| error.to_string())?)
}

fn digest(bytes: &[u8]) -> [u8; 32] {
	Sha256::digest(bytes).into()
}

#[cfg(test)]
#[path = "../tests/unit/scene_prefetch.rs"]
mod tests;
