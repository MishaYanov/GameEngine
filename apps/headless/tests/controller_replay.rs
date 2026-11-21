use engine_core::{SceneData, scene_data::EntityData};
use serde_json::{Value, json};
use std::{
	fs,
	path::PathBuf,
	process::{Command, Output},
	sync::atomic::{AtomicU64, Ordering},
};

struct Scratch(PathBuf);

impl Scratch {
	fn new() -> Self {
		static NEXT: AtomicU64 = AtomicU64::new(0);
		let root = std::env::temp_dir().join(format!(
			"engine-controller-replay-{}-{}",
			std::process::id(),
			NEXT.fetch_add(1, Ordering::Relaxed)
		));
		fs::create_dir(&root).unwrap();
		Self(root)
	}

	fn invoke(&self, project: &str, replay: &Value) -> Output {
		let project = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
			.join("../game")
			.join(project)
			.canonicalize()
			.unwrap();
		let path = self.0.join("replay.json");
		fs::write(&path, serde_json::to_vec(replay).unwrap()).unwrap();
		Command::new(env!("CARGO_BIN_EXE_engine-headless"))
			.current_dir(&self.0)
			.arg("--project")
			.arg(project)
			.arg("--replay")
			.arg(path)
			.arg("--state-output")
			.arg(self.0.join("state.json"))
			.output()
			.unwrap()
	}
}

impl Drop for Scratch {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.0);
	}
}

const COURTYARD: &str = "project.release-learning.json";

fn run(project: &str, actions: &[Value]) -> (Value, SceneData) {
	let scratch = Scratch::new();
	let output = scratch.invoke(project, &json!({ "schema_version": 4, "actions": actions }));
	assert!(
		output.status.success(),
		"{}",
		String::from_utf8_lossy(&output.stderr)
	);
	let report = serde_json::from_slice(&output.stdout).unwrap();
	let state =
		SceneData::from_json(&fs::read_to_string(scratch.0.join("state.json")).unwrap()).unwrap();
	(report, state)
}

fn entity<'a>(scene: &'a SceneData, key: &str) -> &'a EntityData {
	scene
		.entities
		.iter()
		.find(|entity| entity.key.as_str() == key)
		.unwrap()
}

fn pad(x: i16) -> Value {
	json!({ "type": "controller_input", "left_x": x, "left_y": 0, "jump": false, "interact": false })
}

fn keys(keys: &[&str]) -> Value {
	json!({ "type": "keyboard_input", "keys": keys })
}

fn start() -> Value {
	json!({ "type": "command", "command": { "type": "start_play" } })
}

#[test]
fn controller_moves_proportionally_and_keeps_the_other_device_snapshot() {
	let tick = json!({ "type": "tick", "count": 30 });
	let position = |actions: &[Value]| {
		let (_, state) = run(COURTYARD, actions);
		entity(&state, "courtyard-player").transform.position[0]
	};
	let distance = |axis| position(&[start(), pad(axis), tick.clone()]);
	assert_eq!(distance(3000), 0.0);
	let partial = distance(16384);
	let full = distance(i16::MAX);
	assert!((partial - 0.1875).abs() < 0.001);
	assert!((full - 0.5).abs() < 0.001);

	let mut mixed = vec![start(), keys(&["KeyD"]), pad(i16::MIN), tick.clone()];
	assert!((position(&mixed) - full).abs() < 0.001);
	mixed.extend([keys(&[]), tick.clone()]);
	assert!(position(&mixed).abs() < 0.001);

	for sample_after_restart in [keys(&[]), pad(0)] {
		assert_eq!(
			position(&[
				start(),
				keys(&["KeyD"]),
				pad(i16::MAX),
				tick.clone(),
				json!({ "type": "restart_run" }),
				sample_after_restart,
				tick.clone(),
			]),
			0.0
		);
	}
}

#[test]
fn a_controller_tap_opens_the_courtyard_gate_and_finishes_the_route() {
	let replay: Value =
		serde_json::from_str(include_str!("fixtures/controller/courtyard.replay.json")).unwrap();
	let actions = replay["actions"].as_array().unwrap();
	let (_, closed) = run(COURTYARD, &actions[..5]);
	let (_, open) = run(COURTYARD, &actions[..8]);
	let gate = |state: &SceneData| {
		let gate = entity(state, "courtyard-route-gate");
		(gate.renderable, gate.collision_enabled)
	};
	assert_eq!(gate(&closed), (true, true));
	assert_eq!(gate(&open), (false, false));
	let (completed, _) = run(COURTYARD, actions);
	assert_eq!(completed["play_scene_id"], "finish");
	assert_eq!(completed["play_status"]["gameplay"]["completed"], true);
	assert_eq!(completed["play_status"]["gameplay"]["score"], 1);
}

#[test]
fn a_held_gamepad_jump_takes_off_once() {
	let replay: Value =
		serde_json::from_str(include_str!("fixtures/controller/jump.replay.json")).unwrap();
	let (report, _) = run("camera-demo.json", replay["actions"].as_array().unwrap());
	let player = &report["native_components"]["characters"]["states"]["player"];
	assert_eq!(player["jumped_at_tick"], 2);
	assert_eq!(player["grounded"], true);
	assert!(player["landed_at_tick"].as_u64().unwrap() > 2);
}

#[test]
fn controller_replay_requires_version_four_and_signed_16_bit_axes() {
	for (version, axis, error) in [
		(3, 0, "controller_input requires replay schema version 4"),
		(4, 32768, "Invalid replay"),
	] {
		let scratch = Scratch::new();
		let sample = json!({ "type": "controller_input", "left_x": axis, "left_y": 0, "jump": false, "interact": false });
		let output = scratch.invoke(
			COURTYARD,
			&json!({ "schema_version": version, "actions": [start(), sample] }),
		);
		assert!(!output.status.success());
		assert!(String::from_utf8_lossy(&output.stderr).contains(error));
		assert!(!scratch.0.join("state.json").exists());
	}
}
