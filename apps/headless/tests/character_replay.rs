use serde_json::{Value, json};
use std::{fs, path::PathBuf, process::Command};

#[test]
fn character_fixture_replays_jump_and_restart_with_stable_motion() {
	let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
	let directory = root
		.join("target")
		.join(format!("qa-character-replay-{}", std::process::id()));
	fs::create_dir_all(&directory).unwrap();
	let replay = directory.join("replay.json");
	let mut actions = vec![
		json!({"type":"command","command":{"type":"start_play"}}),
		json!({"type":"tick","count":1}),
		json!({"type":"command","command":{"type":"set_character_input","key":"player","movement_xz":[1,0],"jump_held":true}}),
		json!({"type":"command","command":{"type":"set_character_input","key":"player","movement_xz":[1,0],"jump_held":false}}),
		json!({"type":"tick","count":30}),
		json!({"type":"command","command":{"type":"set_character_input","key":"player","movement_xz":[0,0],"jump_held":false}}),
		json!({"type":"tick","count":90}),
	];
	fn run(project: &std::path::Path, replay: &std::path::Path) -> Value {
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
	fs::write(
		&replay,
		serde_json::to_vec(&json!({"schema_version":1,"actions":actions})).unwrap(),
	)
	.unwrap();
	let project = root.join("apps/game/controller-demo.json");
	let a = run(&project, &replay);
	let b = run(&project, &replay);
	assert_eq!(a["native_components_hash_scope"], "native_components_v1");
	assert_eq!(a["native_components_hash"].as_str().unwrap().len(), 16);
	assert_eq!(a["native_components_hash"], b["native_components_hash"]);
	assert_eq!(a["native_components"], b["native_components"]);
	assert_eq!(
		a["native_components"]["characters"]["states"]["player"]["jumped_at_tick"],
		2
	);
	let first = &a["runtime"]["character_motion"][0];
	assert_eq!(first["jumped_at_tick"], 2);
	assert_eq!(first["grounded"], true);
	assert!(first["landed_at_tick"].as_u64().unwrap() > 2);
	for field in [
		"tick",
		"grounded",
		"planar_speed",
		"vertical_speed",
		"jumped_at_tick",
		"landed_at_tick",
	] {
		assert_eq!(first[field], b["runtime"]["character_motion"][0][field]);
	}
	actions.push(json!({"type":"restart_run"}));
	fs::write(
		&replay,
		serde_json::to_vec(&json!({"schema_version":1,"actions":actions})).unwrap(),
	)
	.unwrap();
	let restarted = run(&project, &replay);
	let motion = &restarted["runtime"]["character_motion"][0];
	assert_eq!(motion["tick"], 0);
	assert!(motion["jumped_at_tick"].is_null());
	assert!(motion["landed_at_tick"].is_null());
	assert_eq!(motion["vertical_speed"], 0.0);
	assert_ne!(
		a["native_components_hash"],
		restarted["native_components_hash"]
	);
	assert!(
		restarted["native_components"]["characters"]["states"]["player"]["jumped_at_tick"]
			.is_null()
	);
	fs::remove_dir_all(directory).unwrap();
}
