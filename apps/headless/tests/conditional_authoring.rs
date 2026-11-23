use engine_core::{
	Engine, EngineBuilder, EntityKey, SceneData,
	asset_catalog::AssetId,
	command::{EditorCommandDto, EditorCommandRequest},
	editor::{EditorSession, PrefabData},
	linked_prefab::LinkedPrefabSource,
	project::ProjectData,
	simulation_session::CommittedInputAction,
	triggers::{TriggerEvent, TriggerPhase},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
	fs,
	path::{Path, PathBuf},
	process::{Command, Stdio},
	sync::Arc,
	time::{Duration, Instant},
};

const FIXTURE: &str = "tests/fixtures/conditional-authoring";
const SCENE: &str = "scenes/conditional-lab.json";
const PROP: &str = "prefabs/trigger-door.prefab.json";
const PROP_ID: &str = "90000000-0000-4000-8000-000000000031";
const STEP: f64 = 1.0 / 60.0;

struct TestProject {
	root: PathBuf,
	doors: Vec<(EntityKey, EntityKey)>,
}

impl TestProject {
	fn new() -> Self {
		let root = std::env::temp_dir().join(format!(
			"conditional-authoring-{}",
			AssetId::generate().as_str()
		));
		fs::create_dir(&root).unwrap();
		let mut fixture = Self {
			root,
			doors: Vec::new(),
		};
		let source = Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURE);
		copy_fixture(&source, &fixture.root);
		let project = ProjectData::load(fixture.manifest()).unwrap();
		let path = fixture.assets().join(SCENE);
		let scene = SceneData::from_json(&fs::read_to_string(&path).unwrap()).unwrap();
		let prop_bytes = fs::read(fixture.assets().join(PROP)).unwrap();
		let link = LinkedPrefabSource {
			source_id: AssetId::new(PROP_ID).unwrap(),
			path: PROP.into(),
			fingerprint: format!("{:x}", Sha256::digest(&prop_bytes)),
			prefab: PrefabData::from_json(std::str::from_utf8(&prop_bytes).unwrap()).unwrap(),
		};
		let mut session = EditorSession::for_project(scene, project).unwrap();
		for x in [2.4, 8.0] {
			session
				.instantiate_linked_prefab(link.clone(), [x, 0., 0.], Default::default())
				.unwrap();
		}
		let scene = session.authored_document();
		let mut links = scene
			.entities
			.iter()
			.filter_map(|entity| {
				entity
					.linked_prefab
					.as_ref()
					.map(|link| (entity.transform.position[0], link))
			})
			.collect::<Vec<_>>();
		links.sort_by(|a, b| a.0.total_cmp(&b.0));
		fixture.doors = links
			.iter()
			.map(|(_, link)| {
				(
					link.entity_map[&key("gate")].clone(),
					link.entity_map[&key("sensor")].clone(),
				)
			})
			.collect();
		fs::write(path, scene.to_json().unwrap()).unwrap();
		fixture
	}

	fn manifest(&self) -> PathBuf {
		self.root.join("project.json")
	}

	fn assets(&self) -> PathBuf {
		self.root.join("assets")
	}

	fn output(&self, name: &str) -> PathBuf {
		let path = self.root.join(name);
		fs::create_dir(&path).unwrap();
		path
	}
}

impl Drop for TestProject {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.root);
	}
}

#[derive(Debug, PartialEq)]
struct Observation {
	label: String,
	grounded: bool,
	yaws: Vec<f32>,
}

#[derive(Debug, PartialEq)]
struct Trace {
	actions: Vec<CommittedInputAction>,
	triggers: Vec<TriggerEvent>,
	observations: Vec<Observation>,
}

fn key(name: &str) -> EntityKey {
	EntityKey::new(name).unwrap()
}

fn copy_fixture(from: &Path, to: &Path) {
	for entry in fs::read_dir(from).unwrap() {
		let entry = entry.unwrap();
		let target = to.join(entry.file_name());
		if entry.file_type().unwrap().is_dir() {
			fs::create_dir(&target).unwrap();
			copy_fixture(&entry.path(), &target);
		} else {
			fs::copy(entry.path(), target).unwrap();
		}
	}
}

fn execute(engine: &mut Engine, command: EditorCommandDto) {
	engine
		.execute(EditorCommandRequest {
			expected_revision: engine.session().revision(),
			command,
		})
		.unwrap();
}

fn build_engine(manifest: &Path) -> Engine {
	let project = ProjectData::load(manifest).unwrap();
	let assets = manifest.parent().unwrap().join("assets");
	let data = fs::read_to_string(assets.join(SCENE)).unwrap();
	EngineBuilder::new(SceneData::from_json(&data).unwrap(), project)
		.module_resolver(Arc::new(gameplay_catalog::resolve))
		.input_actions_resolver(Arc::new(move |scene| {
			bridge::input_actions::load_scene(&assets, scene)
		}))
		.build()
		.unwrap()
}

fn observe(engine: &Engine, doors: &[(EntityKey, EntityKey)], label: &str) -> Observation {
	let inspection = engine.runtime_inspection(0, 128).unwrap();
	let grounded = inspection
		.character_motion
		.iter()
		.find(|motion| motion.entity.key.as_str() == "yard-player")
		.unwrap()
		.grounded;
	let scene = engine.session().render_world().unwrap().data();
	let yaws = doors
		.iter()
		.map(|(door, _)| {
			scene
				.entities
				.iter()
				.find(|entity| &entity.key == door)
				.unwrap()
				.transform
				.rotation[1]
		})
		.collect();
	Observation {
		label: label.into(),
		grounded,
		yaws,
	}
}

fn play_route(
	manifest: &Path,
	replay: &Value,
	doors: &[(EntityKey, EntityKey)],
	frame_steps: u64,
) -> Trace {
	let mut engine = build_engine(manifest);
	let authored_before = engine.session().authored_document();
	let mut trace = Trace {
		actions: Vec::new(),
		triggers: Vec::new(),
		observations: Vec::new(),
	};
	for action in replay["actions"].as_array().unwrap() {
		match action["type"].as_str().unwrap() {
			"command" => execute(
				&mut engine,
				serde_json::from_value(action["command"].clone()).unwrap(),
			),
			"keyboard_input" => {
				let keys = serde_json::from_value(action["keys"].clone()).unwrap();
				engine
					.set_action_controls_from_codes(&keys, Default::default(), Default::default())
					.unwrap();
			}
			"tick" => {
				let mut steps = action["count"].as_u64().unwrap();
				while steps > 0 {
					let chunk = steps.min(frame_steps);
					let outcome = engine.tick(STEP * chunk as f64).unwrap();
					assert_eq!(u64::from(outcome.steps), chunk);
					trace.actions.extend(outcome.accepted_action_presses);
					trace.triggers.extend(outcome.trigger_events);
					steps -= chunk;
				}
			}
			"mark" => {
				trace
					.observations
					.push(observe(&engine, doors, action["label"].as_str().unwrap()))
			}
			other => panic!("Unexpected fixture replay action {other}"),
		}
	}

	let first_run = engine.runtime_inspection(0, 128).unwrap().run_id;
	execute(&mut engine, EditorCommandDto::StopPlay);
	assert_eq!(engine.session().authored_document(), authored_before);
	execute(&mut engine, EditorCommandDto::StartPlay);
	engine.tick(STEP).unwrap();
	let restarted = observe(&engine, doors, "restart");
	assert_eq!(restarted.yaws, vec![0.0; doors.len()]);
	assert_ne!(engine.runtime_inspection(0, 128).unwrap().run_id, first_run);
	trace
}

fn headless(manifest: &Path, replay: &Path, current_dir: &Path) -> Value {
	let output = Command::new(env!("CARGO_BIN_EXE_engine-headless"))
		.current_dir(current_dir)
		.arg("--project")
		.arg(manifest)
		.arg("--replay")
		.arg(replay)
		.output()
		.unwrap();
	assert!(
		output.status.success(),
		"{}",
		String::from_utf8_lossy(&output.stderr)
	);
	serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn conditional_controls_and_linked_doors_agree_in_source_and_staged_player_data() {
	let fixture = TestProject::new();
	let manifest = fixture.manifest();
	let replay_path = fixture.root.join("route.replay.json");
	let replay: Value = serde_json::from_slice(&fs::read(&replay_path).unwrap()).unwrap();
	let scene_path = fixture.assets().join(SCENE);
	let authored_bytes = fs::read(&scene_path).unwrap();
	let project = ProjectData::load(&manifest).unwrap();
	let prepared = compiler::prepare_project(&project, &fixture.assets()).unwrap();
	let output = fixture.output("staged");
	let staged = compiler::stage_prepared_project(&prepared, output.to_str().unwrap()).unwrap();
	let packaged_manifest = staged.path().join("project.json");

	let trace = play_route(&manifest, &replay, &fixture.doors, 1);
	assert_eq!(
		trace,
		play_route(&packaged_manifest, &replay, &fixture.doors, 4)
	);
	assert_eq!(
		trace
			.observations
			.iter()
			.map(|point| point.grounded)
			.collect::<Vec<_>>(),
		[true, false, false, true, true,]
	);
	assert_eq!(
		trace
			.actions
			.iter()
			.map(|action| action.id.as_str())
			.collect::<Vec<_>>(),
		["jump", "attack"]
	);
	assert!(
		trace.actions[1].tick > 7,
		"the held Jump must not become Attack"
	);
	let (first_door, first_sensor) = &fixture.doors[0];
	let (_, second_sensor) = &fixture.doors[1];
	assert_eq!(
		trace
			.triggers
			.iter()
			.filter(|edge| edge.sensor == *first_sensor
				&& edge.other.as_str() == "yard-player"
				&& edge.phase == TriggerPhase::Enter)
			.count(),
		1
	);
	assert!(
		trace
			.triggers
			.iter()
			.all(|edge| edge.sensor != *second_sensor)
	);
	let opened = &trace.observations.last().unwrap().yaws;
	assert!(
		(opened[0].abs() - std::f32::consts::FRAC_PI_2).abs() < 0.05,
		"{first_door:?}: {opened:?}"
	);
	assert_eq!(opened[1], 0.0);

	let unrelated = fixture.output("unrelated");
	let source_report = headless(&manifest, &replay_path, &unrelated);
	let packaged_report = headless(&packaged_manifest, &replay_path, &unrelated);
	assert_eq!(
		source_report["accepted_action_presses"],
		json!(trace.actions)
	);
	assert_eq!(
		source_report["accepted_action_presses"],
		packaged_report["accepted_action_presses"]
	);
	assert_eq!(source_report["state_hash"], packaged_report["state_hash"]);
	assert_eq!(source_report["markers"], packaged_report["markers"]);
	assert_eq!(fs::read(scene_path).unwrap(), authored_bytes);
}

#[test]
fn ignored_player_trigger_pair_keeps_the_linked_door_closed() {
	let fixture = TestProject::new();
	let manifest = fixture.manifest();
	let mut project: Value = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
	project["physics_layers"]["ignored_pairs"]
		.as_array_mut()
		.unwrap()
		.push(json!([3, 5]));
	fs::write(&manifest, serde_json::to_vec_pretty(&project).unwrap()).unwrap();
	let replay: Value =
		serde_json::from_slice(&fs::read(fixture.root.join("route.replay.json")).unwrap()).unwrap();
	let trace = play_route(&manifest, &replay, &fixture.doors, 4);
	assert_eq!(trace.observations.last().unwrap().yaws, [0.0, 0.0]);
	assert!(trace.triggers.is_empty());
}

#[test]
fn preflight_rejects_an_unregistered_enabled_script() {
	let fixture = TestProject::new();
	let scene_path = fixture.assets().join(SCENE);
	let mut scene: Value = serde_json::from_slice(&fs::read(&scene_path).unwrap()).unwrap();
	let floor = scene["entities"]
		.as_array_mut()
		.unwrap()
		.iter_mut()
		.find(|entity| entity["key"] == "yard-floor")
		.unwrap();
	floor["components"].as_array_mut().unwrap().push(json!({
		"id": "unknown-script",
		"schema_version": 1,
		"enabled": true,
		"data": {
			"kind": "script",
			"settings": {"script_id": "project.missing_fixture_script", "config_version": 1}
		}
	}));
	fs::write(scene_path, serde_json::to_vec_pretty(&scene).unwrap()).unwrap();
	let project = ProjectData::load(fixture.manifest()).unwrap();
	let error = compiler::prepare_project(&project, &fixture.assets())
		.err()
		.expect("an unresolved enabled script must block preflight");
	assert!(error.contains("project.missing_fixture_script"), "{error}");
}

#[test]
#[ignore = "requires compiling the standalone Vulkan Player and a working Vulkan driver"]
fn exported_conditional_world_starts_in_the_actual_player() {
	let fixture = TestProject::new();
	let manifest = fixture.manifest();
	let output = fixture.output("native");
	let installed =
		compiler::install_project_from_manifest(&manifest, output.to_str().unwrap()).unwrap();
	let player = installed.join(format!("game{}", std::env::consts::EXE_SUFFIX));
	let preferences = fixture.output("player-preferences");
	let stdout = fixture.root.join("player.stdout");
	let stderr = fixture.root.join("player.stderr");
	let mut child = Command::new(player)
		.current_dir(fixture.output("player-unrelated"))
		.env("LOCALAPPDATA", &preferences)
		.env("XDG_STATE_HOME", preferences)
		.env("ENGINE_LOG_DIR", fixture.output("player-logs"))
		.arg("--smoke-test")
		.stdout(Stdio::from(fs::File::create(&stdout).unwrap()))
		.stderr(Stdio::from(fs::File::create(&stderr).unwrap()))
		.spawn()
		.unwrap();
	let started = Instant::now();
	let status = loop {
		if let Some(status) = child.try_wait().unwrap() {
			break status;
		}
		if started.elapsed() >= Duration::from_secs(90) {
			let _ = child.kill();
			let _ = child.wait();
			panic!(
				"Vulkan Player timed out. {}",
				fs::read_to_string(&stderr).unwrap_or_default()
			);
		}
		std::thread::sleep(Duration::from_millis(50));
	};
	assert!(
		status.success(),
		"{}",
		fs::read_to_string(&stderr).unwrap_or_default()
	);
	let transcript = fs::read_to_string(stdout).unwrap();
	assert!(
		transcript.contains("GAME_STARTUP_SMOKE_OK mode=player"),
		"{transcript}"
	);
}
