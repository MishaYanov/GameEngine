use super::{
	args::{Options, validate_elapsed},
	report::{self, ReplayIdentity, ReplayMarker, ReplayReport, Summary},
	source::{Source, read_bounded, source},
};
use engine_core::{
	Bounds3, Engine, EngineBuilder,
	camera_input::CameraIntent,
	command::{EditorCommandDto, EditorCommandRequest},
	input::GamepadSample,
};
use serde::{
	Deserialize,
	de::{self, SeqAccess, Visitor},
};
use sha2::{Digest, Sha256};
use std::{
	collections::HashSet,
	path::Path,
	sync::Arc,
	time::{Duration, Instant},
};

const MAX_REPLAY_BYTES: usize = 8 * 1024 * 1024;
const MAX_REPLAY_ACTIONS: usize = 10_000;
const MAX_REPLAY_KEYS: usize = 16;
const MAX_REPLAY_MARKERS: usize = 128;
const MAX_MARKER_LABEL_BYTES: usize = 64;
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
	Mark {
		label: String,
	},
	CameraInput(CameraIntent),
	KeyboardInput {
		#[serde(deserialize_with = "deserialize_keyboard_keys")]
		keys: HashSet<String>,
	},
	ControllerInput {
		left_x: i16,
		left_y: i16,
		jump: bool,
		interact: bool,
	},
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

impl Replay {
	fn validate(&self, extra_steps: u32) -> Result<(), String> {
		if !matches!(self.schema_version, 1..=5) {
			return Err("Supported replay schema versions are 1 through 5".into());
		}
		let mut labels = HashSet::new();
		let mut total = extra_steps;
		for action in &self.actions {
			match action {
				ReplayAction::Mark { label } => {
					if self.schema_version < 5 {
						return Err("mark requires replay schema version 5".into());
					}
					if label.is_empty()
						|| label.len() > MAX_MARKER_LABEL_BYTES
						|| !label
							.bytes()
							.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
					{
						return Err("mark requires a label of 1 through 64 ASCII letters, digits, underscores or hyphens".into());
					}
					if !labels.insert(label.as_str()) {
						return Err(format!("Duplicate replay mark {label:?}"));
					}
					if labels.len() > MAX_REPLAY_MARKERS {
						return Err("Replay exceeds 128 marks".into());
					}
				}
				ReplayAction::CameraInput(_) if self.schema_version < 2 => {
					return Err("camera_input requires replay schema version 2".into());
				}
				ReplayAction::CameraInput(sample) => sample.validate()?,
				ReplayAction::KeyboardInput { .. } if self.schema_version < 3 => {
					return Err("keyboard_input requires replay schema version 3".into());
				}
				ReplayAction::ControllerInput { .. } if self.schema_version < 4 => {
					return Err("controller_input requires replay schema version 4".into());
				}
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
				ReplayAction::Tick { count, .. } => {
					total = total
						.checked_add(*count)
						.filter(|value| *value <= MAX_TICK_CALLS)
						.ok_or("Replay and --steps exceed 100000 tick calls")?;
				}
				_ => {}
			}
		}
		Ok(())
	}
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

fn deserialize_keyboard_keys<'de, D: de::Deserializer<'de>>(
	deserializer: D,
) -> Result<HashSet<String>, D::Error> {
	struct KeyboardKeys;
	impl<'de> Visitor<'de> for KeyboardKeys {
		type Value = HashSet<String>;

		fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
			f.write_str("a list of held physical key codes")
		}

		fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Self::Value, A::Error> {
			let mut keys = HashSet::new();
			while let Some(key) = sequence.next_element::<String>()? {
				if keys.len() == MAX_REPLAY_KEYS {
					return Err(de::Error::custom("keyboard_input accepts at most 16 keys"));
				}
				let mut bytes = key.bytes();
				if key.len() > 32
					|| !bytes.next().is_some_and(|byte| byte.is_ascii_alphabetic())
					|| !bytes.all(|byte| byte.is_ascii_alphanumeric())
					|| !keys.insert(key)
				{
					return Err(de::Error::custom(
						"keyboard_input requires distinct physical key codes such as KeyE",
					));
				}
			}
			Ok(keys)
		}
	}
	deserializer.deserialize_seq(KeyboardKeys)
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

#[derive(Default)]
struct ReplayControls {
	keyboard: HashSet<String>,
	gamepad: GamepadSample,
}

impl ReplayControls {
	fn submit(&self, engine: &mut Engine) -> Result<(), String> {
		engine.set_game_controls_from_codes(&self.keyboard, self.gamepad)
	}
}

pub(super) fn run(options: Options) -> Result<Summary, String> {
	let started = Instant::now();
	let budget = Duration::from_millis(options.budget_ms.unwrap_or(30_000));
	let Source {
		project,
		parsed,
		documents,
		asset_root,
		project_manifest_sha256,
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
	let mut replay_report = ReplayReport::default();
	let steps = options
		.steps
		.unwrap_or(if options.replay.is_some() { 0 } else { 60 });
	if let Some(path) = &options.replay {
		let replay_text = read_bounded(path, MAX_REPLAY_BYTES)?;
		let replay: Replay =
			serde_json::from_str(&replay_text).map_err(|e| format!("Invalid replay: {e}"))?;
		replay.validate(steps)?;
		if replay.schema_version >= 4 && options.dt.is_none() && steps == 0 {
			replay_report.identity =
				project_manifest_sha256.map(|project_manifest_sha256| ReplayIdentity {
					replay_sha256: format!("{:x}", Sha256::digest(replay_text.as_bytes())),
					project_manifest_sha256,
				});
		}
		let mut controls = ReplayControls::default();
		for action in replay.actions {
			check_budget(started, budget)?;
			match action {
				ReplayAction::Mark { label } => replay_report
					.markers
					.push(ReplayMarker::capture(&engine, label)?),
				ReplayAction::CameraInput(sample) => {
					let owner = engine
						.camera_input_owner()
						.ok_or("camera_input requires an unpaused orbit camera in Play")?;
					engine.submit_camera_input(&owner.run_id, owner.scene_generation, sample)?;
					measures.commands += 1;
				}
				ReplayAction::KeyboardInput { keys } => {
					controls.keyboard = keys;
					controls.submit(&mut engine)?;
					measures.commands += 1;
				}
				ReplayAction::ControllerInput {
					left_x,
					left_y,
					jump,
					interact,
				} => {
					controls.gamepad = GamepadSample {
						left_x,
						left_y,
						jump,
						interact,
					};
					controls.submit(&mut engine)?;
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
					controls = ReplayControls::default();
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
					let reset_controls = matches!(
						command.as_ref(),
						EditorCommandDto::StartPlay | EditorCommandDto::StopPlay
					) || matches!(
						command.as_ref(),
						EditorCommandDto::SetPaused { paused } if *paused != engine.session().is_paused()
					);
					let request = EditorCommandRequest {
						expected_revision: expected_revision.unwrap_or(engine.session().revision()),
						command: *command,
					};
					engine
						.execute(request)
						.map_err(|e| format!("Replay command failed ({:?}): {e}", e.code))?;
					if reset_controls {
						controls = ReplayControls::default();
					}
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
					if advance(
						&mut engine,
						&asset_root,
						count,
						elapsed,
						&mut measures,
						started,
						budget,
					)? {
						controls = ReplayControls::default();
					}
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
	report::finish(
		&engine,
		&options,
		&measures,
		replay_report,
		source_schema,
		started,
		budget,
	)
}

fn advance(
	engine: &mut Engine,
	asset_root: &Path,
	count: u32,
	elapsed: f64,
	measures: &mut Measurements,
	started: Instant,
	budget: Duration,
) -> Result<bool, String> {
	let mut scene_changed = false;
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
				scene_changed = true;
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
	Ok(scene_changed)
}
pub(super) fn check_budget(started: Instant, budget: Duration) -> Result<(), String> {
	if started.elapsed() > budget {
		Err("Headless wall-time budget exceeded between actions".into())
	} else {
		Ok(())
	}
}
