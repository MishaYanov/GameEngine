//! CPU-only runner for an authored project and bounded command/tick replay.
use engine_core::{
	Bounds3, Engine, EngineBuilder,
	command::{EditorCommandDto, EditorCommandRequest},
	project::{MAX_PROJECT_BYTES, ProjectData},
	scene_catalog::SceneId,
	scene_data::{MAX_SCENE_BYTES, SceneData, validate_model_path},
};
use serde::{
	Deserialize, Serialize,
	de::{self, SeqAccess, Visitor},
};
use std::{
	collections::{HashSet, VecDeque},
	ffi::OsString,
	fs::{File, OpenOptions},
	io::{self, Read, Write},
	path::{Path, PathBuf},
	process::ExitCode,
	time::{Duration, Instant},
};

const MAX_REPLAY_BYTES: usize = 8 * 1024 * 1024;
const MAX_REPLAY_ACTIONS: usize = 10_000;
const MAX_TICK_CALLS: u32 = 100_000;
const HELP: &str = "engine-headless --project <project.json> [--asset-root <assets>]\n  or: engine-headless --scene <scene.json> [--asset-root <assets>]\nOptions:\n  --steps <0..100000>     Tick calls (default 60, or 0 with replay).\n  --dt <seconds>          Elapsed time per call; default project's fixed interval.\n  --replay <replay.json>  Version-1 typed command/tick/asset_metadata actions.\n  --budget-ms <1..300000> Budget checked between actions; default 30000.\n  --output <json>         Write summary to a new file as well as stdout.\n  --state-output <json>   Write final render scene to a new file.\n  --help                 Show help.\nProject scenes resolve beneath the asset root (default project folder/assets).\nStandalone scenes have no implicit behaviors. --steps starts Play; replay owns\nits mode transitions. Outputs must not exist. No models, GPU or window are loaded.\nReplay asset_metadata actions supply optional CPU bounds for picking/framing.\n";

#[derive(Default)]
struct Options {
	project: Option<PathBuf>,
	scene: Option<PathBuf>,
	asset_root: Option<PathBuf>,
	replay: Option<PathBuf>,
	output: Option<PathBuf>,
	state_output: Option<PathBuf>,
	steps: Option<u32>,
	dt: Option<f64>,
	budget_ms: Option<u64>,
	help: bool,
}
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
	Command {
		#[serde(default)]
		expected_revision: Option<u64>,
		command: EditorCommandDto,
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
#[derive(Serialize)]
struct Summary {
	schema_version: u32,
	project: String,
	mode: &'static str,
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
	fresh_entity_ids_generated: bool,
	elapsed_ms: f64,
	tick_total_ms: f64,
	tick_max_ms: f64,
}
#[derive(Default)]
struct Measurements {
	commands: u64,
	tick_calls: u32,
	fixed_steps: u64,
	dropped: f64,
	tick_total: Duration,
	tick_max: Duration,
	fresh_ids: bool,
	bounds: HashSet<(String, usize)>,
}

fn main() -> ExitCode {
	let result = parse_options().and_then(|options| {
		if options.help {
			print!("{HELP}");
			return Ok(());
		}
		let summary = run(options)?;
		let stdout = io::stdout();
		let mut output = stdout.lock();
		serde_json::to_writer_pretty(&mut output, &summary).map_err(|e| e.to_string())?;
		output.write_all(b"\n").map_err(|e| e.to_string())
	});
	match result {
		Ok(()) => ExitCode::SUCCESS,
		Err(message) => {
			let _ = serde_json::to_writer(
				io::stderr().lock(),
				&serde_json::json!({"error":{"code":"headless_run_failed","message":message}}),
			);
			eprintln!();
			ExitCode::FAILURE
		}
	}
}
fn parse_options() -> Result<Options, String> {
	let mut arguments: VecDeque<_> = std::env::args_os().skip(1).collect();
	if arguments.len() > 32 {
		return Err("Too many headless arguments".into());
	}
	let mut options = Options::default();
	while let Some(argument) = arguments.pop_front() {
		let flag = argument.to_str().ok_or("Flags must be valid text")?;
		match flag {
			"--project" => unique_path(&mut options.project, value(&mut arguments, flag)?, flag)?,
			"--scene" => unique_path(&mut options.scene, value(&mut arguments, flag)?, flag)?,
			"--asset-root" | "--assets" => {
				unique_path(&mut options.asset_root, value(&mut arguments, flag)?, flag)?
			}
			"--replay" => unique_path(&mut options.replay, value(&mut arguments, flag)?, flag)?,
			"--output" => unique_path(&mut options.output, value(&mut arguments, flag)?, flag)?,
			"--state-output" => unique_path(
				&mut options.state_output,
				value(&mut arguments, flag)?,
				flag,
			)?,
			"--steps" => {
				if options.steps.is_some() {
					return Err("--steps may be provided only once".into());
				}
				let count = text(value(&mut arguments, flag)?)?
					.parse::<u32>()
					.map_err(|_| "--steps needs an unsigned integer")?;
				if count > MAX_TICK_CALLS {
					return Err("--steps exceeds 100000".into());
				}
				options.steps = Some(count);
			}
			"--dt" => {
				if options.dt.is_some() {
					return Err("--dt may be provided only once".into());
				}
				let elapsed = text(value(&mut arguments, flag)?)?
					.parse::<f64>()
					.map_err(|_| "--dt needs seconds as a number")?;
				validate_elapsed(elapsed)?;
				options.dt = Some(elapsed);
			}
			"--budget-ms" => {
				if options.budget_ms.is_some() {
					return Err("--budget-ms may be provided only once".into());
				}
				let budget = text(value(&mut arguments, flag)?)?
					.parse::<u64>()
					.map_err(|_| "--budget-ms needs an integer")?;
				if !(1..=300_000).contains(&budget) {
					return Err("Wall-time budget must be 1..=300000 milliseconds".into());
				}
				options.budget_ms = Some(budget);
			}
			"--help" | "-h" => options.help = true,
			_ => return Err(format!("Unknown argument {flag:?}; use --help")),
		}
	}
	if !options.help && (options.project.is_some() == options.scene.is_some()) {
		return Err("Provide exactly one --project or --scene; use --help".into());
	}
	Ok(options)
}
fn value(arguments: &mut VecDeque<OsString>, flag: &str) -> Result<OsString, String> {
	arguments
		.pop_front()
		.ok_or_else(|| format!("{flag} needs a value"))
}
fn text(value: OsString) -> Result<String, String> {
	value
		.into_string()
		.map_err(|_| "Numeric options must be valid text".into())
}
fn unique_path(target: &mut Option<PathBuf>, value: OsString, flag: &str) -> Result<(), String> {
	if target.is_some() {
		return Err(format!("{flag} may be provided only once"));
	}
	*target = Some(PathBuf::from(value));
	Ok(())
}
fn validate_elapsed(elapsed: f64) -> Result<(), String> {
	if !elapsed.is_finite() || !(0.0..=60.0).contains(&elapsed) {
		return Err("Tick elapsed seconds must be finite and within 0..=60".into());
	}
	Ok(())
}

fn run(options: Options) -> Result<Summary, String> {
	let started = Instant::now();
	let budget = Duration::from_millis(options.budget_ms.unwrap_or(30_000));
	let Source {
		project,
		parsed,
		documents,
	} = source(&options)?;
	let source_schema = parsed.source_schema_version;
	let mut engine = EngineBuilder::new(parsed.data, project)
		.with_scenes(documents)
		.build()
		.map_err(|e| e.to_string())?;
	let mut measures = Measurements::default();
	let steps = options
		.steps
		.unwrap_or(if options.replay.is_some() { 0 } else { 60 });
	if let Some(path) = &options.replay {
		let replay: Replay = serde_json::from_str(&read_bounded(path, MAX_REPLAY_BYTES)?)
			.map_err(|e| format!("Invalid replay: {e}"))?;
		if replay.schema_version != 1 {
			return Err("Only replay schema version 1 is supported".into());
		}
		replay.actions.iter().try_fold(steps, |total, action| {
			let count = if let ReplayAction::Tick { count, .. } = action {
				*count
			} else {
				0
			};
			total
				.checked_add(count)
				.filter(|value| *value <= MAX_TICK_CALLS)
				.ok_or("Replay and --steps exceed 100000 tick calls")
		})?;
		for action in replay.actions {
			check_budget(started, budget)?;
			match action {
				ReplayAction::Command {
					expected_revision,
					command,
				} => {
					measures.fresh_ids |= matches!(
						command,
						EditorCommandDto::Spawn { .. }
							| EditorCommandDto::DuplicateSelection { .. }
					);
					let request = EditorCommandRequest {
						expected_revision: expected_revision.unwrap_or(engine.session().revision()),
						command,
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
					advance(&mut engine, count, elapsed, &mut measures, started, budget)?;
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
		advance(&mut engine, steps, elapsed, &mut measures, started, budget)?;
	}
	check_budget(started, budget)?;
	let final_document = engine.session().render_document();
	let mut hash = StateHash {
		value: 0xcbf29ce484222325,
		bytes: 0,
	};
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
	let summary = Summary {
		schema_version: 1,
		project: engine.session().project().name.clone(),
		mode: if engine.session().is_playing() {
			"play"
		} else {
			"edit"
		},
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
struct Source {
	project: ProjectData,
	parsed: engine_core::scene_data::ParsedScene,
	documents: Vec<(SceneId, SceneData)>,
}
fn source(options: &Options) -> Result<Source, String> {
	if let Some(path) = &options.project {
		let manifest = path
			.canonicalize()
			.map_err(|e| format!("Cannot open project: {e}"))?;
		let project = ProjectData::load(&manifest)?;
		let root = options
			.asset_root
			.clone()
			.unwrap_or_else(|| manifest.parent().unwrap_or(Path::new(".")).join("assets"))
			.canonicalize()
			.map_err(|e| format!("Cannot open asset root: {e}"))?;
		if !root.is_dir() {
			return Err("Asset root must be a directory".into());
		}
		let mut parsed = None;
		let mut documents = Vec::new();
		let mut paths = HashSet::new();
		let mut document_bytes = 0usize;
		for entry in project.scenes.iter() {
			let path = contained(&root, entry.asset_path())?;
			if !paths.insert(path.clone()) {
				return Err("Scene paths resolve to the same file".into());
			}
			let document =
				engine_core::scene_data::parse_json(&read_bounded(&path, MAX_SCENE_BYTES)?)?;
			project.validate_scene_for(entry.id(), &document.data)?;
			document_bytes = document_bytes
				.saturating_add(document.data.to_json()?.len().saturating_mul(4))
				.saturating_add(std::mem::size_of::<SceneData>());
			if document_bytes > engine_core::editor::EditorSession::MAX_RETAINED_DOCUMENT_BYTES {
				return Err("Retained scene document byte budget exceeded".into());
			}
			if entry.id() == &project.startup_scene_id {
				parsed = Some(document);
			} else {
				documents.push((entry.id().clone(), document.data));
			}
		}
		return Ok(Source {
			project,
			parsed: parsed.ok_or("Startup scene is missing")?,
			documents,
		});
	}
	let scene = options
		.scene
		.as_ref()
		.ok_or("A scene or project is required")?
		.canonicalize()
		.map_err(|e| format!("Cannot open scene: {e}"))?;
	let default_root = scene
		.ancestors()
		.skip(1)
		.find(|path| path.file_name().is_some_and(|name| name == "assets"))
		.or_else(|| scene.parent())
		.ok_or("Scene has no parent folder")?;
	let root = options
		.asset_root
		.as_deref()
		.unwrap_or(default_root)
		.canonicalize()
		.map_err(|e| e.to_string())?;
	if !root.is_dir() {
		return Err("Asset root must be a directory".into());
	}
	let relative = scene
		.strip_prefix(&root)
		.map_err(|_| "Scene is outside the supplied asset root")?
		.to_str()
		.ok_or("Scene reference must be valid UTF-8")?
		.replace('\\', "/");
	let parsed = engine_core::scene_data::parse_json(&read_bounded(&scene, MAX_SCENE_BYTES)?)?;
	// A standalone run has no persisted catalog. Its transient identity must
	// remain stable so identical inputs produce comparable diagnostic hashes.
	Ok(Source {
		project: ProjectData::for_scene_with_id(
			"Headless scene",
			SceneId::new("headless-scene")?,
			relative,
			&parsed.data,
		)?,
		parsed,
		documents: Vec::new(),
	})
}
fn contained(root: &Path, relative: &str) -> Result<PathBuf, String> {
	validate_model_path(relative)?;
	let path = root
		.join(relative)
		.canonicalize()
		.map_err(|e| format!("Cannot resolve startup scene: {e}"))?;
	if !path.starts_with(root) {
		return Err("Startup scene escapes its asset root".into());
	}
	Ok(path)
}
fn read_bounded(path: &Path, limit: usize) -> Result<String, String> {
	let file = File::open(path).map_err(|e| format!("Cannot read '{}': {e}", path.display()))?;
	if !file.metadata().map_err(|e| e.to_string())?.is_file() {
		return Err("Inputs must be regular files".into());
	}
	let mut source = String::new();
	file.take(limit as u64 + 1)
		.read_to_string(&mut source)
		.map_err(|e| e.to_string())?;
	if source.len() > limit {
		return Err(format!(
			"Input '{}' exceeds its {limit}-byte limit",
			path.display()
		));
	}
	Ok(source)
}
fn advance(
	engine: &mut Engine,
	count: u32,
	elapsed: f64,
	measures: &mut Measurements,
	started: Instant,
	budget: Duration,
) -> Result<(), String> {
	for _ in 0..count {
		check_budget(started, budget)?;
		let tick_start = Instant::now();
		let outcome = engine.tick(elapsed).map_err(|e| e.to_string())?;
		let time = tick_start.elapsed();
		measures.tick_total += time;
		measures.tick_max = measures.tick_max.max(time);
		measures.tick_calls += 1;
		measures.fixed_steps += outcome.steps as u64;
		measures.dropped += outcome.dropped_seconds;
	}
	Ok(())
}
fn check_budget(started: Instant, budget: Duration) -> Result<(), String> {
	if started.elapsed() > budget {
		Err("Headless wall-time budget exceeded between actions".into())
	} else {
		Ok(())
	}
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
}
impl Write for StateHash {
	fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
		self.bytes = self
			.bytes
			.checked_add(bytes.len())
			.filter(|value| *value <= MAX_SCENE_BYTES + MAX_PROJECT_BYTES + 128)
			.ok_or_else(|| {
				io::Error::other("State hash input exceeds the scene/project byte budget")
			})?;
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
