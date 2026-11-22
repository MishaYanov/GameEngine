use serde_json::{Value, json};
use std::{
	fs,
	path::PathBuf,
	process::Command,
	sync::atomic::{AtomicU64, Ordering},
};
struct Scratch(PathBuf);
impl Scratch {
	fn new() -> Self {
		static NEXT: AtomicU64 = AtomicU64::new(0);
		let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
			.join("../../target")
			.join(format!(
				"qa-event-replay-{}-{}",
				std::process::id(),
				NEXT.fetch_add(1, Ordering::Relaxed)
			));
		fs::create_dir(&path).unwrap();
		Self(path)
	}
}
impl Drop for Scratch {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.0);
	}
}
fn run(replay: &std::path::Path) -> Value {
	let fixture = test_project::ProjectFixture::new("event-lab");
	let project = fixture.manifest_path();
	let output = Command::new(env!("CARGO_BIN_EXE_engine-headless"))
		.arg("--project")
		.arg(project)
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
fn normalized(mut value: Value) -> Value {
	value["play_session_id"] = Value::Null;
	value["run_id"] = Value::Null;
	for entity in value["entities"].as_array_mut().unwrap() {
		entity["handle"]["world_id"] = Value::Null;
	}
	value
}
fn machine<'a>(value: &'a Value, key: &str) -> &'a Value {
	&value["runtime"]["entities"]
		.as_array()
		.unwrap()
		.iter()
		.find(|e| e["handle"]["key"] == key)
		.unwrap()["behavior_states"][0]["machine_state"]
}
#[test]
fn event_route_replays_deterministically_and_restart_removes_pending_state() {
	let temp = Scratch::new();
	let replay = temp.0.join("route.json");
	let mut actions = vec![
		json!({"type":"command","command":{"type":"start_play"}}),
		json!({"type":"command","command":{"type":"set_play_input","movement":[0,0,-1]}}),
		json!({"type":"tick","count":180}),
		json!({"type":"command","command":{"type":"set_play_input","movement":[0,0,0]}}),
		json!({"type":"tick","count":60}),
	];
	fs::write(
		&replay,
		serde_json::to_vec(&json!({"schema_version":1,"actions":actions})).unwrap(),
	)
	.unwrap();
	let a = run(&replay);
	let b = run(&replay);
	assert_eq!(a["state_hash"], b["state_hash"]);
	assert_eq!(a["state_hash_scope"], "project_and_render_scene");
	assert_eq!(a["native_components_hash_scope"], "native_components_v1");
	assert_eq!(a["native_components_hash_algorithm"], "fnv1a64");
	assert_eq!(a["native_components_hash"].as_str().unwrap().len(), 16);
	assert_eq!(a["native_components_hash"], b["native_components_hash"]);
	assert_eq!(a["native_components"], b["native_components"]);
	assert_eq!(
		a["native_components"]["lifecycle"]["machines"]
			.as_array()
			.unwrap()
			.len(),
		10
	);
	assert_eq!(a["native_components"]["lifecycle"]["events"]["sequence"], 8);
	assert_eq!(
		normalized(a["runtime"].clone()),
		normalized(b["runtime"].clone())
	);
	assert_eq!(machine(&a, "a-complete"), "done");
	assert_eq!(machine(&a, "a-pickup"), "spent");
	assert_eq!(machine(&a, "a-door"), "closed");
	assert_eq!(machine(&a, "a-left"), "ready");
	assert_eq!(machine(&a, "b-complete"), "waiting");
	assert_eq!(a["runtime"]["pending_events"], 0);
	assert_eq!(a["runtime"]["event_sequence"], 8);
	assert_eq!(a["runtime"]["machine_deadlines"], 0);
	actions.push(json!({"type":"restart_run"}));
	fs::write(
		&replay,
		serde_json::to_vec(&json!({"schema_version":1,"actions":actions})).unwrap(),
	)
	.unwrap();
	let fresh = run(&replay);
	assert_eq!(fresh["runtime"]["tick"], 0);
	assert_eq!(fresh["runtime"]["pending_events"], 0);
	assert_eq!(fresh["runtime"]["event_sequence"], 0);
	assert_ne!(a["native_components_hash"], fresh["native_components_hash"]);
	assert_eq!(
		fresh["native_components"]["lifecycle"]["events"]["sequence"],
		0
	);
	assert!(
		fresh["native_components"]["lifecycle"]["machines"]
			.as_array()
			.unwrap()
			.iter()
			.all(|m| m["initialized"] == false)
	);
	assert_eq!(machine(&fresh, "a-left"), &Value::Null);
}

#[test]
fn native_projection_contains_all_machines_beyond_the_inspection_page() {
	let temp = Scratch::new();
	let path = temp.0.join("scene.json");
	let mut data: Value =
		serde_json::from_str(include_str!("../../game/assets/scenes/event-lab.json")).unwrap();
	for index in 0..150 {
		data["entities"].as_array_mut().unwrap().push(json!({"key":format!("extra-{index}"),"name":"Extra machine","transform":{"position":[20,0,0],"rotation":[0,0,0],"scale":[1,1,1]},"components":[{"id":"logic","schema_version":1,"enabled":true,"data":{"kind":"native_behavior","behavior":{"kind":"state_machine","definition":{"schema_version":1,"initial":"ready","inputs":[],"states":[{"id":"ready","on_enter":[],"transitions":[]}]}}}}]}));
	}
	fs::write(&path, serde_json::to_vec(&data).unwrap()).unwrap();
	let output = Command::new(env!("CARGO_BIN_EXE_engine-headless"))
		.arg("--scene")
		.arg(path)
		.args(["--steps", "1"])
		.output()
		.unwrap();
	assert!(
		output.status.success(),
		"{}",
		String::from_utf8_lossy(&output.stderr)
	);
	let result: Value = serde_json::from_slice(&output.stdout).unwrap();
	assert_eq!(result["runtime"]["entities"].as_array().unwrap().len(), 128);
	assert_eq!(
		result["native_components"]["lifecycle"]["machines"]
			.as_array()
			.unwrap()
			.len(),
		160
	);
	assert_eq!(
		result["native_components"]["lifecycle"]["events"]["pending"]
			.as_array()
			.unwrap()
			.len(),
		1
	);
}

#[path = "../../../tests/support/project_fixture.rs"]
mod test_project;
