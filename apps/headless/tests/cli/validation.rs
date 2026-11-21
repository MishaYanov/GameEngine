use super::*;

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
