//! Installed Player ownership for one speculative authored successor.

use std::{
	path::{Path, PathBuf},
	time::Duration,
};

use gameplay_catalog::cinder_oath::intro_route;
use runtime::{
	project::{ProjectData, SceneRoute, SceneRouteId},
	scene_catalog::SceneId,
	scene_data::ParsedScene,
	scene_load_timing::SceneLoadTiming,
	scene_prefetch::{ConfirmedScene, PrefetchKey, PrefetchOutcome, ScenePrefetch},
};

const TARGET: &str = "scene_prefetch";

pub(super) struct PlayerPrefetch {
	worker: ScenePrefetch,
	root: PathBuf,
	legacy_route: Option<(SceneId, SceneId)>,
	approach: Option<SceneRouteId>,
	generation: u64,
	scheduled: bool,
}

impl PlayerPrefetch {
	pub fn new(
		project: &ProjectData,
		root: &Path,
		legacy_enabled: bool,
	) -> Result<Option<Self>, String> {
		let legacy_route = if legacy_enabled {
			intro_route(project)?
		} else {
			None
		};
		if legacy_route.is_none() && project.scene_routes.is_empty() {
			return Ok(None);
		}
		let worker = match ScenePrefetch::new() {
			Ok(worker) => worker,
			Err(_) => {
				engine_logger::warn!(target: TARGET, "Cannot start scene prefetch worker");
				return Ok(None);
			}
		};
		Ok(Some(Self {
			worker,
			root: root.canonicalize().map_err(|error| error.to_string())?,
			legacy_route,
			approach: None,
			generation: 0,
			scheduled: false,
		}))
	}

	pub fn prime(&mut self, current: &SceneId, project: &ProjectData) {
		let Some((intro, arena)) = &self.legacy_route else {
			return;
		};
		if self.scheduled || current != intro || self.approach.is_some() {
			return;
		}
		let path = project.scene_path(arena).map(PathBuf::from);
		// Retry only a contended mailbox. The real loader diagnoses bad sources.
		if let Ok(path) = path
			&& !self.worker.queue(self.key(arena), path)
		{
			return;
		}
		self.scheduled = true;
	}

	pub fn speculative_route(&self) -> Option<&SceneRouteId> {
		self.approach.as_ref()
	}

	pub fn set_approach(&mut self, route: Option<&SceneRoute>, project: &ProjectData) {
		if self.approach.as_ref() == route.map(|route| &route.id) {
			return;
		}
		let Some(route) = route else {
			self.worker.cancel();
			self.approach = None;
			return;
		};
		if let Ok(path) = project.scene_path(&route.to)
			&& self.worker.queue(self.key(&route.to), PathBuf::from(path))
		{
			self.approach = Some(route.id.clone());
		}
	}

	pub fn confirm(&self, scene: &SceneId, path: &str) -> bool {
		self.worker.confirm(self.key(scene), PathBuf::from(path))
	}

	pub fn take_confirmed(&self, scene: &SceneId) -> Option<Result<ConfirmedScene, String>> {
		self.worker.take_confirmed(&self.key(scene))
	}

	pub fn load(
		&self,
		scene: &SceneId,
		relative: &Path,
	) -> Result<(PathBuf, ParsedScene, PrefetchOutcome, usize), String> {
		if let Some(prefetched) = self.worker.take(&self.key(scene)) {
			return prefetched.load_relative(relative);
		}
		let (path, parsed) =
			runtime::scene_io::load_confined_data_from_canonical_root(&self.root, relative)?;
		Ok((path, parsed, PrefetchOutcome::Miss, 0))
	}

	pub fn invalidate(&mut self) {
		self.worker.cancel();
		self.generation = self.generation.wrapping_add(1);
		self.scheduled = false;
		self.approach = None;
	}

	fn key(&self, scene: &SceneId) -> PrefetchKey {
		PrefetchKey {
			asset_root: self.root.clone(),
			scene: scene.clone(),
			source_generation: self.generation,
		}
	}
}

pub(super) fn record_load(
	timing: &SceneLoadTiming,
	scene: &SceneId,
	outcome: PrefetchOutcome,
	retained_estimate_bytes: usize,
) {
	log(serde_json::json!({
		"schema_version": 1,
		"kind": "load",
		"request_id": timing.id().to_string(),
		"scene_id": scene.as_str(),
		"outcome": outcome,
		"retained_estimate_bytes": retained_estimate_bytes,
	}));
}

pub(super) fn record_active_frame(sample: u32, frame: u32, enabled: bool, elapsed: Duration) {
	log(serde_json::json!({
		"schema_version": 1,
		"kind": "active_frame",
		"sample": sample,
		"frame": frame,
		"enabled": enabled,
		"elapsed_us": elapsed.as_micros().min(u64::MAX as u128) as u64,
	}));
}

fn log(message: serde_json::Value) {
	engine_logger::info!(target: TARGET, "{message}");
}
