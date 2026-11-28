//! Single-worker scene document preparation. GPU and live state stay with the caller.
use std::{
	fs::File,
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

use crate::scene_io::{load_opened_data, paths::open_confined_scene};

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
	asset_root: PathBuf,
	digest: [u8; 32],
	parsed: ParsedScene,
	estimated_bytes: usize,
}

impl PrefetchedScene {
	pub fn load_or_refresh(
		self,
		path: &Path,
	) -> Result<(ParsedScene, PrefetchOutcome, usize), String> {
		let relative = path
			.strip_prefix(&self.asset_root)
			.map_err(|_| "Prefetched scene must be inside the asset root")?;
		let (_, document, outcome, retained) = self.load_relative(relative)?;
		Ok((document, outcome, retained))
	}

	/// Verify a retained scene against the current project-relative source.
	pub fn load_relative(
		self,
		relative: &Path,
	) -> Result<(PathBuf, ParsedScene, PrefetchOutcome, usize), String> {
		let (path, file) = open_confined_scene(&self.asset_root, relative)?;
		let (document, outcome, retained) = self.refresh_from_file(&path, file)?;
		Ok((path, document, outcome, retained))
	}

	fn refresh_from_file(
		self,
		path: &Path,
		file: File,
	) -> Result<(ParsedScene, PrefetchOutcome, usize), String> {
		match read_bounded(&file) {
			Ok(Some(bytes)) if digest(&bytes) == self.digest => {
				Ok((self.parsed, PrefetchOutcome::Hit, self.estimated_bytes))
			}
			Ok(Some(bytes)) => {
				parse(&bytes).map(|fresh| (fresh, PrefetchOutcome::Stale, self.estimated_bytes))
			}
			Ok(None) => load_opened_data(path, file)
				.map(|fresh| (fresh, PrefetchOutcome::Stale, self.estimated_bytes)),
			Err(error) => Err(format!(
				"Cannot verify the prefetched scene source: {error}"
			)),
		}
	}
}

pub struct ConfirmedScene {
	pub path: PathBuf,
	pub document: ParsedScene,
	pub outcome: PrefetchOutcome,
	pub retained_estimate_bytes: usize,
}

enum Work {
	Speculate,
	Confirm(Option<PrefetchedScene>),
}

struct Job {
	key: PrefetchKey,
	path: PathBuf,
	work: Work,
	cancelled: Arc<AtomicBool>,
}

enum Ready {
	Speculative(PrefetchedScene),
	Confirmed(Result<ConfirmedScene, String>),
}

#[derive(Default)]
struct State {
	last: Option<PrefetchKey>,
	confirmed: bool,
	pending: Option<Job>,
	active: Option<Arc<AtomicBool>>,
	ready: Option<(PrefetchKey, Ready)>,
	closed: bool,
}

impl State {
	fn cancel(&mut self) {
		if let Some(active) = self.active.take() {
			active.store(true, Ordering::Relaxed);
		}
		self.last = None;
		self.confirmed = false;
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
		self.queue_work(key, path, false)
	}

	/// Promote a ready speculative document or read the destination under the
	/// normal scene size limit. The source check runs on the worker in both cases.
	pub fn confirm(&self, key: PrefetchKey, path: PathBuf) -> bool {
		self.queue_work(key, path, true)
	}

	fn queue_work(&self, key: PrefetchKey, path: PathBuf, confirmed: bool) -> bool {
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
		let same = state.last.as_ref() == Some(&key);
		if same && state.confirmed == confirmed {
			return true;
		}
		if state.confirmed && !confirmed {
			return false;
		}
		let work = if confirmed {
			let warmed = match state.ready.take() {
				Some((candidate, Ready::Speculative(scene))) if candidate == key => Some(scene),
				_ => None,
			};
			Work::Confirm(warmed)
		} else {
			Work::Speculate
		};
		state.cancel();
		let cancelled = Arc::new(AtomicBool::new(false));
		state.last = Some(key.clone());
		state.confirmed = confirmed;
		state.pending = Some(Job {
			key,
			path,
			work,
			cancelled,
		});
		self.mailbox.changed.notify_one();
		true
	}

	/// Claim one matching result and retire all speculative work before foreground preparation.
	pub fn take(&self, key: &PrefetchKey) -> Option<PrefetchedScene> {
		let mut state = self.mailbox.state.lock().ok()?;
		if state.confirmed {
			return None;
		}
		let ready = state.ready.take().filter(|(candidate, _)| candidate == key);
		state.cancel();
		match ready {
			Some((_, Ready::Speculative(scene))) => Some(scene),
			_ => None,
		}
	}

	/// Poll without blocking the native owner thread. Only the exact requester
	/// can consume the result. A failed result is terminal for this submission.
	pub fn take_confirmed(&self, key: &PrefetchKey) -> Option<Result<ConfirmedScene, String>> {
		let mut state = self.mailbox.state.try_lock().ok()?;
		if !matches!(&state.ready, Some((candidate, Ready::Confirmed(_))) if candidate == key) {
			return None;
		}
		let result = state.ready.take();
		state.cancel();
		match result {
			Some((_, Ready::Confirmed(result))) => Some(result),
			_ => None,
		}
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
		let Some(mut job) = state.pending.take() else {
			continue;
		};
		state.active = Some(Arc::clone(&job.cancelled));
		drop(state);

		let work = std::mem::replace(&mut job.work, Work::Speculate);
		let prepared = match work {
			Work::Speculate => loader(&job).map(Ready::Speculative),
			Work::Confirm(warmed) => Some(Ready::Confirmed(prepare_confirmed(&job, warmed))),
		};

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

fn prepare_confirmed(job: &Job, warmed: Option<PrefetchedScene>) -> Result<ConfirmedScene, String> {
	let (path, file) = open_confined_scene(&job.key.asset_root, &job.path)?;
	let (document, outcome, retained_estimate_bytes) = match warmed {
		Some(warmed) => warmed.refresh_from_file(&path, file)?,
		None => (load_opened_data(&path, file)?, PrefetchOutcome::Miss, 0),
	};
	Ok(ConfirmedScene {
		path,
		document,
		outcome,
		retained_estimate_bytes,
	})
}

fn prepare(job: &Job) -> Option<PrefetchedScene> {
	let cancelled = || job.cancelled.load(Ordering::Relaxed);
	if cancelled() {
		return None;
	}
	let (_, file) = open_confined_scene(&job.key.asset_root, &job.path).ok()?;
	let bytes = read_bounded(&file).ok()??;
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
		asset_root: job.key.asset_root.clone(),
		digest: digest(&bytes),
		parsed,
		estimated_bytes,
	})
}

fn read_bounded(file: &File) -> io::Result<Option<Vec<u8>>> {
	let observed = file.metadata()?;
	if observed.len() > MAX_PREFETCH_SOURCE_BYTES as u64 {
		return Ok(None);
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

fn parse(bytes: &[u8]) -> Result<ParsedScene, String> {
	parse_json(std::str::from_utf8(bytes).map_err(|error| error.to_string())?)
}

fn digest(bytes: &[u8]) -> [u8; 32] {
	Sha256::digest(bytes).into()
}

#[cfg(test)]
#[path = "../tests/unit/scene_prefetch.rs"]
mod tests;
