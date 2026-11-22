//! A bounded CPU worker for one selected inactive authored scene.
use std::{
	collections::VecDeque,
	path::{Path, PathBuf},
	sync::{
		atomic::{AtomicBool, Ordering},
		Arc, Condvar, Mutex,
	},
};

use bridge::viewport::{
	SceneWarmingMode, SceneWarmingResultDto, WarmingSessionReportDto, WarmingSessionRowDto,
};
use runtime::{
	assets::PreparedModelBatch, scene_catalog::SceneId, scene_io::PreparedSceneReferences,
	SceneData,
};

use super::EditorWorkspace;

const MAX_REPORT_ROWS: usize = 32;
const MAX_REPORT_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
struct WarmKey {
	workspace: u64,
	revision: u64,
	asset_generation: u64,
	mode: SceneWarmingMode,
	origin: SceneId,
	scene: SceneId,
}

struct Job {
	key: WarmKey,
	document: Arc<SceneData>,
	asset_root: PathBuf,
	cancel: Arc<AtomicBool>,
}

pub(crate) struct WarmedScene {
	pub(crate) references: PreparedSceneReferences,
	pub(crate) models: Option<PreparedModelBatch>,
}

#[derive(Default)]
struct Pending {
	selected: Option<WarmKey>,
	cancel: Option<Arc<AtomicBool>>,
	queue: Option<Job>,
	ready: Option<WarmedScene>,
	closed: bool,
}

impl Pending {
	fn cancel(&mut self) -> Option<WarmedScene> {
		if let Some(cancel) = self.cancel.take() {
			cancel.store(true, Ordering::Relaxed);
		}
		self.selected = None;
		self.queue = None;
		self.ready.take()
	}
}

#[derive(Default)]
struct Mailbox {
	state: Mutex<Pending>,
	wake: Condvar,
}

struct WarmWorker {
	mailbox: Arc<Mailbox>,
}

impl WarmWorker {
	fn new() -> Result<Self, String> {
		Self::spawn(|job| {
			let cancelled = || job.cancel.load(Ordering::Relaxed);
			let references =
				PreparedSceneReferences::build(Arc::clone(&job.document), cancelled).ok()??;
			let models = if job.key.mode == SceneWarmingMode::Models {
				PreparedModelBatch::prepare(
					&job.asset_root,
					references.static_model_assets(),
					&cancelled,
				)
			} else {
				None
			};
			Some(WarmedScene { references, models })
		})
	}

	fn spawn(
		prepare: impl Fn(&Job) -> Option<WarmedScene> + Send + 'static,
	) -> Result<Self, String> {
		let mailbox = Arc::new(Mailbox::default());
		let owned = Arc::clone(&mailbox);
		std::thread::Builder::new()
			.name("editor-scene-warm".into())
			.spawn(move || work(&owned, prepare))
			.map_err(|error| error.to_string())?;
		Ok(Self { mailbox })
	}

	fn request(&self, key: WarmKey, document: Arc<SceneData>, asset_root: PathBuf) {
		let Ok(mut state) = self.mailbox.state.lock() else {
			return;
		};
		if state.selected.as_ref() == Some(&key) {
			return;
		}
		let retired = state.cancel();
		let cancel = Arc::new(AtomicBool::new(false));
		state.selected = Some(key.clone());
		state.cancel = Some(Arc::clone(&cancel));
		state.queue = Some(Job {
			key,
			document,
			asset_root,
			cancel,
		});
		self.mailbox.wake.notify_one();
		drop(state);
		drop(retired);
	}
}

impl Drop for WarmWorker {
	fn drop(&mut self) {
		if let Ok(mut state) = self.mailbox.state.lock() {
			let retired = state.cancel();
			state.closed = true;
			self.mailbox.wake.notify_one();
			drop(state);
			drop(retired);
		}
	}
}

fn work(mailbox: &Mailbox, prepare: impl Fn(&Job) -> Option<WarmedScene>) {
	loop {
		let Ok(state) = mailbox.state.lock() else {
			return;
		};
		let Ok(mut state) = mailbox
			.wake
			.wait_while(state, |value| !value.closed && value.queue.is_none())
		else {
			return;
		};
		if state.closed {
			return;
		}
		let Some(job) = state.queue.take() else {
			continue;
		};
		drop(state);
		let prepared = prepare(&job);
		let Ok(mut state) = mailbox.state.lock() else {
			return;
		};
		if !state.closed
			&& !job.cancel.load(Ordering::Relaxed)
			&& state.selected.as_ref() == Some(&job.key)
		{
			state.ready = prepared;
			#[cfg(test)]
			mailbox.wake.notify_all();
		}
	}
}

#[derive(Default)]
pub(crate) struct SceneWarming {
	worker: Option<WarmWorker>,
	mode: SceneWarmingMode,
	mode_revision: u64,
	asset_generation: u64,
	latest: Option<SceneWarmingResultDto>,
	presented: VecDeque<WarmingSessionRowDto>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct WarmingContext {
	pub(crate) mode: SceneWarmingMode,
	pub(crate) mode_revision: u64,
	pub(crate) asset_generation: u64,
}

pub(crate) enum WarmClaim {
	Unrequested,
	Hit(WarmedScene),
	Miss,
	Stale,
}

impl WarmClaim {
	pub(crate) fn into_scene(self) -> (Option<WarmedScene>, &'static str) {
		match self {
			Self::Unrequested => (None, "unrequested"),
			Self::Hit(scene) => (Some(scene), "hit"),
			Self::Miss => (None, "miss"),
			Self::Stale => (None, "stale"),
		}
	}
}

impl SceneWarming {
	pub(crate) fn context(&self) -> WarmingContext {
		WarmingContext {
			mode: self.mode,
			mode_revision: self.mode_revision,
			asset_generation: self.asset_generation,
		}
	}

	fn set_mode(&mut self, mode: SceneWarmingMode) {
		if self.mode == mode {
			return;
		}
		self.mode = mode;
		self.mode_revision = self.mode_revision.wrapping_add(1);
		self.latest = None;
		self.cancel();
	}

	pub(crate) fn record_presented(&mut self, context: WarmingContext, row: WarmingSessionRowDto) {
		if self.context() == context {
			self.latest = Some(row.result.clone());
		}
		if self.presented.len() == MAX_REPORT_ROWS {
			self.presented.pop_front();
		}
		self.presented.push_back(row);
	}

	fn report(&self, session_id: String, workspace_generation: u64) -> WarmingSessionReportDto {
		WarmingSessionReportDto {
			schema_version: 1,
			session_id,
			workspace_generation,
			rows: self.presented.iter().cloned().collect(),
		}
	}

	pub(crate) fn latest(&self, scene: &str, revision: u64) -> Option<&SceneWarmingResultDto> {
		self.latest
			.as_ref()
			.filter(|result| result.scene_id.as_str() == scene && result.scene_revision == revision)
	}

	pub(crate) fn cancel(&self) {
		if let Some(worker) = &self.worker {
			if let Ok(mut state) = worker.mailbox.state.lock() {
				let retired = state.cancel();
				drop(state);
				drop(retired);
			}
		}
	}

	pub(crate) fn invalidate_assets(&mut self) {
		self.asset_generation = self.asset_generation.wrapping_add(1);
		self.latest = None;
		self.cancel();
	}

	pub(crate) fn is_requested(&self) -> bool {
		self.worker.as_ref().is_some_and(|worker| {
			worker
				.mailbox
				.state
				.try_lock()
				.is_ok_and(|state| state.selected.is_some())
		})
	}

	pub(crate) fn claim(&self, workspace: u64, scene: &SceneId, document: &SceneData) -> WarmClaim {
		let Some(worker) = &self.worker else {
			return WarmClaim::Unrequested;
		};
		let Ok(mut state) = worker.mailbox.state.lock() else {
			return WarmClaim::Miss;
		};
		let Some(key) = state.selected.as_ref() else {
			return WarmClaim::Unrequested;
		};
		if key.workspace == workspace && &key.origin == scene {
			return WarmClaim::Unrequested;
		}
		if key.workspace != workspace
			|| &key.scene != scene
			|| key.mode != self.mode
			|| key.asset_generation != self.asset_generation
		{
			let retired = state.cancel();
			drop(state);
			drop(retired);
			return WarmClaim::Stale;
		}
		let prepared = state.cancel();
		drop(state);
		match prepared {
			Some(scene) if scene.references.matches(document) => WarmClaim::Hit(scene),
			Some(_) => WarmClaim::Stale,
			None => WarmClaim::Miss,
		}
	}
}

impl EditorWorkspace {
	pub(crate) fn check_warming_export(&self, expected_generation: u64) -> Result<(), String> {
		if self.generation != expected_generation {
			return Err("Workspace changed before the warming report could be exported".into());
		}
		Ok(())
	}

	pub(crate) fn capture_warming_report(
		&self,
		expected_generation: u64,
		session_id: String,
	) -> Result<WarmingSessionReportDto, String> {
		self.check_warming_export(expected_generation)?;
		Ok(self.scene_warming.report(session_id, self.generation))
	}

	pub(crate) fn set_scene_warming_mode(
		&mut self,
		expected_generation: u64,
		mode: SceneWarmingMode,
	) -> Result<u64, String> {
		if self.generation != expected_generation {
			return Err("Workspace changed before the scene warming mode update".into());
		}
		self.scene_warming.set_mode(mode);
		Ok(self.scene_warming.mode_revision)
	}

	pub(crate) fn warm_scene(
		&mut self,
		expected_generation: u64,
		expected_revision: u64,
		expected_mode_revision: u64,
		scene: Option<SceneId>,
	) -> Result<(), String> {
		let session = self.backend.session();
		if self.generation != expected_generation || session.revision() != expected_revision {
			return Err("Workspace changed before the scene warming request".into());
		}
		if self.scene_warming.mode_revision != expected_mode_revision {
			return Err("Scene warming mode changed before the request".into());
		}
		let Some(scene) = scene else {
			self.scene_warming.cancel();
			return Ok(());
		};
		let mode = self.scene_warming.mode;
		if mode == SceneWarmingMode::Off
			|| session.is_playing()
			|| session.has_transaction()
			|| &scene == session.active_scene_id()
		{
			self.scene_warming.cancel();
			return Ok(());
		}
		let document = session.scene_document(&scene)?;
		let key = WarmKey {
			workspace: self.generation,
			revision: session.scene_revision(),
			asset_generation: self.scene_warming.asset_generation,
			mode,
			origin: session.active_scene_id().clone(),
			scene,
		};
		if self.scene_warming.worker.is_none() {
			self.scene_warming.worker = Some(WarmWorker::new()?);
		}
		if let Some(worker) = &self.scene_warming.worker {
			worker.request(key, document, self.asset_root.clone());
		}
		Ok(())
	}
}

pub(crate) fn publish_warming_report(
	report: &WarmingSessionReportDto,
	destination: &Path,
) -> Result<usize, String> {
	let mut bytes = serde_json::to_vec_pretty(report).map_err(|error| error.to_string())?;
	bytes.push(b'\n');
	if bytes.len() > MAX_REPORT_BYTES {
		return Err("The warming report exceeds its export limit".into());
	}
	runtime::scene_io::save_new_bytes(destination, &bytes)?;
	Ok(report.rows.len())
}

#[cfg(test)]
#[path = "../../tests/unit/editor_scene_warming.rs"]
mod tests;
