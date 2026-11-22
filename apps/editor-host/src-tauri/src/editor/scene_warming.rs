//! A bounded CPU worker for one selected inactive authored scene.
use std::sync::{
	atomic::{AtomicBool, Ordering},
	Arc, Condvar, Mutex,
};

use runtime::{scene_catalog::SceneId, scene_io::PreparedSceneReferences, SceneData};

use super::EditorWorkspace;

#[derive(Clone, Debug, Eq, PartialEq)]
struct WarmKey {
	workspace: u64,
	revision: u64,
	asset_generation: u64,
	origin: SceneId,
	scene: SceneId,
}

struct Job {
	key: WarmKey,
	document: Arc<SceneData>,
	cancel: Arc<AtomicBool>,
}

#[derive(Default)]
struct Pending {
	selected: Option<WarmKey>,
	cancel: Option<Arc<AtomicBool>>,
	queue: Option<Job>,
	ready: Option<PreparedSceneReferences>,
	closed: bool,
}

impl Pending {
	fn cancel(&mut self) {
		if let Some(cancel) = self.cancel.take() {
			cancel.store(true, Ordering::Relaxed);
		}
		self.selected = None;
		self.queue = None;
		self.ready = None;
	}
}

#[derive(Default)]
struct Mailbox {
	state: Mutex<Pending>,
	wake: Condvar,
}

struct WarmWorker(Arc<Mailbox>);

impl WarmWorker {
	fn new() -> Result<Self, String> {
		Self::spawn(|job| {
			PreparedSceneReferences::build(Arc::clone(&job.document), || {
				job.cancel.load(Ordering::Relaxed)
			})
				.ok()
				.flatten()
		})
	}

	fn spawn(
		prepare: impl Fn(&Job) -> Option<PreparedSceneReferences> + Send + 'static,
	) -> Result<Self, String> {
		let mailbox = Arc::new(Mailbox::default());
		let owned = Arc::clone(&mailbox);
		std::thread::Builder::new()
			.name("editor-scene-warm".into())
			.spawn(move || work(&owned, prepare))
			.map_err(|error| error.to_string())?;
		Ok(Self(mailbox))
	}

	fn request(&self, key: WarmKey, document: Arc<SceneData>) {
		let Ok(mut state) = self.0.state.lock() else {
			return;
		};
		if state.selected.as_ref() == Some(&key) {
			return;
		}
		state.cancel();
		let cancel = Arc::new(AtomicBool::new(false));
		state.selected = Some(key.clone());
		state.cancel = Some(Arc::clone(&cancel));
		state.queue = Some(Job {
			key,
			document,
			cancel,
		});
		self.0.wake.notify_one();
	}
}

impl Drop for WarmWorker {
	fn drop(&mut self) {
		if let Ok(mut state) = self.0.state.lock() {
			state.cancel();
			state.closed = true;
			self.0.wake.notify_one();
		}
	}
}

fn work(mailbox: &Mailbox, prepare: impl Fn(&Job) -> Option<PreparedSceneReferences>) {
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
	asset_generation: u64,
}

pub(crate) enum WarmClaim {
	Unrequested,
	Hit(PreparedSceneReferences),
	Miss,
	Stale,
}

impl WarmClaim {
	pub(crate) fn into_references(self) -> (Option<PreparedSceneReferences>, &'static str) {
		match self {
			Self::Unrequested => (None, "unrequested"),
			Self::Hit(references) => (Some(references), "hit"),
			Self::Miss => (None, "miss"),
			Self::Stale => (None, "stale"),
		}
	}
}

impl SceneWarming {
	pub(crate) fn cancel(&self) {
		if let Some(worker) = &self.worker {
			if let Ok(mut state) = worker.0.state.lock() {
				state.cancel();
			}
		}
	}

	pub(crate) fn invalidate_assets(&mut self) {
		self.asset_generation = self.asset_generation.wrapping_add(1);
		self.cancel();
	}

	pub(crate) fn is_requested(&self) -> bool {
		self.worker.as_ref().is_some_and(|worker| {
			worker
				.0
				.state
				.try_lock()
				.is_ok_and(|state| state.selected.is_some())
		})
	}

	pub(crate) fn claim(&self, workspace: u64, scene: &SceneId, document: &SceneData) -> WarmClaim {
		let Some(worker) = &self.worker else {
			return WarmClaim::Unrequested;
		};
		let Ok(mut state) = worker.0.state.lock() else {
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
			|| key.asset_generation != self.asset_generation
		{
			state.cancel();
			return WarmClaim::Stale;
		}
		let result = match state.ready.take() {
			Some(references) if references.matches(document) => WarmClaim::Hit(references),
			Some(_) => WarmClaim::Stale,
			None => WarmClaim::Miss,
		};
		state.cancel();
		result
	}
}

impl EditorWorkspace {
	pub(crate) fn warm_scene(
		&mut self,
		expected_generation: u64,
		expected_revision: u64,
		scene: Option<SceneId>,
	) -> Result<(), String> {
		let session = self.backend.session();
		if self.generation != expected_generation || session.revision() != expected_revision {
			return Err("Workspace changed before the scene warming request".into());
		}
		let Some(scene) = scene else {
			self.scene_warming.cancel();
			return Ok(());
		};
		if session.is_playing() || session.has_transaction() || &scene == session.active_scene_id()
		{
			self.scene_warming.cancel();
			return Ok(());
		}
		let document = session.scene_document(&scene)?;
		let key = WarmKey {
			workspace: self.generation,
			revision: session.scene_revision(),
			asset_generation: self.scene_warming.asset_generation,
			origin: session.active_scene_id().clone(),
			scene,
		};
		if self.scene_warming.worker.is_none() {
			self.scene_warming.worker = Some(WarmWorker::new()?);
		}
		if let Some(worker) = &self.scene_warming.worker {
			worker.request(key, document);
		}
		Ok(())
	}
}

#[cfg(test)]
#[path = "../../tests/unit/editor_scene_warming.rs"]
mod tests;
