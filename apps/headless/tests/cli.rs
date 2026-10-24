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
fn replay_switches_between_catalog_scenes_and_preserves_drafts_and_scene_tagged_history() {
	use engine_core::scene_catalog::{SceneDescriptor, SceneId};

	let fixture = Fixture::new();
	let main_source = fs::read(&fixture.scene).unwrap();
	let mut secondary = SceneData::from_json(std::str::from_utf8(&main_source).unwrap()).unwrap();
	secondary.entities.truncate(1);
	secondary.entities[0].name = "Secondary actor".into();
	secondary.entities[0].transform.position = [40.0, 0.0, 0.0];
	let secondary_path = fixture.root.join("assets/scenes/secondary.json");
	fs::write(&secondary_path, secondary.to_json().unwrap()).unwrap();
	let secondary_source = fs::read(&secondary_path).unwrap();
	let mut project = ProjectData::load(&fixture.project).unwrap();
	let main_id = project.startup_scene_id.clone();
	let secondary_id = SceneId::new("secondary").unwrap();
	project
		.scenes
		.insert(SceneDescriptor::new(secondary_id.clone(), "scenes/secondary.json").unwrap())
		.unwrap();
	fs::write(&fixture.project, project.to_json().unwrap()).unwrap();
	let manifest_source = fs::read(&fixture.project).unwrap();

	let actions = vec![
		json!({"type":"command","command":{"type":"rename","key":"actor","name":"Edited main"}}),
		json!({"type":"command","command":{"type":"switch_scene","scene_id":secondary_id.as_str()}}),
		json!({"type":"command","command":{"type":"rename","key":"actor","name":"Edited secondary"}}),
		json!({"type":"command","command":{"type":"switch_scene","scene_id":main_id.as_str()}}),
		json!({"type":"command","command":{"type":"undo"}}),
		json!({"type":"command","command":{"type":"undo"}}),
		json!({"type":"command","command":{"type":"redo"}}),
		json!({"type":"command","command":{"type":"redo"}}),
		json!({"type":"command","command":{"type":"switch_scene","scene_id":main_id.as_str()}}),
	];
	// Each prefix exposes an intermediate active document through the existing
	// state output. Global undo/redo activates the scene owning that record.
	for (checkpoint, command_count, expected_name, expected_position, expected_entities) in [
		(
			"secondary-undone",
			5,
			"Secondary actor",
			[40.0, 0.0, 0.0],
			1,
		),
		("main-undone", 6, "Actor", [0.0, 0.0, 0.0], 2),
		(
			"secondary-redone",
			8,
			"Edited secondary",
			[40.0, 0.0, 0.0],
			1,
		),
		("main-retained", 9, "Edited main", [0.0, 0.0, 0.0], 2),
	] {
		let replay = fixture.replay(actions[..command_count].to_vec());
		let state_path = fixture.root.join(format!("{checkpoint}.json"));
		let summary = success(
			fixture
				.command()
				.arg("--replay")
				.arg(&replay)
				.arg("--state-output")
				.arg(&state_path)
				.output()
				.unwrap(),
		);
		let final_scene = SceneData::from_json(&fs::read_to_string(&state_path).unwrap()).unwrap();
		assert_eq!(summary["commands"], command_count);
		assert_eq!(summary["mode"], "edit");
		assert_eq!(summary["fixed_steps"], 0);
		assert_eq!(summary["entities"], expected_entities);
		assert_eq!(final_scene.entities[0].name, expected_name);
		assert_eq!(
			final_scene.entities[0].transform.position,
			expected_position
		);
	}
	assert_eq!(fs::read(&fixture.scene).unwrap(), main_source);
	assert_eq!(fs::read(&secondary_path).unwrap(), secondary_source);
	assert_eq!(fs::read(&fixture.project).unwrap(), manifest_source);
}

#[test]
fn project_catalog_selects_startup_by_id_and_only_runs_that_scenes_behaviors() {
	use engine_core::{
		project::SceneBehavior,
		scene_catalog::{SceneDescriptor, SceneId},
	};

	let fixture = Fixture::new();
	let source_bytes = fs::read(&fixture.scene).unwrap();
	let mut secondary = SceneData::from_json(std::str::from_utf8(&source_bytes).unwrap()).unwrap();
	secondary.entities.truncate(1);
	secondary.entities[0].transform.position = [40.0, 0.0, 0.0];
	let secondary_path = fixture.root.join("assets/scenes/secondary.json");
	fs::write(&secondary_path, secondary.to_json().unwrap()).unwrap();
	let secondary_bytes = fs::read(&secondary_path).unwrap();
	let mut project = ProjectData::load(&fixture.project).unwrap();
	let secondary_id = SceneId::new("secondary").unwrap();
	project
		.scenes
		.insert(SceneDescriptor::new(secondary_id.clone(), "scenes/secondary.json").unwrap())
		.unwrap();
	project.behaviors.push(SceneBehavior {
		scene_id: secondary_id.clone(),
		behavior: BehaviorSpec::Spin {
			entity: EntityKey::new("actor").unwrap(),
			angular_velocity: [0.0, 2.0, 0.0],
		},
	});
	project.startup_scene_id = secondary_id;
	fs::write(&fixture.project, project.to_json().unwrap()).unwrap();
	let project_bytes = fs::read(&fixture.project).unwrap();
	let state_path = fixture.root.join("secondary-state.json");
	let summary = success(
		fixture
			.command()
			.arg("--steps")
			.arg("6")
			.arg("--state-output")
			.arg(&state_path)
			.output()
			.unwrap(),
	);
	let final_scene = SceneData::from_json(&fs::read_to_string(&state_path).unwrap()).unwrap();
	assert_eq!(summary["entities"], 1);
	assert_eq!(summary["fixed_steps"], 6);
	assert_eq!(final_scene.entities[0].transform.position, [40.0, 0.0, 0.0]);
	assert!((final_scene.entities[0].transform.rotation[1] - 0.2).abs() < 1e-5);
	assert!(
		final_scene.entities[0].transform.rotation[2].abs() < 1e-5,
		"inactive scene's spin ran against the overlapping entity key"
	);
	assert_eq!(fs::read(&fixture.scene).unwrap(), source_bytes);
	assert_eq!(fs::read(&secondary_path).unwrap(), secondary_bytes);
	assert_eq!(fs::read(&fixture.project).unwrap(), project_bytes);
}

#[test]
fn version_one_manifest_is_rejected_without_migration_or_output_files() {
	let fixture = Fixture::new();
	let legacy = json!({"schema_version":1,"name":"Legacy project","startup_scene":"scenes/main.json","fixed_hz":60,"max_catchup_steps":8,"behaviors":[]}).to_string();
	fs::write(&fixture.project, legacy.as_bytes()).unwrap();
	let scene_before = fs::read(&fixture.scene).unwrap();
	let output = fixture.root.join("summary.json");
	let state = fixture.root.join("state.json");
	let error = failure(
		fixture
			.command()
			.arg("--output")
			.arg(&output)
			.arg("--state-output")
			.arg(&state)
			.output()
			.unwrap(),
	);
	assert!(
		error["error"]["message"]
			.as_str()
			.unwrap()
			.to_ascii_lowercase()
			.contains("version"),
		"{error}"
	);
	assert!(!output.exists());
	assert!(!state.exists());
	assert_eq!(fs::read(&fixture.project).unwrap(), legacy.as_bytes());
	assert_eq!(fs::read(&fixture.scene).unwrap(), scene_before);
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
