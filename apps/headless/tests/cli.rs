use engine_core::{
	EntityKey,
	project::{BehaviorSpec, ProjectData, SceneBehavior},
	scene_catalog::{SceneDescriptor, SceneId},
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
				follow: None,
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
	fn add_finish(&self, project: &mut ProjectData, source: &SceneData) -> SceneData {
		let id = SceneId::new("finish").unwrap();
		project
			.scenes
			.insert(SceneDescriptor::new(id.clone(), "scenes/finish.json").unwrap())
			.unwrap();
		let player = EntityKey::new("finish-player").unwrap();
		let marker = EntityKey::new("finish-marker").unwrap();
		let mut finish = source.clone();
		finish.bindings = [
			("player".into(), player.clone()),
			("finish".into(), marker.clone()),
		]
		.into();
		finish.entities = vec![
			EntityData::new(
				player.clone(),
				"Destination player",
				"models/not-loaded.glb",
			),
			EntityData::new(marker, "Finish marker", "models/not-loaded.glb").with_transform(
				TransformData {
					position: [1.0, 0.0, 0.0],
					..Default::default()
				},
			),
		];
		project.behaviors.push(SceneBehavior {
			scene_id: id,
			behavior: BehaviorSpec::ControlledMove {
				entity: player,
				speed: 2.0,
				space: Default::default(),
			},
		});
		fs::write(
			self.root.join("assets/scenes/finish.json"),
			finish.to_json().unwrap(),
		)
		.unwrap();
		finish
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
fn bundled_route_reports_locked_exit_key_completion_restart_and_restored_authored_state() {
	let fixture = Fixture::new();
	let sample = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../game");
	let manifest = sample.join("project.json");
	let project = ProjectData::load(&manifest).unwrap();
	let source = sample
		.join("assets")
		.join(project.startup_scene_path().unwrap());
	let source_bytes = fs::read(&source).unwrap();
	let authored = SceneData::from_json(std::str::from_utf8(&source_bytes).unwrap()).unwrap();
	let mut actions = vec![json!({"type":"command","command":{"type":"start_play"}})];
	for (index, movement, count, scene_id, score, objective, completed) in [
		(0, -1.0, 90, "courtyard", 0, "Find the courtyard key", false),
		(
			1,
			1.0,
			190,
			"courtyard",
			1,
			"Reach the courtyard exit",
			false,
		),
		// The locked door now stops the actor at contact; leave enough travel
		// to return from the pickup and sample the authored exit sensor.
		(2, -1.0, 240, "finish", 1, "Reach the finish marker", false),
		(3, 1.0, 90, "finish", 1, "Courtyard Run complete", true),
	] {
		actions.extend([
			json!({"type":"command","command":{"type":"set_play_input","movement":[movement,0,0]}}),
			json!({"type":"tick","count":count}),
		]);
		let replay = fixture.replay(actions.clone());
		let summary = success(
			Command::new(env!("CARGO_BIN_EXE_engine-headless"))
				.current_dir(&fixture.root)
				.arg("--project")
				.arg(&manifest)
				.arg("--replay")
				.arg(replay)
				.output()
				.unwrap(),
		);
		assert_eq!(summary["active_scene_id"], "courtyard", "phase {index}");
		assert_eq!(summary["play_scene_id"], scene_id);
		assert_eq!(summary["play_status"]["scene_id"], scene_id);
		assert_eq!(summary["play_status"]["gameplay"]["score"], score);
		assert_eq!(summary["play_status"]["gameplay"]["objective"], objective);
		assert_eq!(summary["play_status"]["gameplay"]["completed"], completed);
	}
	actions.extend([
		json!({"type":"restart_run"}),
		json!({"type":"tick","count":5}),
	]);
	let replay = fixture.replay(actions.clone());
	let state = fixture.root.join("restarted.json");
	let run = |replay: &PathBuf, state: &PathBuf| {
		success(
			Command::new(env!("CARGO_BIN_EXE_engine-headless"))
				.current_dir(&fixture.root)
				.arg("--project")
				.arg(&manifest)
				.arg("--replay")
				.arg(replay)
				.arg("--state-output")
				.arg(state)
				.output()
				.unwrap(),
		)
	};
	let restarted = run(&replay, &state);
	assert_eq!(restarted["play_scene_id"], "courtyard");
	assert_eq!(restarted["play_status"]["gameplay"]["score"], 0);
	assert_eq!(restarted["play_status"]["gameplay"]["completed"], false);
	assert_eq!(
		restarted["play_status"]["gameplay"]["objective"],
		"Find the courtyard key"
	);
	assert_eq!(
		restarted["simulation_tick"], 1,
		"Restart clears held input and samples fresh sensors once"
	);
	assert_eq!(
		SceneData::from_json(&fs::read_to_string(state).unwrap()).unwrap(),
		authored
	);
	actions.push(json!({"type":"command","command":{"type":"stop_play"}}));
	let replay = fixture.replay(actions);
	let state = fixture.root.join("restored.json");
	let stopped = run(&replay, &state);
	assert_eq!(stopped["mode"], "edit");
	assert!(stopped.get("play_status").is_none());
	assert_eq!(
		SceneData::from_json(&fs::read_to_string(state).unwrap()).unwrap(),
		authored
	);
	assert_eq!(fs::read(&source).unwrap(), source_bytes);
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
fn headless_collects_the_bound_key_without_input_only_when_the_campaign_is_selected() {
	let fixture = Fixture::new();
	let mut scene = SceneData::from_json(&fs::read_to_string(&fixture.scene).unwrap()).unwrap();
	let key = EntityKey::new("key-marker").unwrap();
	scene
		.entities
		.push(EntityData::new(key.clone(), "Key", "models/not-loaded.glb"));
	scene.bindings.extend([
		("player".into(), EntityKey::new("actor").unwrap()),
		("key".into(), key),
		("exit".into(), EntityKey::new("other").unwrap()),
	]);
	let document = scene.to_json().unwrap();
	fs::write(&fixture.scene, &document).unwrap();

	let mut project = ProjectData::load(&fixture.project).unwrap();
	project.behaviors.truncate(1);
	fs::write(&fixture.project, project.to_json().unwrap()).unwrap();
	let run = || success(fixture.command().arg("--steps").arg("2").output().unwrap());
	assert_eq!(run()["fixed_steps"], 0);

	// The player spawns next to the key. With movement idle and the exit far
	// away, the pickup is the only reason to step.
	project.gameplay_module = Some(courtyard_gameplay::COURTYARD_RUN_ID.into());
	fixture.add_finish(&mut project, &scene);
	fs::write(&fixture.project, project.to_json().unwrap()).unwrap();
	let selected = run();
	let repeated = run();
	assert_eq!(selected["fixed_steps"], 1);
	assert_eq!(selected["simulation_tick"], 1);
	assert_eq!(selected["state_hash"], repeated["state_hash"]);
	assert_eq!(selected["change_cursor"], repeated["change_cursor"]);
	assert_eq!(fs::read_to_string(&fixture.scene).unwrap(), document);
}

#[test]
fn headless_run_reaches_finish_and_stop_restores_authored_scene() {
	let fixture = Fixture::new();
	let mut scene = SceneData::from_json(&fs::read_to_string(&fixture.scene).unwrap()).unwrap();
	scene.entities[1].transform.position = [0.0; 3];
	let key = EntityKey::new("key-marker").unwrap();
	scene
		.entities
		.push(EntityData::new(key.clone(), "Key", "models/not-loaded.glb"));
	scene.bindings.extend([
		("player".into(), EntityKey::new("actor").unwrap()),
		("key".into(), key),
		("exit".into(), EntityKey::new("other").unwrap()),
	]);
	let mut project = ProjectData::load(&fixture.project).unwrap();
	project.behaviors.truncate(1);
	project.gameplay_module = Some(courtyard_gameplay::COURTYARD_RUN_ID.into());
	let finish = fixture.add_finish(&mut project, &scene);
	let authored = scene.to_json().unwrap();
	fs::write(&fixture.scene, &authored).unwrap();
	fs::write(&fixture.project, project.to_json().unwrap()).unwrap();
	let mut actions = vec![
		json!({"type":"command","command":{"type":"start_play"}}),
		json!({"type":"command","command":{"type":"set_play_input","movement":[1,0,0]}}),
		json!({"type":"tick","count":10}),
	];
	let replay = fixture.replay(actions.clone());
	let state = fixture.root.join("destination.json");
	let result = success(
		fixture
			.command()
			.arg("--replay")
			.arg(&replay)
			.arg("--state-output")
			.arg(&state)
			.output()
			.unwrap(),
	);
	assert_eq!(result["active_scene_id"], project.startup_scene_id.as_str());
	assert_eq!(result["play_scene_id"], "finish");
	assert_eq!(result["fixed_steps"], 2);
	assert_eq!(result["simulation_tick"], 0);
	let rendered = SceneData::from_json(&fs::read_to_string(&state).unwrap()).unwrap();
	assert_eq!(
		rendered, finish,
		"old held input must not move the destination player"
	);
	assert_eq!(fs::read_to_string(&fixture.scene).unwrap(), authored);

	actions.extend([
		json!({"type":"command","command":{"type":"set_play_input","movement":[1,0,0]}}),
		json!({"type":"tick","count":90}),
	]);
	let replay = fixture.replay(actions.clone());
	let completed_state = fixture.root.join("completed.json");
	let completed = success(
		fixture
			.command()
			.arg("--replay")
			.arg(&replay)
			.arg("--state-output")
			.arg(&completed_state)
			.output()
			.unwrap(),
	);
	assert_eq!(
		completed["active_scene_id"],
		project.startup_scene_id.as_str()
	);
	assert_eq!(completed["play_scene_id"], "finish");
	// Twenty movement steps enter marker range, then one completion step
	// commits without moving. The remaining held-input ticks all stay idle.
	assert_eq!(completed["simulation_tick"], 21);
	assert_eq!(completed["fixed_steps"], 23);
	let rendered = SceneData::from_json(&fs::read_to_string(completed_state).unwrap()).unwrap();
	let player_x = rendered.entities[0].transform.position[0];
	assert!((player_x - 2.0 / 3.0).abs() < 1e-5, "{player_x}");
	assert_eq!(rendered.bindings, finish.bindings);

	// Background gameplay must not make the editor's Stop revision stale.
	actions.push(json!({"type":"command","expected_revision":3,"command":{"type":"stop_play"}}));
	let replay = fixture.replay(actions);
	let stopped_state = fixture.root.join("stopped.json");
	let stopped = success(
		fixture
			.command()
			.arg("--replay")
			.arg(&replay)
			.arg("--state-output")
			.arg(&stopped_state)
			.output()
			.unwrap(),
	);
	assert_eq!(stopped["mode"], "edit");
	assert!(stopped.get("play_scene_id").is_none());
	assert_eq!(fs::read_to_string(&fixture.scene).unwrap(), authored);
	assert_eq!(
		SceneData::from_json(
			&fs::read_to_string(fixture.root.join("assets/scenes/finish.json")).unwrap()
		)
		.unwrap(),
		finish,
	);
	assert_eq!(
		SceneData::from_json(&fs::read_to_string(stopped_state).unwrap()).unwrap(),
		scene
	);
}

#[test]
fn headless_can_open_a_course_draft_but_rejects_play_without_its_exit_role() {
	let fixture = Fixture::new();
	let mut scene = SceneData::from_json(&fs::read_to_string(&fixture.scene).unwrap()).unwrap();
	scene.bindings.extend([
		("player".into(), EntityKey::new("actor").unwrap()),
		("key".into(), EntityKey::new("other").unwrap()),
	]);
	fs::write(&fixture.scene, scene.to_json().unwrap()).unwrap();
	let mut project = ProjectData::load(&fixture.project).unwrap();
	project.gameplay_module = Some(courtyard_gameplay::COURTYARD_RUN_ID.into());
	fs::write(&fixture.project, project.to_json().unwrap()).unwrap();

	let opened = success(fixture.command().arg("--steps").arg("0").output().unwrap());
	assert_eq!(opened["mode"], "edit");
	let error = failure(fixture.command().arg("--steps").arg("1").output().unwrap());
	assert!(
		error["error"]["message"]
			.as_str()
			.unwrap()
			.contains(r#"requires a "exit" binding"#)
	);
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
