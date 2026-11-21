use super::{
	args::{Options, validate_elapsed},
	report::{self, Summary},
	source::{Source, read_bounded, source},
};
use engine_core::{
	Bounds3, Engine, EngineBuilder,
	camera_input::CameraIntent,
	command::{EditorCommandDto, EditorCommandRequest},
};
use serde::{
	Deserialize,
	de::{self, SeqAccess, Visitor},
};
use std::{
	collections::HashSet,
	path::Path,
	sync::Arc,
	time::{Duration, Instant},
};

const MAX_REPLAY_BYTES: usize = 8 * 1024 * 1024;
const MAX_REPLAY_ACTIONS: usize = 10_000;
pub(super) const MAX_TICK_CALLS: u32 = 100_000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Replay {
	schema_version: u32,
	#[serde(deserialize_with = "deserialize_actions")]
	actions: Vec<ReplayAction>,
}
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum ReplayAction {
	RestartRun {},
	CameraInput(CameraIntent),
	Command {
		#[serde(default)]
		expected_revision: Option<u64>,
		command: Box<EditorCommandDto>,
	},
	Tick {
		#[serde(default = "one")]
		count: u32,
		#[serde(default)]
		elapsed_seconds: Option<f64>,
	},
	AssetMetadata {
		path: String,
		#[serde(default)]
		mesh_index: usize,
		min: [f32; 3],
		max: [f32; 3],
	},
}
fn one() -> u32 {
	1
}
fn deserialize_actions<'de, D: de::Deserializer<'de>>(
	deserializer: D,
) -> Result<Vec<ReplayAction>, D::Error> {
	struct Actions;
	impl<'de> Visitor<'de> for Actions {
		type Value = Vec<ReplayAction>;
		fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
			f.write_str("at most 10000 replay actions")
		}
		fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Self::Value, A::Error> {
			let mut actions =
				Vec::with_capacity(sequence.size_hint().unwrap_or(0).min(MAX_REPLAY_ACTIONS));
			while let Some(action) = sequence.next_element()? {
				if actions.len() == MAX_REPLAY_ACTIONS {
					return Err(de::Error::custom("Replay exceeds 10000 actions"));
				}
				actions.push(action);
			}
			Ok(actions)
		}
	}
	deserializer.deserialize_seq(Actions)
}

#[derive(Default)]
pub(super) struct Measurements {
	pub(super) commands: u64,
	pub(super) tick_calls: u32,
	pub(super) fixed_steps: u64,
	pub(super) dropped: f64,
	pub(super) tick_total: Duration,
	pub(super) tick_max: Duration,
	pub(super) fresh_ids: bool,
	pub(super) bounds: HashSet<(String, usize)>,
}

pub(super) fn run(options: Options) -> Result<Summary, String> {
	let started = Instant::now();
	let budget = Duration::from_millis(options.budget_ms.unwrap_or(30_000));
	let Source {
		project,
		parsed,
		documents,
		asset_root,
	} = source(&options)?;
	let source_schema = parsed.source_schema_version;
	let animation_root = asset_root.clone();
	let mut engine = EngineBuilder::new(parsed.data, project)
		.with_scenes(documents)
		.module_resolver(Arc::new(courtyard_gameplay::resolve))
		.animation_resolver(Arc::new(move |scene| {
			bridge::animation_asset::prepare_scene(&animation_root, scene)
		}))
		.build()
		.map_err(|e| e.to_string())?;
	let mut measures = Measurements::default();
	let steps = options
		.steps
		.unwrap_or(if options.replay.is_some() { 0 } else { 60 });
	if let Some(path) = &options.replay {
		let replay: Replay = serde_json::from_str(&read_bounded(path, MAX_REPLAY_BYTES)?)
			.map_err(|e| format!("Invalid replay: {e}"))?;
		if !matches!(replay.schema_version, 1 | 2) {
			return Err("Supported replay schema versions are 1 and 2".into());
		}
		replay.actions.iter().try_fold(steps, |total, action| {
			match action {
				ReplayAction::CameraInput(_) if replay.schema_version == 1 => {
					return Err("camera_input requires replay schema version 2".to_string());
				}
				ReplayAction::CameraInput(sample) => sample.validate()?,
				ReplayAction::Command { command, .. }
					if matches!(
						command.as_ref(),
						EditorCommandDto::SetPlayCameraInput { .. }
					) =>
				{
					return Err(
						"Use the version 2 camera_input action to address the current replay scene"
							.into(),
					);
				}
				_ => {}
			}
			let count = if let ReplayAction::Tick { count, .. } = action {
				*count
			} else {
				0
			};
			total
				.checked_add(count)
				.filter(|value| *value <= MAX_TICK_CALLS)
				.ok_or_else(|| "Replay and --steps exceed 100000 tick calls".to_string())
		})?;
		for action in replay.actions {
			check_budget(started, budget)?;
			match action {
				ReplayAction::CameraInput(sample) => {
					let owner = engine
						.camera_input_owner()
						.ok_or("camera_input requires an unpaused orbit camera in Play")?;
					engine.submit_camera_input(&owner.run_id, owner.scene_generation, sample)?;
					measures.commands += 1;
				}
				ReplayAction::RestartRun {} => {
					let status = engine
						.play_status()
						.ok_or("Restart Run requires Play mode")?;
					let (prepared, _) = engine
						.prepare_restart_play(&status.run_id, engine.session().revision())
						.map_err(|error| format!("Cannot prepare Restart Run: {error}"))?;
					engine
						.commit_play_scene(prepared)
						.map_err(|error| format!("Cannot install Restart Run: {error}"))?;
					measures.commands += 1;
				}
				ReplayAction::Command {
					expected_revision,
					command,
				} => {
					measures.fresh_ids |= matches!(
						command.as_ref(),
						EditorCommandDto::Spawn { .. }
							| EditorCommandDto::DuplicateSelection { .. }
					);
					let request = EditorCommandRequest {
						expected_revision: expected_revision.unwrap_or(engine.session().revision()),
						command: *command,
					};
					engine
						.execute(request)
						.map_err(|e| format!("Replay command failed ({:?}): {e}", e.code))?;
					measures.commands += 1;
				}
				ReplayAction::Tick {
					count,
					elapsed_seconds,
				} => {
					let elapsed = elapsed_seconds
						.or(options.dt)
						.unwrap_or(1.0 / engine.session().project().fixed_hz as f64);
					validate_elapsed(elapsed)?;
					advance(
						&mut engine,
						&asset_root,
						count,
						elapsed,
						&mut measures,
						started,
						budget,
					)?;
				}
				ReplayAction::AssetMetadata {
					path,
					mesh_index,
					min,
					max,
				} => {
					let bounds = Bounds3::new(min, max)?;
					engine
						.set_model_bounds(path.clone(), mesh_index, bounds)
						.map_err(|e| e.to_string())?;
					measures.bounds.insert((path, mesh_index));
				}
			}
		}
	}
	if steps > 0 {
		if !engine.session().is_playing() {
			engine
				.execute(EditorCommandRequest {
					expected_revision: engine.session().revision(),
					command: EditorCommandDto::StartPlay,
				})
				.map_err(|e| e.to_string())?;
			measures.commands += 1;
		}
		let elapsed = options
			.dt
			.unwrap_or(1.0 / engine.session().project().fixed_hz as f64);
		advance(
			&mut engine,
			&asset_root,
			steps,
			elapsed,
			&mut measures,
			started,
			budget,
		)?;
	}
	check_budget(started, budget)?;
	report::finish(&engine, &options, &measures, source_schema, started, budget)
}

fn advance(
	engine: &mut Engine,
	asset_root: &Path,
	count: u32,
	elapsed: f64,
	measures: &mut Measurements,
	started: Instant,
	budget: Duration,
) -> Result<(), String> {
	for _ in 0..count {
		check_budget(started, budget)?;
		let tick_start = Instant::now();
		let mut remaining_elapsed = elapsed;
		loop {
			let outcome = engine.tick(remaining_elapsed).map_err(|e| e.to_string())?;
			if let Some(destination) = &outcome.requested_scene {
				check_budget(started, budget)?;
				let (prepared, _) = engine.prepare_pending_play_scene().map_err(|error| {
					format!("Cannot prepare requested scene {destination:?}: {error}")
				})?;
				engine.commit_play_scene(prepared).map_err(|error| {
					format!("Cannot install requested scene {destination:?}: {error}")
				})?;
			}
			if !engine.session().is_paused() {
				for request in engine.pending_runtime_spawns() {
					check_budget(started, budget)?;
					let result = (|| {
						let (prefab, fingerprint) =
							bridge::runtime_prefab::load(asset_root, &request.prefab_path)?;
						let prepared = engine.prepare_runtime_spawn(
							&prefab,
							&fingerprint,
							request.position,
							Some(request.id),
						)?;
						engine
							.commit_runtime_spawn(prepared)
							.map_err(|error| error.to_string())
					})();
					if let Err(error) = result {
						engine
							.reject_runtime_spawn(request.id, &error)
							.map_err(|error| error.to_string())?;
					}
				}
			}
			measures.fixed_steps += outcome.steps as u64;
			measures.dropped += outcome.dropped_seconds;
			if outcome.pending_steps == 0 {
				break;
			}
			remaining_elapsed = 0.0;
		}
		let time = tick_start.elapsed();
		measures.tick_total += time;
		measures.tick_max = measures.tick_max.max(time);
		measures.tick_calls += 1;
	}
	Ok(())
}
pub(super) fn check_budget(started: Instant, budget: Duration) -> Result<(), String> {
	if started.elapsed() > budget {
		Err("Headless wall-time budget exceeded between actions".into())
	} else {
		Ok(())
	}
}
