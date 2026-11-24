use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

#[test]
fn replay_switches_between_two_characters_and_restarts_the_authored_owner() {
	let fixture = test_project::ProjectFixture::new("controller-room");
	let manifest = fixture.manifest_path();
	let root = manifest.parent().unwrap();
	let scene = root.join("assets/scenes/controller-room.json");
	let mut source: Value = serde_json::from_slice(&fs::read(&scene).unwrap()).unwrap();
	source["schema_version"] = json!(7);
	let entities = source["entities"].as_array_mut().unwrap();
	let first = entities
		.iter_mut()
		.find(|actor| actor["key"] == "player")
		.unwrap();
	let controller = |local| {
		json!({
			"id": "player-controller", "schema_version": 1, "enabled": true,
			"data": {
				"kind": "player_controller",
				"settings": {"input": {"kind": "project_default"}, "space": "world", "local": local}
			}
		})
	};
	let mut second = first.clone();
	first["components"]
		.as_array_mut()
		.unwrap()
		.push(controller(true));
	second["key"] = json!("second");
	second["name"] = json!("Second Character");
	second["transform"]["position"][0] = json!(-5.0);
	second["components"]
		.as_array_mut()
		.unwrap()
		.push(controller(false));
	entities.push(second);
	let original = serde_json::to_vec(&source).unwrap();
	fs::write(&scene, &original).unwrap();

	let replay = root.join("possession.replay.json");
	let mut actions = vec![
		json!({"type":"command","command":{"type":"start_play"}}),
		json!({"type":"tick"}),
		json!({"type":"keyboard_input","keys":["KeyD"]}),
		json!({"type":"tick","count":3}),
		json!({"type":"set_local_possession","target":"second"}),
		json!({"type":"tick"}),
		json!({"type":"keyboard_input","keys":["Space"]}),
		json!({"type":"tick"}),
		json!({"type":"keyboard_input","keys":[]}),
		json!({"type":"keyboard_input","keys":["Space"]}),
		json!({"type":"tick","count":10}),
	];
	let run = |version, steps: &[Value]| {
		fs::write(
			&replay,
			json!({"schema_version":version,"actions":steps}).to_string(),
		)
		.unwrap();
		run_replay(&manifest, &replay)
	};

	let older = run(5, &actions);
	assert!(!older.status.success());
	let stderr = String::from_utf8_lossy(&older.stderr);
	assert!(
		stderr.contains("requires replay schema version 6"),
		"{stderr}"
	);

	let first = read(run(6, &actions));
	let repeated = read(run(6, &actions));
	assert_eq!(
		first["native_components_hash"],
		repeated["native_components_hash"]
	);
	assert_eq!(first["state_hash"], repeated["state_hash"]);
	assert_eq!(first["runtime"]["local_player"]["key"], "second");
	assert_eq!(first["runtime"]["possession_generation"], 1);
	assert_eq!(
		first["native_components"]["local_possession"]["owner"],
		"second"
	);
	assert!(
		first["native_components"]["characters"]["states"]["player"]["jumped_at_tick"].is_null()
	);
	assert_eq!(
		first["native_components"]["characters"]["states"]["second"]["jumped_at_tick"],
		7
	);

	actions.push(json!({"type":"restart_run"}));
	let restarted = read(run(6, &actions));
	assert_eq!(restarted["runtime"]["local_player"]["key"], "player");
	assert!(restarted["runtime"]["possession_generation"].is_null());
	assert!(
		restarted["native_components"]["characters"]["states"]["second"]["jumped_at_tick"]
			.is_null()
	);
	assert_eq!(fs::read(scene).unwrap(), original);
}

fn run_replay(project: &Path, replay: &Path) -> std::process::Output {
	Command::new(env!("CARGO_BIN_EXE_engine-headless"))
		.arg("--project")
		.arg(project)
		.arg("--replay")
		.arg(replay)
		.output()
		.unwrap()
}

fn read(output: std::process::Output) -> Value {
	assert!(
		output.status.success(),
		"{}",
		String::from_utf8_lossy(&output.stderr)
	);
	serde_json::from_slice(&output.stdout).unwrap()
}

#[path = "../../../tests/support/project_fixture.rs"]
mod test_project;
