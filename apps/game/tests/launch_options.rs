use game::launch::{LaunchMode, LaunchOptions};

#[test]
fn ordinary_development_runs_preserve_controls_and_installed_projects_default_to_player() {
	for legacy_demo in [false, true] {
		assert_eq!(
			LaunchOptions::parse(std::iter::empty::<&str>(), false, legacy_demo)
				.unwrap()
				.mode,
			LaunchMode::Development
		);
		assert_eq!(
			LaunchOptions::parse(std::iter::empty::<&str>(), true, legacy_demo)
				.unwrap()
				.mode,
			LaunchMode::Player
		);
	}
}

#[test]
fn explicit_launch_mode_wins_over_both_installed_discovery_and_legacy_environment() {
	for installed in [false, true] {
		for legacy_demo in [false, true] {
			assert_eq!(
				LaunchOptions::parse(["--development"], installed, legacy_demo)
					.unwrap()
					.mode,
				LaunchMode::Development
			);
			assert_eq!(
				LaunchOptions::parse(["--player"], installed, legacy_demo)
					.unwrap()
					.mode,
				LaunchMode::Player
			);
		}
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
		assert!(LaunchOptions::parse(arguments, false, false).is_err());
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
