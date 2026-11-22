use serde_json::{Value, json};
use std::{fs, path::PathBuf, process::Command};

fn workspace() -> PathBuf {
	PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
struct Scratch(PathBuf);
impl Scratch {
	fn new(label: &str) -> Self {
		let path = workspace().join("target").join(format!(
			"qa-headless-runtime-{label}-{}",
			std::process::id()
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
fn run(arguments: &[&std::ffi::OsStr]) -> Value {
	let output = Command::new(env!("CARGO_BIN_EXE_engine-headless"))
		.args(arguments)
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
fn repeated_spawn_collect_replay_has_stable_outcome_and_restart_clears_lifecycle() {
	let temp = Scratch::new("replay");
	let replay = temp.0.join("replay.json");
	let mut actions = vec![
		json!({"type":"command","command":{"type":"start_play"}}),
		json!({"type":"command","command":{"type":"set_play_input","movement":[0,0,-1]}}),
		json!({"type":"tick","count":45}),
		json!({"type":"command","command":{"type":"set_play_input","movement":[0,0,0]}}),
		json!({"type":"tick","count":1200}),
	];
	fs::write(
		&replay,
		serde_json::to_vec(&json!({"schema_version":1,"actions":actions})).unwrap(),
	)
	.unwrap();
	let fixture = test_project::ProjectFixture::new("component-lab");
	let project = fixture.manifest_path();
	let args = [
		"--project".as_ref(),
		project.as_os_str(),
		"--replay".as_ref(),
		replay.as_os_str(),
	];
	let a = run(&args);
	let b = run(&args);
	assert_eq!(a["state_hash"], b["state_hash"]);
	for key in [
		"topology_revision",
		"total_entities",
		"timer_count",
		"pending_spawns",
		"last_error",
		"tick",
	] {
		assert_eq!(a["runtime"][key], b["runtime"][key]);
	}
	assert_eq!(a["runtime"]["topology_revision"], 20);
	assert_eq!(a["runtime"]["total_entities"], 6);
	assert_eq!(a["runtime"]["timer_count"], 0);
	assert!(a["runtime"]["last_error"].is_null());
	actions.push(json!({"type":"restart_run"}));
	fs::write(
		&replay,
		serde_json::to_vec(&json!({"schema_version":1,"actions":actions})).unwrap(),
	)
	.unwrap();
	let restarted = run(&args);
	assert_eq!(restarted["runtime"]["tick"], 0);
	assert_eq!(restarted["runtime"]["topology_revision"], 0);
	assert_eq!(restarted["runtime"]["pending_spawns"], 0);
	assert!(
		restarted["runtime"]["entities"]
			.as_array()
			.unwrap()
			.iter()
			.all(|entity| entity["spawned"] == false)
	);
}

#[test]
fn failed_prefab_load_preserves_the_world_and_exposes_a_bounded_runtime_error() {
	let temp = Scratch::new("failure");
	let scene = temp.0.join("scene.json");
	fs::copy(
		workspace().join("apps/game/assets/scenes/component-lab.json"),
		&scene,
	)
	.unwrap();
	let output = run(&[
		"--scene".as_ref(),
		scene.as_os_str(),
		"--asset-root".as_ref(),
		temp.0.as_os_str(),
		"--steps".as_ref(),
		"120".as_ref(),
	]);
	assert_eq!(output["runtime"]["total_entities"], 6);
	assert_eq!(output["runtime"]["topology_revision"], 0);
	assert_eq!(output["runtime"]["pending_spawns"], 0);
	let error = output["runtime"]["last_error"].as_str().unwrap();
	assert!(!error.is_empty());
	assert!(error.chars().count() <= 512);
}

#[test]
fn host_drains_spawn_boundaries_before_accepting_the_next_fixed_step() {
	let temp = Scratch::new("partition");
	let scene = temp.0.join("scene.json");
	let mut data: Value = serde_json::from_slice(
		&fs::read(workspace().join("apps/game/assets/scenes/component-lab.json")).unwrap(),
	)
	.unwrap();
	let spawner = &mut data["entities"][5];
	spawner["transform"]["position"] = json!([0, 0, 0]);
	spawner["components"][0]["data"]["behavior"]["interval_ticks"] = json!(1);
	fs::write(&scene, serde_json::to_vec(&data).unwrap()).unwrap();
	fs::create_dir(temp.0.join("prefabs")).unwrap();
	fs::copy(
		workspace().join("apps/game/assets/prefabs/timed-collectible.json"),
		temp.0.join("prefabs/timed-collectible.json"),
	)
	.unwrap();
	let combined = run(&[
		"--scene".as_ref(),
		scene.as_os_str(),
		"--asset-root".as_ref(),
		temp.0.as_os_str(),
		"--steps".as_ref(),
		"1".as_ref(),
		"--dt".as_ref(),
		"0.06666666666666667".as_ref(),
	]);
	let split = run(&[
		"--scene".as_ref(),
		scene.as_os_str(),
		"--asset-root".as_ref(),
		temp.0.as_os_str(),
		"--steps".as_ref(),
		"4".as_ref(),
	]);
	assert_eq!(combined["state_hash"], split["state_hash"]);
	assert_eq!(combined["fixed_steps"], 4);
	assert_eq!(combined["runtime"]["topology_revision"], 7);
	assert_eq!(
		combined["runtime"]["topology_revision"],
		split["runtime"]["topology_revision"]
	);
	assert_eq!(combined["dropped_seconds"], 0.0);
}

#[path = "../../../tests/support/project_fixture.rs"]
mod test_project;
