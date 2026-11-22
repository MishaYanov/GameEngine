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

#[test]
fn audio_catalog_is_validated_before_headless_reports_are_published() {
	const KEY_CLIP: &[u8] =
		include_bytes!("../../../game/fixtures/courtyard-audio/assets/audio/cue-key.wav");
	let fixture = Fixture::new();
	let initial = success(fixture.command().arg("--steps").arg("0").output().unwrap());
	let audio = fixture.root.join("assets/audio");
	fs::create_dir(&audio).unwrap();
	fs::write(
		audio.join("cues.json"),
		br#"{"schema_version":1,"cues":[{"event":"key_collected","path":"audio/cue-key.wav"}]}"#,
	)
	.unwrap();
	fs::write(audio.join("cue-key.wav"), KEY_CLIP).unwrap();
	let valid = success(fixture.command().arg("--steps").arg("0").output().unwrap());
	assert_eq!(valid["state_hash"], initial["state_hash"]);
	assert_eq!(valid["fixed_steps"], initial["fixed_steps"]);

	let cue_manifest = audio.join("cues.json");
	let project = ProjectData::load(&fixture.project).unwrap();
	let variant = |scene: &str| {
		json!({
			"schema_version": 2,
			"cues": [{"event":"key_collected","path":"audio/cue-key.wav"}],
			"bus_percent": {"collection": 60},
			"scene_bus_percent": { (scene): {"collection": 20} },
		})
		.to_string()
	};
	fs::write(&cue_manifest, variant(project.startup_scene_id.as_str())).unwrap();
	let mixed = success(fixture.command().arg("--steps").arg("0").output().unwrap());
	assert_eq!(mixed["state_hash"], initial["state_hash"]);

	let output = fixture.root.join("report.json");
	fs::write(&cue_manifest, variant("unknown")).unwrap();
	let missing_scene = failure(
		fixture
			.command()
			.arg("--output")
			.arg(&output)
			.output()
			.unwrap(),
	);
	assert!(
		missing_scene["error"]["message"]
			.as_str()
			.unwrap()
			.contains("unregistered scene")
	);
	assert!(!output.exists());

	fs::write(&cue_manifest, variant(project.startup_scene_id.as_str())).unwrap();
	fs::write(audio.join("cue-key.wav"), &KEY_CLIP[..KEY_CLIP.len() - 1]).unwrap();
	let rejected = failure(
		fixture
			.command()
			.arg("--output")
			.arg(&output)
			.output()
			.unwrap(),
	);
	assert!(
		rejected["error"]["message"]
			.as_str()
			.unwrap()
			.contains("canonical 16 kHz"),
		"{rejected}"
	);
	assert!(!output.exists());
}

#[cfg(unix)]
#[test]
fn audio_rejects_a_linked_default_project_asset_root() {
	use std::os::unix::fs::symlink;

	let fixture = Fixture::new();
	let assets = fixture.root.join("assets");
	let real_assets = fixture.root.join("ordinary-assets");
	fs::rename(&assets, &real_assets).unwrap();
	symlink(real_assets, &assets).unwrap();
	let report = fixture.root.join("report.json");
	let rejected = failure(
		fixture
			.command()
			.arg("--output")
			.arg(&report)
			.output()
			.unwrap(),
	);
	assert!(
		rejected["error"]["message"]
			.as_str()
			.unwrap()
			.contains("Feedback audio asset root must be an ordinary directory"),
		"{rejected}"
	);
	assert!(!report.exists());
}
