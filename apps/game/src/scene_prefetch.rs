//! Installed Player ownership for one speculative authored successor.

use std::{
	path::{Path, PathBuf},
	time::Duration,
};

use courtyard_gameplay::{CampaignDefinition, settings::InteractionSettings};
use runtime::{
	project::ProjectData,
	scene_catalog::SceneId,
	scene_data::ParsedScene,
	scene_load_timing::SceneLoadTiming,
	scene_prefetch::{PrefetchKey, PrefetchOutcome, ScenePrefetch},
};

const TARGET: &str = "scene_prefetch";

pub(super) struct PlayerPrefetch {
	worker: ScenePrefetch,
	root: PathBuf,
	route: CampaignDefinition,
	generation: u64,
	scheduled: Option<SceneId>,
}

impl PlayerPrefetch {
	pub fn new(project: &ProjectData, root: &Path) -> Result<Option<Self>, String> {
		let Some(route) =
			InteractionSettings::from_project(project)?.and_then(|settings| settings.campaign)
		else {
			return Ok(None);
		};
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
			route,
			generation: 0,
			scheduled: None,
		}))
	}

	pub fn prime(&mut self, current: &SceneId, project: &ProjectData) {
		if self.scheduled.as_ref() == Some(current) {
			return;
		}
		let Some(successor) = self.route.successor(current) else {
			return;
		};
		let path = project.scene_path(successor).map(PathBuf::from);
		// Retry only a contended mailbox. The real loader diagnoses bad sources.
		if let Ok(path) = path
			&& !self.worker.queue(self.key(successor), path)
		{
			return;
		}
		self.scheduled = Some(current.clone());
	}

	pub fn load(
		&self,
		scene: &SceneId,
		path: &Path,
	) -> Result<(ParsedScene, PrefetchOutcome, usize), String> {
		if let Some(prefetched) = self.worker.take(&self.key(scene)) {
			return prefetched.load_or_refresh(path);
		}
		Ok((
			runtime::scene_io::load_data(path)?,
			PrefetchOutcome::Miss,
			0,
		))
	}

	pub fn invalidate(&mut self) {
		self.worker.cancel();
		self.generation = self.generation.wrapping_add(1);
		self.scheduled = None;
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
