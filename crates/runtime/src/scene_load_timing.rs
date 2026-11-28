//! Monotonic, one-shot request measurements shared by the native scene hosts.
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::assets::AssetManager;

pub const LOG_TARGET: &str = "scene_load";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
	Presented,
	Cancelled,
	Failed,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
	Startup,
	Authored,
	Play,
	Warmup,
}

pub struct SceneLoadTiming {
	id: uuid::Uuid,
	host: &'static str,
	scope: Scope,
	started: Instant,
	cpu_ready: Option<Instant>,
	gpu_started: Option<Instant>,
	installed: Option<Instant>,
	benchmark: Option<SceneLoadBenchmark>,
	terminal: bool,
}

/// Non-overlapping wall-clock spans recorded only during a switch benchmark.
#[derive(Clone, Copy)]
pub enum BenchmarkPhase {
	Read,
	Simulation,
	Drain,
	Install,
}

#[derive(Clone, Copy, Default, Serialize)]
pub struct SceneLoadBenchmark {
	read_us: Option<u64>,
	simulation_us: Option<u64>,
	native_assembly_us: Option<u64>,
	drain_us: Option<u64>,
	install_us: Option<u64>,
	assets: Option<AssetLoadBenchmark>,
	overlap: Option<SceneLoadResources>,
}

/// Assembly subspans for static GLB model cache lookups and uploads.
#[derive(Clone, Copy, Default, Serialize)]
pub(crate) struct AssetLoadBenchmark {
	pub(crate) cache_hits: usize,
	pub(crate) cache_misses: usize,
	pub(crate) static_import_us: u64,
	pub(crate) static_upload_us: u64,
}

#[derive(Clone, Copy, Serialize)]
pub struct SceneLoadResources {
	models: usize,
	cpu_retained_source_bytes: usize,
	logical_gpu_bytes: usize,
	logical_gpu_resources: usize,
	tracked_gpu: Option<TrackedGpuResources>,
}

impl SceneLoadResources {
	pub fn capture(assets: &AssetManager, renderer: &renderer::WindowRenderer) -> Self {
		Self {
			models: assets.loaded_model_count(),
			cpu_retained_source_bytes: assets.retained_source_bytes(),
			logical_gpu_bytes: assets.gpu_logical_bytes(),
			logical_gpu_resources: assets.gpu_resource_count(),
			tracked_gpu: renderer
				.resource_metrics()
				.map(|metrics| TrackedGpuResources {
					allocations: metrics.totals.live_allocations,
					bytes: metrics.totals.allocation_bytes,
					peak_bytes: metrics.totals.peak_allocation_bytes,
				}),
		}
	}
}

#[derive(Clone, Copy, Serialize)]
struct TrackedGpuResources {
	allocations: u64,
	bytes: u64,
	// Includes transient staging observed by the tracker over the process lifetime.
	peak_bytes: u64,
}

#[derive(Serialize)]
struct TimingRecord {
	schema_version: u8,
	request_id: String,
	host: &'static str,
	scope: Scope,
	outcome: Outcome,
	elapsed_us: u64,
	cpu_us: Option<u64>,
	queue_us: Option<u64>,
	gpu_us: Option<u64>,
	presentation_us: Option<u64>,
	first_present_us: Option<u64>,
}

impl SceneLoadTiming {
	pub fn new(host: &'static str, scope: Scope, started: Instant) -> Self {
		Self {
			id: uuid::Uuid::new_v4(),
			host,
			scope,
			started,
			cpu_ready: None,
			gpu_started: None,
			installed: None,
			benchmark: None,
			terminal: false,
		}
	}

	pub fn id(&self) -> uuid::Uuid {
		self.id
	}

	pub fn cpu_ready(&mut self) {
		self.cpu_ready_at(Instant::now());
	}

	pub fn cpu_ready_at(&mut self, at: Instant) {
		self.cpu_ready.get_or_insert(at);
	}

	pub fn gpu_started(&mut self) {
		self.gpu_started.get_or_insert_with(Instant::now);
	}

	pub fn installed(&mut self) {
		self.installed.get_or_insert_with(Instant::now);
	}

	/// Keep diagnostics separate from the established scene_load timing record.
	pub fn enable_benchmark(&mut self) {
		self.benchmark = Some(SceneLoadBenchmark::default());
	}

	pub fn is_benchmark_enabled(&self) -> bool {
		self.benchmark.is_some()
	}

	pub fn benchmark(&self) -> Option<SceneLoadBenchmark> {
		self.benchmark
	}

	pub fn measure<T>(&mut self, phase: BenchmarkPhase, work: impl FnOnce() -> T) -> T {
		let Some(benchmark) = &mut self.benchmark else {
			return work();
		};
		let started = Instant::now();
		let result = work();
		let elapsed_us = micros(started.elapsed());
		let slot = match phase {
			BenchmarkPhase::Read => &mut benchmark.read_us,
			BenchmarkPhase::Simulation => &mut benchmark.simulation_us,
			BenchmarkPhase::Drain => &mut benchmark.drain_us,
			BenchmarkPhase::Install => &mut benchmark.install_us,
		};
		*slot = Some(elapsed_us);
		result
	}

	pub fn measure_native_assembly<T>(
		&mut self,
		assets: &mut AssetManager,
		renderer: &renderer::WindowRenderer,
		assemble: impl FnOnce(&mut AssetManager) -> T,
	) -> T {
		let Some(benchmark) = &mut self.benchmark else {
			return assemble(assets);
		};
		let started = Instant::now();
		assets.start_scene_load_benchmark();
		let result = assemble(assets);
		benchmark.native_assembly_us = Some(micros(started.elapsed()));
		benchmark.assets = assets.finish_scene_load_benchmark();
		benchmark.overlap = Some(SceneLoadResources::capture(assets, renderer));
		result
	}

	pub fn finish(self, outcome: Outcome) {
		self.finish_at(outcome, Instant::now());
	}

	pub fn finish_at(mut self, outcome: Outcome, when: Instant) {
		self.emit(outcome, when);
	}

	fn record(&self, outcome: Outcome, now: Instant) -> TimingRecord {
		let span = |from: Instant, to: Instant| {
			to.saturating_duration_since(from)
				.as_micros()
				.min(u64::MAX as u128) as u64
		};
		let presented = outcome == Outcome::Presented && self.installed.is_some();
		let elapsed_us = span(self.started, now);
		TimingRecord {
			schema_version: 1,
			request_id: self.id.to_string(),
			host: self.host,
			scope: self.scope,
			outcome,
			elapsed_us,
			cpu_us: self.cpu_ready.map(|end| span(self.started, end)),
			queue_us: self
				.cpu_ready
				.zip(self.gpu_started)
				.map(|(start, end)| span(start, end)),
			gpu_us: self
				.gpu_started
				.zip(self.installed)
				.map(|(start, end)| span(start, end)),
			presentation_us: self
				.installed
				.filter(|_| presented)
				.map(|start| span(start, now)),
			first_present_us: presented.then_some(elapsed_us),
		}
	}

	fn emit(&mut self, outcome: Outcome, when: Instant) {
		self.terminal = true;
		if let Ok(message) = serde_json::to_string(&self.record(outcome, when)) {
			engine_logger::info!(target: LOG_TARGET, "{message}");
		}
	}
}

impl Drop for SceneLoadTiming {
	fn drop(&mut self) {
		if !self.terminal {
			self.emit(Outcome::Cancelled, Instant::now());
		}
	}
}

pub(crate) fn micros(span: Duration) -> u64 {
	span.as_micros().min(u64::MAX as u128) as u64
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::time::Duration;

	#[test]
	fn phase_lengths_are_request_relative_and_cancelled_never_presents() {
		let started = Instant::now();
		let mut timing = SceneLoadTiming::new("player", Scope::Play, started);
		timing.cpu_ready = Some(started + Duration::from_micros(10));
		timing.gpu_started = Some(started + Duration::from_micros(20));
		timing.installed = Some(started + Duration::from_micros(35));
		let done = started + Duration::from_micros(50);
		let record = timing.record(Outcome::Presented, done);
		assert_eq!(record.cpu_us, Some(10));
		assert_eq!(record.queue_us, Some(10));
		assert_eq!(record.gpu_us, Some(15));
		assert_eq!(record.presentation_us, Some(15));
		assert_eq!(record.first_present_us, Some(50));
		let record = timing.record(Outcome::Cancelled, done);
		assert_eq!(record.first_present_us, None);
		assert_eq!(record.presentation_us, None);
	}
}
