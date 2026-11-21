//! Cross-process test of the installed checkpoint CLI and ordinary Continue path.
use super::*;

const NATIVE_TRANSFER_TEST: &str = "checkpoint_native_tests::checkpoint_transfer_native::packaged_checkpoint_transfer_spans_profiles";
const CHAPTER_PROFILE: &str = "GameEngine/checkpoints/3ef795f1-4bb8-41ff-bebb-66a170e68878";

#[test]
#[ignore = "requires native Vulkan and a copied chaptered Player in GAME_ENGINE_QA_TRANSFER_PLAYER"]
fn packaged_checkpoint_transfer_spans_profiles() {
	if let Some(phase) = std::env::var_os(PHASE_VAR) {
		run_native_phase(phase.to_str().unwrap());
		return;
	}
	let player = std::env::var_os("GAME_ENGINE_QA_TRANSFER_PLAYER")
		.map(PathBuf::from)
		.expect("set GAME_ENGINE_QA_TRANSFER_PLAYER to packaged game.exe")
		.canonicalize()
		.unwrap();
	let manifest = player.with_file_name("project.json");
	let source = ProfileDir::new();
	let destination = ProfileDir::new();
	run_isolated_phase(
		NATIVE_TRANSFER_TEST,
		"chapters-save",
		&source,
		Some(&manifest),
	);

	let source_dir = source.0.join(CHAPTER_PROFILE);
	let target_dir = destination.0.join(CHAPTER_PROFILE);
	let garden = fs::read(source_dir.join("slot-2.json")).unwrap();
	let courtyard = fs::read(source_dir.join("slot.json")).unwrap();
	let transfer = source.0.join("portable.json");
	let execute = |profile: &ProfileDir, arguments: &[&str]| {
		Command::new(&player)
			.args(arguments)
			.current_dir(&source.0)
			.env("LOCALAPPDATA", &profile.0)
			.creation_flags(0x0800_0000)
			.output()
			.unwrap()
	};
	let path = transfer.to_str().unwrap();
	let export = execute(&source, &["--export-slot", "2", path]);
	assert!(
		export.status.success(),
		"{}",
		String::from_utf8_lossy(&export.stderr)
	);
	assert!(
		!execute(&source, &["--export-slot", "1", path])
			.status
			.success()
	);
	let preview = |file| {
		let result = execute(&destination, &["--preview-slot-import", "2", file]);
		assert!(
			result.status.success(),
			"{}",
			String::from_utf8_lossy(&result.stderr)
		);
		serde_json::from_slice::<serde_json::Value>(&result.stdout).unwrap()
	};
	let commit = |review: &serde_json::Value, file: &str, replace| {
		let mut arguments = vec![
			"--commit-slot-import",
			"2",
			file,
			"--archive-sha256",
			review["archive_sha256"].as_str().unwrap(),
			"--slot-generation",
			review["destination_generation_sha256"].as_str().unwrap(),
		];
		if replace {
			arguments.push("--replace");
		}
		execute(&destination, &arguments)
	};

	let review = preview(path);
	assert_eq!(review["chapter_label"], "Garden");
	assert_eq!(review["replacement_required"], false);
	let result = commit(&review, path, false);
	assert!(
		result.status.success(),
		"{}",
		String::from_utf8_lossy(&result.stderr)
	);
	assert_eq!(fs::read(target_dir.join("slot-2.json")).unwrap(), garden);
	assert!(!commit(&review, path, true).status.success());
	run_isolated_phase(
		NATIVE_TRANSFER_TEST,
		"chapters-transfer-continue",
		&destination,
		Some(&manifest),
	);

	let replacement = source.0.join("courtyard.json");
	let replacement = replacement.to_str().unwrap();
	assert!(
		execute(&source, &["--export-slot", "1", replacement])
			.status
			.success()
	);
	let review = preview(replacement);
	assert_eq!(review["chapter_label"], "Courtyard");
	assert_eq!(review["replacement_required"], true);
	assert!(!commit(&review, replacement, false).status.success());
	assert!(commit(&review, replacement, true).status.success());
	assert_eq!(fs::read(target_dir.join("slot-2.json")).unwrap(), courtyard);
	assert_eq!(
		fs::read(target_dir.join("slot-2.previous.json")).unwrap(),
		garden
	);
	assert!(!target_dir.join("slot.json").exists());
	assert!(!target_dir.join("slot-3.json").exists());
	assert_eq!(fs::read(source_dir.join("slot-2.json")).unwrap(), garden);
	assert_eq!(fs::read(source_dir.join("slot.json")).unwrap(), courtyard);
}

pub(super) fn continue_imported_garden(app: &mut GameApp) {
	open_slot_actions(app, false, SaveSlot::Two);
	menu_keys(app, &[Key::Enter]);
	close_checkpoint_menu(app);
	assert_eq!(app.editor.play_scene_id().unwrap().as_str(), "garden");
	assert_eq!(progress(app).score, 2);
	let garden = SceneId::new("garden").unwrap();
	assert!(progress(app).scene_facts(&garden).has_key);
	move_for(app, Key::D, 90);
	assert_eq!(app.editor.play_scene_id().unwrap().as_str(), "finish");
	move_for(app, Key::D, 90);
	assert!(progress(app).completed);
}
