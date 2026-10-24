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
		project.behaviors = vec![
			BehaviorSpec::ControlledMove {
				entity: EntityKey::new("actor").unwrap(),
				speed: 2.0,
				space: Default::default(),
			},
			BehaviorSpec::Spin {
				entity: EntityKey::new("actor").unwrap(),
				angular_velocity: [0.0, 0.0, 1.0],
			},
		];
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
	fn replay(&self, actions: Vec<Value>) -> PathBuf {
		let path = self.root.join("replay.json");
		fs::write(
			&path,
			json!({"schema_version":1,"actions":actions}).to_string(),
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
fn help_and_invalid_options_exit_without_graphics_or_project_io() {
	let help = Command::new(env!("CARGO_BIN_EXE_engine-headless"))
		.arg("--help")
		.output()
		.unwrap();
	assert!(help.status.success());
	assert!(String::from_utf8_lossy(&help.stdout).contains("No models, GPU or window"));
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
fn stale_replay_revision_fails_atomically_without_creating_output_or_changing_source() {
	let fixture = Fixture::new();
	let source = fs::read(&fixture.scene).unwrap();
	let replay = fixture.replay(vec![json!({"type":"command","expected_revision":99,"command":{"type":"rename","key":"actor","name":"Rejected"}})]);
	let output = fixture.root.join("summary.json");
	let error = failure(
		fixture
			.command()
			.arg("--replay")
			.arg(replay)
			.arg("--output")
			.arg(&output)
			.output()
			.unwrap(),
	);
	assert!(
		error["error"]["message"]
			.as_str()
			.unwrap()
			.contains("Conflict")
	);
	assert!(!output.exists());
	assert_eq!(fs::read(&fixture.scene).unwrap(), source);
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
fn replay_action_and_combined_tick_limits_fail_before_running_commands() {
	let fixture = Fixture::new();
	let replay = fixture.replay(vec![json!({"type":"tick","count":100001})]);
	let error = failure(
		fixture
			.command()
			.arg("--replay")
			.arg(&replay)
			.output()
			.unwrap(),
	);
	assert!(
		error["error"]["message"]
			.as_str()
			.unwrap()
			.contains("100000")
	);
	let replay = fixture.replay(vec![json!({"type":"tick","count":0}); 10001]);
	let error = failure(
		fixture
			.command()
			.arg("--replay")
			.arg(replay)
			.output()
			.unwrap(),
	);
	assert!(
		error["error"]["message"]
			.as_str()
			.unwrap()
			.contains("10000")
	);
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
