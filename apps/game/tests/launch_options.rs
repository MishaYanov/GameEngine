use game::launch::{LaunchMode, LaunchOptions};

#[test]
fn installed_projects_default_to_player() {
	for (installed, expected) in [(false, LaunchMode::Development), (true, LaunchMode::Player)] {
		assert_eq!(
			LaunchOptions::parse(std::iter::empty::<&str>(), installed)
				.unwrap()
				.mode,
			expected
		);
	}
}

#[test]
fn explicit_mode_overrides_installed_project_discovery() {
	for installed in [false, true] {
		assert_eq!(
			LaunchOptions::parse(["--development"], installed)
				.unwrap()
				.mode,
			LaunchMode::Development
		);
		assert_eq!(
			LaunchOptions::parse(["--player"], installed).unwrap().mode,
			LaunchMode::Player
		);
	}
}

#[test]
fn unknown_duplicate_and_conflicting_launch_flags_are_rejected() {
	for arguments in [
		vec!["--unknown"],
		vec!["--player", "--development"],
		vec!["--player", "--player"],
		vec!["--development", "--development"],
		vec!["--player", "scene.json"],
	] {
		assert!(LaunchOptions::parse(arguments, false).is_err());
	}
}

#[test]
fn real_executable_rejects_invalid_launch_arguments_before_creating_platform_resources() {
	let result = std::process::Command::new(env!("CARGO_BIN_EXE_game"))
		.arg("--qa-invalid-launch-mode")
		.output()
		.expect("run game startup validation");
	assert!(!result.status.success());
	let stderr = String::from_utf8_lossy(&result.stderr);
	assert!(
		stderr.contains("Unknown game argument"),
		"unexpected startup failure: {stderr}"
	);
	assert!(stderr.contains("--development") && stderr.contains("--player"));
	assert!(!stderr.contains("failed to create event loop"));
}

#[test]
fn smoke_mode_is_explicit_and_keeps_launch_selection() {
	assert!(
		!LaunchOptions::parse(std::iter::empty::<&str>(), false)
			.unwrap()
			.smoke_test
	);
	for arguments in [vec!["--smoke-test"], vec!["--smoke-test", "--development"]] {
		let options = LaunchOptions::parse(arguments, false).unwrap();
		assert!(options.smoke_test);
		assert_eq!(options.mode, LaunchMode::Development);
	}
	let options = LaunchOptions::parse(["--player", "--smoke-test"], false).unwrap();
	assert!(options.smoke_test);
	assert_eq!(options.mode, LaunchMode::Player);
	assert!(LaunchOptions::parse(["--smoke-test", "--smoke-test"], false).is_err());
}

#[test]
fn playtrace_is_opt_in_and_requires_a_regular_player_run() {
	use std::path::Path;

	let options = LaunchOptions::parse(["--record-playtrace", "trial.json"], true).unwrap();
	assert_eq!(
		options.record_playtrace.as_deref(),
		Some(Path::new("trial.json"))
	);
	assert!(LaunchOptions::parse(["--player", "--record-playtrace", "trial.json"], false).is_ok());
	assert!(
		LaunchOptions::parse(["--player"], true)
			.unwrap()
			.record_playtrace
			.is_none()
	);
	for arguments in [
		vec!["--development", "--record-playtrace", "trial.json"],
		vec!["--smoke-test", "--record-playtrace", "trial.json"],
		vec!["--record-playtrace"],
		vec!["--record-playtrace", "--player"],
		vec![
			"--record-playtrace",
			"trial.json",
			"--record-playtrace",
			"again.json",
		],
	] {
		assert!(LaunchOptions::parse(arguments, true).is_err());
	}
}

#[test]
fn scene_cycles_require_bounded_player_smoke() {
	for options in [
		vec!["--smoke-scene-cycles", "2"],
		vec!["--development", "--smoke-test", "--smoke-scene-cycles", "2"],
		vec!["--smoke-test", "--smoke-scene-cycles", "0"],
		vec!["--smoke-test", "--smoke-scene-cycles", "101"],
		vec![
			"--smoke-test",
			"--smoke-scene-cycles",
			"2",
			"--smoke-scene-cycles",
			"3",
		],
	] {
		assert!(LaunchOptions::parse(options, true).is_err());
	}
	assert_eq!(
		LaunchOptions::parse(["--smoke-test", "--smoke-scene-cycles", "20"], true)
			.unwrap()
			.smoke_scene_cycles,
		20
	);
}

#[test]
fn checkpoint_commands_are_excluded_from_the_player_and_prefetch_is_installed_only() {
	assert!(LaunchOptions::parse(["--smoke-test", "--smoke-prefetch-probe"], true).is_err());
	assert!(
		!LaunchOptions::parse(["--smoke-test", "--smoke-scene-cycles", "32"], true)
			.unwrap()
			.smoke_prefetch_probe
	);
	assert!(
		LaunchOptions::parse(
			[
				"--smoke-test",
				"--smoke-scene-cycles",
				"32",
				"--smoke-prefetch-probe"
			],
			true,
		)
		.unwrap()
		.smoke_prefetch_probe
	);
	for command in [
		"--export-slot",
		"--preview-slot-import",
		"--commit-slot-import",
		"--export-transfer-history",
	] {
		let error = LaunchOptions::parse([command], true).unwrap_err();
		assert!(error.starts_with("Unknown game argument"));
	}
	assert!(LaunchOptions::parse(["--scene-prefetch", "--development"], true).is_err());
	assert!(LaunchOptions::parse(["--scene-prefetch"], false).is_err());
	assert!(
		LaunchOptions::parse(["--smoke-test", "--scene-prefetch"], true)
			.unwrap()
			.scene_prefetch
	);
}

#[test]
fn input_preset_commands_are_standalone_and_apply_requires_both_tokens() {
	use game::launch::InputPresetCommand;

	let digest = "ab".repeat(32);
	for command in ["--export-input-preset", "--preview-input-preset"] {
		assert!(LaunchOptions::parse([command, "preset.json"], false).is_err());
		assert!(LaunchOptions::parse([command, "preset.json", "--player"], true).is_err());
		assert!(
			LaunchOptions::parse([command, "preset.json"], true)
				.unwrap()
				.input_preset
				.is_some()
		);
	}
	let apply = LaunchOptions::parse(
		[
			"--apply-input-preset",
			"preset.json",
			"--source-sha256",
			&digest,
			"--destination-sha256",
			&digest,
		],
		true,
	)
	.unwrap()
	.input_preset;
	assert!(
		matches!(apply, Some(InputPresetCommand::Apply { source_sha256, destination_sha256, .. }) if source_sha256 == [0xab; 32] && destination_sha256 == [0xab; 32])
	);
	for arguments in [
		vec![
			"--apply-input-preset",
			"preset.json",
			"--source-sha256",
			&digest,
		],
		vec![
			"--apply-input-preset",
			"preset.json",
			"--source-sha256",
			&digest,
			"--source-sha256",
			&digest,
		],
		vec![
			"--apply-input-preset",
			"preset.json",
			"--source-sha256",
			"zz",
			"--destination-sha256",
			&digest,
		],
	] {
		assert!(LaunchOptions::parse(arguments, true).is_err());
	}
}

#[test]
fn recent_source_experiment_is_limited_to_installed_player_switch_smoke() {
	let flag = "--smoke-retain-recent-source";
	let smoke = ["--smoke-test", "--smoke-scene-cycles", "2", flag];
	assert!(LaunchOptions::parse([flag], true).is_err());
	assert!(LaunchOptions::parse(smoke, false).is_err());
	assert!(LaunchOptions::parse([flag, flag], true).is_err());
	assert!(
		LaunchOptions::parse(smoke, true)
			.unwrap()
			.smoke_retain_recent_source
	);
	assert!(
		!LaunchOptions::parse(["--smoke-test", "--smoke-scene-cycles", "2"], true)
			.unwrap()
			.smoke_retain_recent_source
	);
}
