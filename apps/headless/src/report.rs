use super::{
	args::Options,
	replay::{Measurements, check_budget},
};
use engine_core::{
	Engine,
	command::PlayStatusDto,
	project::{MAX_PROJECT_BYTES, ProjectData},
	scene_catalog::SceneId,
	scene_data::{MAX_SCENE_BYTES, SceneData},
};
use serde::Serialize;
use std::{
	collections::HashSet,
	fs::OpenOptions,
	io::{self, Write},
	path::Path,
	time::{Duration, Instant},
};

const MAX_CAMERA_REPLAY_BYTES: usize = 64 * 1024;
const MAX_STATE_HASH_BYTES: usize = MAX_SCENE_BYTES + MAX_PROJECT_BYTES + 128;

#[derive(Serialize)]
struct CameraPose {
	position: [f32; 3],
	yaw: f32,
	pitch: f32,
}
#[derive(Serialize)]
pub(super) struct Summary {
	#[serde(skip_serializing_if = "Option::is_none")]
	orbit_camera_pose: Option<CameraPose>,
	camera_runtime: Option<engine_core::camera_replay::CameraReplayProjection>,
	camera_runtime_hash_algorithm: &'static str,
	camera_runtime_hash_scope: &'static str,
	camera_runtime_hash: String,
	camera_runtime_hash_bytes: usize,
	runtime: Option<engine_core::runtime_lifecycle::RuntimeInspectionDto>,
	schema_version: u32,
	project: String,
	mode: &'static str,
	active_scene_id: SceneId,
	#[serde(skip_serializing_if = "Option::is_none")]
	play_scene_id: Option<SceneId>,
	#[serde(skip_serializing_if = "Option::is_none")]
	play_status: Option<PlayStatusDto>,
	revision: u64,
	scene_revision: u64,
	change_cursor: u64,
	entities: usize,
	asset_references: usize,
	model_bounds_loaded: usize,
	commands: u64,
	tick_calls: u32,
	fixed_steps: u64,
	simulation_tick: u64,
	dropped_seconds: f64,
	matrix_updates: u64,
	collision_rebuilds: u64,
	collision_updates: u64,
	collision_queries: u64,
	retained_event_bytes: usize,
	source_scene_schema: u32,
	state_hash_algorithm: &'static str,
	state_hash_scope: &'static str,
	state_hash: String,
	state_hash_bytes: usize,
	native_components_hash_algorithm: &'static str,
	native_components_hash_scope: &'static str,
	native_components_hash: String,
	native_components_hash_bytes: usize,
	native_components: serde_json::Value,
	fresh_entity_ids_generated: bool,
	elapsed_ms: f64,
	tick_total_ms: f64,
	tick_max_ms: f64,
}

pub(super) fn finish(
	engine: &Engine,
	options: &Options,
	measures: &Measurements,
	source_schema: u32,
	started: Instant,
	budget: Duration,
) -> Result<Summary, String> {
	let final_document = engine.session().render_document();
	let mut hash = StateHash::new(MAX_STATE_HASH_BYTES);
	#[derive(Serialize)]
	struct State<'a> {
		project: &'a ProjectData,
		scene: &'a SceneData,
	}
	serde_json::to_writer(
		&mut hash,
		&State {
			project: engine.session().project(),
			scene: &final_document,
		},
	)
	.map_err(|e| e.to_string())?;
	check_budget(started, budget)?;
	let assets: HashSet<_> = final_document
		.entities
		.iter()
		.filter(|e| e.renderable)
		.map(|e| (&e.model_path, e.mesh_index.unwrap_or(0)))
		.collect();
	let stats = engine.session().simulation_stats();
	let native_components = engine.session().native_replay_state();
	let mut native_hash = StateHash::new(MAX_STATE_HASH_BYTES);
	serde_json::to_writer(&mut native_hash, &native_components)
		.map_err(|error| error.to_string())?;
	let camera_runtime = engine.camera_replay_state()?;
	let mut camera_hash = StateHash::new(MAX_CAMERA_REPLAY_BYTES);
	serde_json::to_writer(&mut camera_hash, &camera_runtime)
		.map_err(|error| format!("Cannot hash camera replay: {error}"))?;
	let summary = Summary {
		camera_runtime,
		camera_runtime_hash_algorithm: "fnv1a64",
		camera_runtime_hash_scope: "camera_runtime_v1",
		camera_runtime_hash: format!("{:016x}", camera_hash.value),
		camera_runtime_hash_bytes: camera_hash.bytes,
		orbit_camera_pose: engine.session().orbit_state().map(|_| {
			let camera = engine.session().render_camera();
			CameraPose {
				position: camera.position,
				yaw: camera.yaw,
				pitch: camera.pitch,
			}
		}),
		runtime: engine.runtime_inspection(0, 128).ok(),
		schema_version: 1,
		project: engine.session().project().name.clone(),
		mode: if engine.session().is_playing() {
			"play"
		} else {
			"edit"
		},
		active_scene_id: engine.session().active_scene_id().clone(),
		play_scene_id: engine.session().play_scene_id().cloned(),
		play_status: engine.play_status(),
		revision: engine.session().revision(),
		scene_revision: engine.session().scene_revision(),
		change_cursor: engine.change_cursor(),
		entities: final_document.entities.len(),
		asset_references: assets.len(),
		model_bounds_loaded: measures.bounds.len(),
		commands: measures.commands,
		tick_calls: measures.tick_calls,
		fixed_steps: measures.fixed_steps,
		simulation_tick: stats.map_or(0, |s| s.0),
		dropped_seconds: measures.dropped,
		matrix_updates: stats.map_or(0, |s| s.1),
		collision_rebuilds: stats.map_or(0, |s| s.2.rebuilds),
		collision_updates: stats.map_or(0, |s| s.2.updated_colliders),
		collision_queries: stats.map_or(0, |s| s.2.queries),
		retained_event_bytes: engine.retained_event_bytes(),
		source_scene_schema: source_schema,
		state_hash_algorithm: "fnv1a64",
		state_hash_scope: "project_and_render_scene",
		state_hash: format!("{:016x}", hash.value),
		state_hash_bytes: hash.bytes,
		native_components_hash_algorithm: "fnv1a64",
		native_components_hash_scope: "native_components_v1",
		native_components_hash: format!("{:016x}", native_hash.value),
		native_components_hash_bytes: native_hash.bytes,
		native_components,
		fresh_entity_ids_generated: measures.fresh_ids,
		elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
		tick_total_ms: measures.tick_total.as_secs_f64() * 1000.0,
		tick_max_ms: measures.tick_max.as_secs_f64() * 1000.0,
	};
	if let Some(path) = &options.state_output {
		write_new(path, final_document.to_json()?.as_bytes())?;
	}
	if let Some(path) = &options.output {
		write_new(
			path,
			&serde_json::to_vec_pretty(&summary).map_err(|e| e.to_string())?,
		)?;
	}
	Ok(summary)
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
	let mut file = OpenOptions::new()
		.write(true)
		.create_new(true)
		.open(path)
		.map_err(|e| format!("Cannot create output '{}': {e}", path.display()))?;
	file.write_all(bytes)
		.and_then(|()| file.write_all(b"\n"))
		.and_then(|()| file.sync_all())
		.map_err(|e| {
			format!(
				"Cannot finish output '{}': {e}; incomplete file retained",
				path.display()
			)
		})
}
struct StateHash {
	value: u64,
	bytes: usize,
	max_bytes: usize,
}
impl StateHash {
	fn new(max_bytes: usize) -> Self {
		Self {
			value: 0xcbf29ce484222325,
			bytes: 0,
			max_bytes,
		}
	}
}
impl Write for StateHash {
	fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
		self.bytes = self
			.bytes
			.checked_add(bytes.len())
			.filter(|value| *value <= self.max_bytes)
			.ok_or_else(|| io::Error::other("State hash input exceeds its byte budget"))?;
		for byte in bytes {
			self.value ^= *byte as u64;
			self.value = self.value.wrapping_mul(0x100000001b3);
		}
		Ok(bytes.len())
	}
	fn flush(&mut self) -> io::Result<()> {
		Ok(())
	}
}

#[cfg(test)]
#[path = "../tests/unit/report.rs"]
mod tests;
