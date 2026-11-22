use super::*;
use engine_core::project::ProjectData;
use std::path::Path;

const ACTOR: &str = "yard-player";
const CAMERA: &str = "yard-camera";
const ROUTE: &str = include_str!("../fixtures/authoring-controller/practice-yard.replay.json");

fn run_yard(scratch: &Scratch, project: &Path, actions: &[Value]) -> (Value, SceneData) {
	static NEXT: AtomicU64 = AtomicU64::new(0);
	let id = NEXT.fetch_add(1, Ordering::Relaxed);
	let path = scratch.0.join(format!("yard-replay-{id}.json"));
	let state = scratch.0.join(format!("yard-state-{id}.json"));
	fs::write(
		&path,
		serde_json::to_vec(&json!({ "schema_version": 5, "actions": actions })).unwrap(),
	)
	.unwrap();
	let output = Command::new(env!("CARGO_BIN_EXE_engine-headless"))
		.current_dir(&scratch.0)
		.arg("--project")
		.arg(project)
		.arg("--replay")
		.arg(path)
		.arg("--state-output")
		.arg(&state)
		.output()
		.unwrap();
	assert!(
		output.status.success(),
		"{}",
		String::from_utf8_lossy(&output.stderr)
	);
	let report = serde_json::from_slice(&output.stdout).unwrap();
	let state = SceneData::from_json(&fs::read_to_string(state).unwrap()).unwrap();
	(report, state)
}

fn compare_trace(source: &Value, packaged: &Value) {
	for field in [
		"camera_runtime",
		"camera_runtime_hash",
		"native_components_hash",
		"state_hash",
		"accepted_action_presses",
	] {
		assert_eq!(source[field], packaged[field], "{field}");
	}
}

fn physical_pad(left_x: i16, left_y: i16, right_x: i16, jump: bool, attack: bool) -> Value {
	json!({ "type": "physical_controller_input", "left_x": left_x, "left_y": left_y,
		"right_x": right_x, "right_y": 0, "face_buttons": [jump, false, attack, false] })
}

fn through<'a>(actions: &'a [Value], label: &str) -> &'a [Value] {
	let end = actions
		.iter()
		.position(|action| action["type"] == "mark" && action["label"] == label)
		.unwrap();
	&actions[..=end]
}

#[test]
fn practice_yard_input_asset_and_exported_scene_agree_through_play_and_restart() {
	let scratch = Scratch::new();
	let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("tests/fixtures/authoring-controller/project.json");
	let asset_root = source.parent().unwrap().join("assets");
	let project = ProjectData::load(&source).unwrap();
	let prepared = compiler::prepare_project(&project, &asset_root).unwrap();
	let stage = compiler::stage_prepared_project(&prepared, scratch.0.to_str().unwrap()).unwrap();
	let packaged = stage.path().join("project.json");
	let action_file = "assets/controls/yard.input-actions.json";
	assert_eq!(
		fs::read(source.parent().unwrap().join(action_file)).unwrap(),
		fs::read(stage.path().join(action_file)).unwrap()
	);
	let route: Value = serde_json::from_str(ROUTE).unwrap();
	let actions = route["actions"].as_array().unwrap();
	let checkpoint = |label| {
		let phase = through(actions, label);
		let (source_trace, source_scene) = run_yard(&scratch, &source, phase);
		let (export_trace, export_scene) = run_yard(&scratch, &packaged, phase);
		compare_trace(&source_trace, &export_trace);
		assert_eq!(
			source_scene.default_game_camera.as_ref().unwrap().as_str(),
			CAMERA
		);
		assert_eq!(source_scene, export_scene);
		assert_eq!(source_scene.local_player().unwrap().key.as_str(), ACTOR);
		assert_eq!(source_trace["play_scene_id"], "practice-yard");
		(source_trace, source_scene)
	};
	let (admitted, original) = checkpoint("admitted");
	let (blocked, _) = checkpoint("blocked");
	let orbit = &blocked["camera_runtime"]["orbit"];
	assert_eq!(orbit["target"]["key"], ACTOR);
	assert_eq!(orbit["obstruction"]["blocker"], "yard-wall");
	let blocked_distance = orbit["effective_distance"].as_f64().unwrap();
	assert!((0.0..3.0).contains(&blocked_distance));
	let (recovering, _) = checkpoint("recovering");
	let recovery = &recovering["camera_runtime"]["orbit"];
	assert!(recovery["obstruction"].is_null());
	assert_eq!(recovery["arm_settled"], false);
	assert!(recovery["effective_distance"].as_f64().unwrap() > blocked_distance);
	let (cleared, moved) = checkpoint("cleared");
	assert_eq!(
		cleared["camera_runtime"]["orbit"]["effective_distance"],
		5.0
	);
	assert!(entity(&moved, ACTOR).transform.position[0] > 2.5);
	let (jumping, up) = checkpoint("jumping");
	let jump = &jumping["native_components"]["characters"]["states"][ACTOR];
	assert_eq!(jump["grounded"], false);
	let takeoff = jump["jumped_at_tick"].as_u64().unwrap();
	let presses = jumping["accepted_action_presses"].as_array().unwrap();
	assert_eq!(
		presses
			.iter()
			.filter(|press| press["id"] == "jump" && press["tick"] == takeoff)
			.count(),
		1
	);
	assert_eq!(
		presses
			.iter()
			.filter(|press| press["id"] == "attack")
			.count(),
		1
	);
	assert!(entity(&up, ACTOR).transform.position[1] > 1.4);
	let (landed, stepped) = checkpoint("landed-step");
	let support = &landed["native_components"]["characters"]["states"][ACTOR];
	assert_eq!(support["grounded"], true);
	assert_eq!(support["support"], "yard-step");
	assert_eq!(support["jumped_at_tick"], takeoff);
	assert!((entity(&stepped, ACTOR).transform.position[1] - 1.3).abs() < 0.02);

	let (turned, _) = checkpoint("turned");
	assert!((turned["camera_runtime"]["orbit"]["yaw"].as_f64().unwrap() - 0.3).abs() < 1e-5);
	let (paused, held) = checkpoint("paused");
	assert_eq!(paused["simulation_tick"], turned["simulation_tick"]);
	assert_eq!(
		paused["camera_runtime"]["pose"],
		turned["camera_runtime"]["pose"]
	);
	assert_eq!(
		paused["camera_runtime"]["pending_intent"]["yaw_delta"].as_f64(),
		Some(0.0)
	);
	let (neutral, released) = checkpoint("neutral");
	assert_eq!(
		neutral["camera_runtime"]["pose"],
		turned["camera_runtime"]["pose"]
	);
	assert_eq!(
		entity(&held, ACTOR).transform,
		entity(&released, ACTOR).transform
	);
	assert_eq!(
		neutral["accepted_action_presses"],
		turned["accepted_action_presses"]
	);
	let (restarted, reset) = checkpoint("restarted");
	assert_eq!(
		restarted["camera_runtime"]["pose"],
		admitted["camera_runtime"]["pose"]
	);
	assert_eq!(entity(&reset, ACTOR), entity(&original, ACTOR));
	let (stopped, authored) = run_yard(&scratch, &packaged, actions);
	assert!(stopped["camera_runtime"].is_null());
	assert_eq!(
		authored,
		SceneData::from_json(
			&fs::read_to_string(asset_root.join("scenes/practice-yard.json")).unwrap()
		)
		.unwrap()
	);

	let mut grouped = through(actions, "turned").to_vec();
	for action in &mut grouped {
		if action["type"] == "tick" {
			let count = action["count"].as_u64().unwrap();
			assert_eq!(count % 4, 0);
			action["count"] = json!(count / 4);
			action["elapsed_seconds"] = json!(4.0 / 60.0);
		}
	}
	let (batched, _) = run_yard(&scratch, &packaged, &grouped);
	assert_eq!(batched["fixed_steps"], turned["fixed_steps"]);
	compare_trace(&turned, &batched);
}

#[test]
fn practice_yard_gamepad_replays_the_step_and_releases_look() {
	let scratch = Scratch::new();
	let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("tests/fixtures/authoring-controller/project.json");
	let route: Value = serde_json::from_str(ROUTE).unwrap();
	let keyboard = through(route["actions"].as_array().unwrap(), "landed-step");
	let (_, keyboard_state) = run_yard(&scratch, &source, keyboard);
	let mut pad: Vec<_> = keyboard
		.iter()
		.cloned()
		.map(|action| {
			if action["type"] != "keyboard_input" {
				return action;
			}
			let held = action["keys"].as_array().unwrap();
			let has = |key: &str| held.iter().any(|value| value == key);
			physical_pad(
				if has("ArrowRight") { i16::MAX } else { 0 },
				if has("ArrowUp") { i16::MAX } else { 0 },
				0,
				has("KeyJ"),
				has("KeyF"),
			)
		})
		.collect();
	let (report, state) = run_yard(&scratch, &source, &pad);
	assert_eq!(
		entity(&state, ACTOR).transform,
		entity(&keyboard_state, ACTOR).transform
	);
	assert_eq!(
		report["native_components"]["characters"]["states"][ACTOR]["support"],
		"yard-step"
	);
	pad.extend([
		physical_pad(0, 0, i16::MAX / 2, false, false),
		json!({"type":"tick", "count":24}),
	]);
	let (turning, _) = run_yard(&scratch, &source, &pad);
	let yaw = turning["camera_runtime"]["orbit"]["yaw"].as_f64().unwrap();
	assert!(
		yaw.abs() > 0.05,
		"The bound right stick did not turn the Camera"
	);
	pad.extend([
		physical_pad(0, 0, 0, false, false),
		json!({"type":"tick", "count":24}),
	]);
	let (released, released_actor) = run_yard(&scratch, &source, &pad);
	assert_eq!(
		released["camera_runtime"]["orbit"]["yaw"].as_f64(),
		Some(yaw)
	);
	assert_eq!(
		entity(&released_actor, ACTOR).transform,
		entity(&state, ACTOR).transform
	);
}
