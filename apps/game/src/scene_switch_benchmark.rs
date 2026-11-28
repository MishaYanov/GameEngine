//! Companion evidence for native smoke-test scene switches.
use std::time::Duration;

use runtime::{
	scene_catalog::SceneId,
	scene_load_timing::{SceneLoadBenchmark, SceneLoadResources, SceneLoadTiming},
};
use serde::Serialize;

#[derive(Serialize)]
pub(super) struct SceneSwitchSample<'a> {
	schema_version: u8,
	request_id: Option<String>,
	index: u32,
	source: &'a str,
	destination: &'a str,
	reason: &'static str,
	active_frame_us: [u64; 3],
	frame_gap_us: Option<u64>,
	before: SceneLoadResources,
	settled: Option<SceneLoadResources>,
	preparation: Option<SceneLoadBenchmark>,
}

impl<'a> SceneSwitchSample<'a> {
	pub(super) fn new(
		index: u32,
		source: &'a SceneId,
		destination: &'a SceneId,
		before: SceneLoadResources,
		active_frame_us: [u64; 3],
	) -> Self {
		Self {
			schema_version: 1,
			request_id: None,
			index,
			source: source.as_str(),
			destination: destination.as_str(),
			reason: "smoke_cycle",
			active_frame_us,
			frame_gap_us: None,
			before,
			settled: None,
			preparation: None,
		}
	}

	pub(super) fn observe_install(&mut self, timing: &SceneLoadTiming) {
		self.request_id = Some(timing.id().to_string());
		self.preparation = timing.benchmark();
	}

	pub(super) fn observe_present(&mut self, frame_gap_us: u64, settled: SceneLoadResources) {
		self.frame_gap_us = Some(frame_gap_us);
		self.settled = Some(settled);
	}

	pub(super) fn emit(self) {
		if let Ok(message) = serde_json::to_string(&self) {
			engine_logger::info!(target: "scene_switch", "{message}");
		}
	}
}

pub(super) fn micros(span: Duration) -> u64 {
	span.as_micros().min(u64::MAX as u128) as u64
}
