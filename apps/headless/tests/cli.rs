use engine_core::{
	EntityKey,
	project::{BehaviorSpec, ProjectData},
	scene_data::{CURRENT_SCHEMA_VERSION, CameraData, EntityData, SceneData, TransformData},
};
use serde_json::{Value, json};
use std::{
	fs,
	path::PathBuf,
	process::{Command, Output},
	sync::atomic::{AtomicU64, Ordering},
};
struct Fixture {
	root: PathBuf,
	scene: PathBuf,
	project: PathBuf,
}
impl Fixture {
	fn new() -> Self {
		static NEXT: AtomicU64 = AtomicU64::new(0);
		let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
			.join("../../target")
			.join(format!(
				"qa-headless-{}-{}",
				std::process::id(),
				NEXT.fetch_add(1, Ordering::Relaxed)
			));
		fs::create_dir(&root).unwrap();
		fs::create_dir_all(root.join("assets/scenes")).unwrap();
		let camera = engine_core::Camera::default();
		let scene = SceneData {
			schema_version: CURRENT_SCHEMA_VERSION,
			organization: Default::default(),
			default_game_camera: None,
			bindings: Default::default(),
			lights: vec![],
			entities: vec![
				EntityData::new(
					EntityKey::new("actor").unwrap(),
					"Actor",
					"models/not-loaded.glb",
				),
				EntityData::new(
					EntityKey::new("other").unwrap(),
					"Other",
					"models/not-loaded.glb",
				)
				.with_transform(TransformData {
					position: [5.0, 0.0, 0.0],
					..Default::default()
				}),
			],
			camera: CameraData {
				follow: None,
				orbit: None,
				position: camera.position,
				yaw: camera.yaw,
				pitch: camera.pitch,
				vertical_fov: camera.vertical_fov,
				near_plane: camera.near,
				far_plane: camera.far,
			},
		};
		let mut project =
			ProjectData::for_scene("Headless authored project", "scenes/main.json", &scene)
				.unwrap();
		for behavior in [
			BehaviorSpec::ControlledMove {
				entity: EntityKey::new("actor").unwrap(),
				speed: 2.0,
				space: Default::default(),
			},
			BehaviorSpec::Spin {
				entity: EntityKey::new("actor").unwrap(),
				angular_velocity: [0.0, 0.0, 1.0],
			},
		] {
			project.add_behavior(behavior);
		}
		let scene_path = root.join("assets/scenes/main.json");
		let project_path = root.join("project.json");
		fs::write(&scene_path, scene.to_json().unwrap()).unwrap();
		fs::write(&project_path, project.to_json().unwrap()).unwrap();
		Self {
			root,
			scene: scene_path,
			project: project_path,
		}
	}
	fn command(&self) -> Command {
		let mut command = Command::new(env!("CARGO_BIN_EXE_engine-headless"));
		command.arg("--project").arg(&self.project);
		command
	}
	fn configure_orbit(&self, follow_seconds: f32) {
		let mut scene: Value = serde_json::from_slice(&fs::read(&self.scene).unwrap()).unwrap();
		scene["camera"]["orbit"] = json!({
			"target":"actor", "pivot_offset":[0,0,0], "initial_yaw":0.0, "initial_pitch":0.0,
			"initial_distance":4.0, "min_distance":1.0, "max_distance":10.0, "min_pitch":-1.0,
			"max_pitch":1.0, "follow_seconds":follow_seconds, "recovery_seconds":0.0,
			"teleport_distance":5.0, "probe_radius":0.3, "obstruction_mask":1
		});
		fs::write(&self.scene, scene.to_string()).unwrap();
	}
	fn replay(&self, actions: Vec<Value>) -> PathBuf {
		self.replay_version(1, actions)
	}
	fn replay_version(&self, schema_version: u32, actions: Vec<Value>) -> PathBuf {
		let path = self.root.join("replay.json");
		fs::write(
			&path,
			json!({"schema_version":schema_version,"actions":actions}).to_string(),
		)
		.unwrap();
		path
	}
}
impl Drop for Fixture {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.root);
	}
}
fn success(output: Output) -> Value {
	assert!(
		output.status.success(),
		"headless failed: {}",
		String::from_utf8_lossy(&output.stderr)
	);
	serde_json::from_slice(&output.stdout).unwrap()
}
fn failure(output: Output) -> Value {
	assert!(
		!output.status.success(),
		"unexpected success: {}",
		String::from_utf8_lossy(&output.stdout)
	);
	let error: Value = serde_json::from_slice(&output.stderr).unwrap();
	assert_eq!(error["error"]["code"], "headless_run_failed");
	error
}

#[test]
fn core_logs_use_application_sinks_without_polluting_cli_output() {
	let fixture = Fixture::new();
	for mode in ["regular", "debug"] {
		let logs = fixture.root.join(mode);
		let output = fixture
			.command()
			.args(["--steps", "1"])
			.env("ENGINE_LOG_DIR", &logs)
			.env("ENGINE_LOG_MODE", mode)
			.env("ENGINE_LOG_CONSOLE", "1")
			.output()
			.unwrap();
		assert!(output.stderr.is_empty());
		success(output);
		let session = fs::read_dir(logs).unwrap().next().unwrap().unwrap().path();
		let records = engine_logger::read_jsonl(session.join("000000.jsonl"), 512 * 1024).unwrap();
		assert!(
			records
				.iter()
				.any(|entry| entry.component == "engine_core::play"
					&& entry.level == engine_logger::Level::Info
					&& entry.message.starts_with("Play started"))
		);
		assert_eq!(
			records
				.iter()
				.any(|entry| entry.component == "engine_core::engine"
					&& entry.level == engine_logger::Level::Debug),
			mode == "debug",
		);
	}

	let logs = fixture.root.join("failed");
	let replay = fixture.replay(vec![
		json!({"type":"command","expected_revision":100,"command":{"type":"select","keys":[]}}),
	]);
	let output = fixture
		.command()
		.arg("--replay")
		.arg(replay)
		.env("ENGINE_LOG_DIR", &logs)
		.env("ENGINE_LOG_MODE", "debug")
		.env("ENGINE_LOG_CONSOLE", "1")
		.output()
		.unwrap();
	assert!(output.stdout.is_empty());
	failure(output);
	let session = fs::read_dir(logs).unwrap().next().unwrap().unwrap().path();
	let errors = engine_logger::read_jsonl(session.join("errors.jsonl"), 512 * 1024).unwrap();
	assert_eq!(errors.len(), 1);
	assert_eq!(errors[0].component, "headless");
	assert_eq!(
		errors[0].message,
		"Headless execution failed: operation=run"
	);
}

#[test]
fn help_and_invalid_options_exit_without_graphics_or_project_io() {
	let help = Command::new(env!("CARGO_BIN_EXE_engine-headless"))
		.arg("--help")
		.output()
		.unwrap();
	assert!(help.status.success());
	let help = String::from_utf8_lossy(&help.stdout);
	assert!(help.contains("No GPU or window is loaded"));
	assert!(help.contains("Animated assets are decoded on the CPU"));
	for args in [
		vec![],
		vec!["--unknown"],
		vec!["--project"],
		vec!["--scene", "x", "--project", "y"],
		vec!["--scene", "x", "--steps", "100001"],
		vec!["--scene", "x", "--dt", "NaN"],
		vec!["--scene", "x", "--dt", "-1"],
		vec!["--scene", "x", "--budget-ms", "0"],
	] {
		failure(
			Command::new(env!("CARGO_BIN_EXE_engine-headless"))
				.args(args)
				.output()
				.unwrap(),
		);
	}
}

#[test]
fn replay_restart_requires_play_and_rejects_unrecognized_fields() {
	let fixture = Fixture::new();
	for action in [
		json!({"type":"restart_run"}),
		json!({"type":"restart_run","unexpected":true}),
	] {
		let replay = fixture.replay(vec![action]);
		let output = failure(
			fixture
				.command()
				.arg("--replay")
				.arg(replay)
				.output()
				.unwrap(),
		);
		assert!(
			output["error"]["message"]
				.as_str()
				.unwrap()
				.contains("Play mode")
				|| output["error"]["message"]
					.as_str()
					.unwrap()
					.contains("unknown field")
		);
	}
}

#[test]
fn headless_rejects_an_unavailable_gameplay_module() {
	let fixture = Fixture::new();
	let mut project = ProjectData::load(&fixture.project).unwrap();
	project.gameplay_module = Some("missing_module".into());
	fs::write(&fixture.project, project.to_json().unwrap()).unwrap();

	let error = failure(fixture.command().arg("--steps").arg("1").output().unwrap());
	assert!(
		error["error"]["message"]
			.as_str()
			.unwrap()
			.contains("missing_module")
	);
}

#[test]
fn replay_uses_authored_behavior_targets_and_shared_simulation_with_deterministic_state_hash() {
	let fixture = Fixture::new();
	let source = fs::read(&fixture.scene).unwrap();
	let replay = fixture.replay(vec![json!({"type":"command","command":{"type":"select","keys":["other"]}}), json!({"type":"command","command":{"type":"start_play"}}), json!({"type":"command","command":{"type":"set_entity_input","entity":"actor","movement":[1,0,0]}}), json!({"type":"tick","count":6})]);
	let state = fixture.root.join("state.json");
	let summary = success(
		fixture
			.command()
			.arg("--replay")
			.arg(&replay)
			.arg("--state-output")
			.arg(&state)
			.output()
			.unwrap(),
	);
	let repeated = success(
		fixture
			.command()
			.arg("--replay")
			.arg(&replay)
			.output()
			.unwrap(),
	);
	assert_eq!(summary["fixed_steps"], 6);
	assert_eq!(summary["simulation_tick"], 6);
	assert_eq!(summary["commands"], 3);
	assert_eq!(summary["revision"], 3);
	assert_eq!(summary["state_hash"], repeated["state_hash"]);
	assert_eq!(summary["fresh_entity_ids_generated"], false);
	let final_scene = SceneData::from_json(&fs::read_to_string(state).unwrap()).unwrap();
	assert!((final_scene.entities[0].transform.position[0] - 0.2).abs() < 1e-5);
	assert!((final_scene.entities[0].transform.rotation[2] - 0.1).abs() < 1e-5);
	assert_eq!(final_scene.entities[1].transform.position, [5.0, 0.0, 0.0]);
	assert_eq!(fs::read(&fixture.scene).unwrap(), source);
}

#[test]
fn standalone_scene_has_no_implicit_gameplay_and_does_not_load_model_files() {
	let fixture = Fixture::new();
	let state = fixture.root.join("static-state.json");
	let summary = success(
		Command::new(env!("CARGO_BIN_EXE_engine-headless"))
			.arg("--scene")
			.arg(&fixture.scene)
			.arg("--steps")
			.arg("60")
			.arg("--state-output")
			.arg(&state)
			.output()
			.unwrap(),
	);
	assert_eq!(summary["tick_calls"], 60);
	assert_eq!(summary["fixed_steps"], 0);
	assert_eq!(summary["asset_references"], 1);
	assert_eq!(summary["model_bounds_loaded"], 0);
	assert_eq!(
		SceneData::from_json(&fs::read_to_string(state).unwrap()).unwrap(),
		SceneData::from_json(&fs::read_to_string(&fixture.scene).unwrap()).unwrap()
	);
}

#[test]
fn repeated_standalone_runs_hash_the_same_semantics_and_detect_authored_changes() {
	let fixture = Fixture::new();
	let source = fs::read(&fixture.scene).unwrap();
	let invoke = || {
		success(
			Command::new(env!("CARGO_BIN_EXE_engine-headless"))
				.arg("--scene")
				.arg(&fixture.scene)
				.arg("--steps")
				.arg("0")
				.output()
				.unwrap(),
		)
	};
	let first = invoke();
	let repeated = invoke();
	assert_eq!(first["state_hash"], repeated["state_hash"]);
	assert_eq!(first["state_hash_bytes"], repeated["state_hash_bytes"]);
	assert!(first["state_hash_bytes"].as_u64().unwrap() > 0);
	assert_eq!(first["fresh_entity_ids_generated"], false);
	assert_eq!(fs::read(&fixture.scene).unwrap(), source);

	let mut changed = SceneData::from_json(std::str::from_utf8(&source).unwrap()).unwrap();
	changed.entities[0].name = "Changed authored actor".into();
	fs::write(&fixture.scene, changed.to_json().unwrap()).unwrap();
	assert_ne!(first["state_hash"], invoke()["state_hash"]);
}

#[test]
fn explicit_bounds_support_framing_without_importing_geometry_and_existing_outputs_are_preserved() {
	let fixture = Fixture::new();
	let replay = fixture.replay(vec![json!({"type":"asset_metadata","path":"models/not-loaded.glb","mesh_index":0,"min":[-1,-1,-1],"max":[1,1,1]}), json!({"type":"command","command":{"type":"select","keys":["actor"]}}), json!({"type":"command","command":{"type":"frame_selection"}})]);
	let output = fixture.root.join("summary.json");
	let summary = success(
		fixture
			.command()
			.arg("--replay")
			.arg(&replay)
			.arg("--output")
			.arg(&output)
			.output()
			.unwrap(),
	);
	assert_eq!(summary["model_bounds_loaded"], 1);
	assert_eq!(summary["mode"], "edit");
	let original = fs::read(&output).unwrap();
	failure(
		fixture
			.command()
			.arg("--replay")
			.arg(replay)
			.arg("--output")
			.arg(&output)
			.output()
			.unwrap(),
	);
	assert_eq!(fs::read(output).unwrap(), original);
}

#[test]
fn thousand_and_ten_thousand_entity_runs_report_bounded_incremental_work_without_timing_thresholds()
{
	for entities in [1000, 10_000] {
		let fixture = Fixture::new();
		let mut data = SceneData::from_json(&fs::read_to_string(&fixture.scene).unwrap()).unwrap();
		data.entities.truncate(1);
		for index in 1..entities {
			data.entities.push(EntityData::new(
				EntityKey::new(format!("static-{index}")).unwrap(),
				"Static",
				"models/not-loaded.glb",
			));
		}
		fs::write(&fixture.scene, data.to_json().unwrap()).unwrap();
		let replay = fixture.replay(vec![json!({"type":"command","command":{"type":"start_play"}}), json!({"type":"command","command":{"type":"set_entity_input","entity":"actor","movement":[1,0,0]}}), json!({"type":"tick","count":120})]);
		let report = success(
			fixture
				.command()
				.arg("--replay")
				.arg(replay)
				.output()
				.unwrap(),
		);
		assert_eq!(report["entities"], entities);
		assert_eq!(report["fixed_steps"], 120);
		assert_eq!(report["collision_rebuilds"], 1);
		assert_eq!(report["collision_updates"], 0);
		assert!(report["matrix_updates"].as_u64().unwrap() <= entities as u64 + 4 * 120);
		for field in ["elapsed_ms", "tick_total_ms", "tick_max_ms"] {
			assert!(report[field].as_f64().unwrap().is_finite());
		}
		eprintln!("CPU RESOURCE REPORT {}", serde_json::to_string(&json!({"entities":entities,"fixed_steps":report["fixed_steps"],"matrix_updates":report["matrix_updates"],"collision_rebuilds":report["collision_rebuilds"],"elapsed_ms":report["elapsed_ms"],"tick_total_ms":report["tick_total_ms"],"tick_max_ms":report["tick_max_ms"]})).unwrap());
	}
}

#[path = "cli/camera.rs"]
mod camera;
#[path = "cli/scenes.rs"]
mod scenes;
#[path = "cli/validation.rs"]
mod validation;
