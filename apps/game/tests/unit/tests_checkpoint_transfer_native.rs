//! Cross-process rehearsals of installed checkpoint transfer and ordinary Continue.
use super::*;

const NATIVE_TRANSFER_TEST: &str = "checkpoint_native_tests::checkpoint_transfer_native::packaged_checkpoint_transfer_spans_profiles";
const NATIVE_MENU_TRANSFER_TEST: &str =
	"checkpoint_native_tests::checkpoint_transfer_native::checkpoint_transfer_menu_spans_profiles";
const CHAPTER_PROFILE: &str = "GameEngine/checkpoints/3ef795f1-4bb8-41ff-bebb-66a170e68878";

#[test]
#[ignore = "requires a native Windows Vulkan session"]
fn checkpoint_transfer_menu_spans_profiles() {
	if let Some(phase) = std::env::var_os(PHASE_VAR) {
		run_native_phase(phase.to_str().unwrap());
		return;
	}
	let manifest =
		PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("project.courtyard-chapters.json");
	let rehearsal = ProfileDir::new();
	let source = ProfileDir(rehearsal.0.join("source"));
	let destination = ProfileDir(rehearsal.0.join("destination"));
	let transfers = rehearsal.0.join("transfers");
	for path in [&source.0, &destination.0, &transfers] {
		fs::create_dir(path).unwrap();
	}
	let run = |phase, profile| {
		run_isolated_phase(NATIVE_MENU_TRANSFER_TEST, phase, profile, Some(&manifest));
	};
	run("chapters-save", &source);
	let saved_slots = ["slot.json", "slot-2.json", "slot-2.previous.json"]
		.map(|name| source.0.join(CHAPTER_PROFILE).join(name));
	let saved_bytes = saved_slots.each_ref().map(|path| fs::read(path).unwrap());

	run("chapters-transfer-menu-export", &source);
	assert_history(&source.0, "export", "garden_entry");
	assert!(
		!destination
			.0
			.join(CHAPTER_PROFILE)
			.join("transfer-history.json")
			.exists()
	);
	let mut files = fs::read_dir(&transfers).unwrap();
	let archive = files.next().unwrap().unwrap();
	assert!(files.next().is_none());
	let name = archive.file_name();
	let name = name.to_str().unwrap();
	assert!(name.starts_with("slot-2-") && name.ends_with(".courtyard-save.json"));
	let archive = archive.path();
	let archive_bytes = fs::read(&archive).unwrap();

	let target = destination.0.join(CHAPTER_PROFILE);
	let imported = target.join("slot-2.json");
	assert!(!imported.exists());
	run("chapters-transfer-menu-import", &destination);
	let mixed = history_rows(&destination.0);
	assert_eq!(mixed.len(), 2);
	assert_eq!(mixed[0]["direction"], "import");
	assert_eq!(mixed[1]["direction"], "export");
	assert!(
		mixed
			.iter()
			.all(|row| row["slot"] == 2 && row["checkpoint_id"] == "garden_entry")
	);
	assert_eq!(fs::read(&imported).unwrap(), saved_bytes[1]);
	run("chapters-transfer-continue", &destination);
	run("chapters-transfer-history-manage", &destination);
	run("chapters-transfer-history-relaunch", &destination);
	assert_eq!(fs::read(&imported).unwrap(), saved_bytes[1]);
	for name in ["slot.json", "slot-2.previous.json", "slot-3.json"] {
		assert!(!target.join(name).exists(), "unexpected file {name}");
	}
	assert_eq!(fs::read(archive).unwrap(), archive_bytes);
	assert_eq!(saved_slots.map(|path| fs::read(path).unwrap()), saved_bytes);
}

fn assert_history(profile: &Path, direction: &str, checkpoint: &str) {
	let entries = history_rows(profile);
	assert_eq!(entries.len(), 1);
	assert_eq!(entries[0]["slot"], 2);
	assert_eq!(entries[0]["direction"], direction);
	assert_eq!(entries[0]["checkpoint_id"], checkpoint);
}

fn history_rows(profile: &Path) -> Vec<serde_json::Value> {
	let file = profile.join(CHAPTER_PROFILE).join("transfer-history.json");
	let record: serde_json::Value = serde_json::from_slice(&fs::read(file).unwrap()).unwrap();
	record["entries"].as_array().unwrap().clone()
}

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
	let source_history = source_dir.join("transfer-history.json");
	let exported_history = fs::read(&source_history).unwrap();
	assert!(
		!execute(&source, &["--export-slot", "1", path])
			.status
			.success()
	);
	assert_eq!(fs::read(&source_history).unwrap(), exported_history);
	assert_history(&source.0, "export", "garden_entry");
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
	let destination_history = target_dir.join("transfer-history.json");
	let accepted = fs::read(&destination_history).unwrap();
	assert!(!commit(&review, path, true).status.success());
	assert_eq!(fs::read(&destination_history).unwrap(), accepted);
	assert_history(&destination.0, "import", "garden_entry");
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

	let all = source.0.join("all-history.json");
	let slot = source.0.join("slot-2-history.json");
	let all_command = ["--export-transfer-history", all.to_str().unwrap()];
	assert!(execute(&source, &all_command).status.success());
	assert!(
		execute(
			&source,
			&[
				"--export-transfer-history",
				slot.to_str().unwrap(),
				"--slot",
				"2"
			]
		)
			.status
			.success()
	);
	let all_bytes = fs::read(&all).unwrap();
	let all_history: serde_json::Value = serde_json::from_slice(&all_bytes).unwrap();
	let slot_history: serde_json::Value = serde_json::from_slice(&fs::read(slot).unwrap()).unwrap();
	assert_eq!(all_history["entries"].as_array().unwrap().len(), 2);
	assert_eq!(all_history["entries"][0]["slot"], 1);
	assert_eq!(all_history["entries"][1]["slot"], 2);
	assert_eq!(slot_history["entries"].as_array().unwrap().len(), 1);
	assert_eq!(slot_history["entries"][0], all_history["entries"][1]);
	assert!(!execute(&source, &all_command).status.success());
	assert_eq!(fs::read(all).unwrap(), all_bytes);

	let invalid_history = b"incomplete checkpoint history";
	fs::write(&source_history, invalid_history).unwrap();
	let extra = source.0.join("after-history-failure.json");
	let export = execute(&source, &["--export-slot", "2", extra.to_str().unwrap()]);
	assert!(export.status.success());
	assert!(String::from_utf8_lossy(&export.stderr).contains("History is unavailable"));
	assert!(extra.is_file());
	assert_eq!(fs::read(source_history).unwrap(), invalid_history);
}

pub(super) fn run_menu_phase(app: &mut GameApp, phase: &str) {
	let profile = PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap());
	let directory = profile.parent().unwrap().join("transfers");
	app.transfer_directory = Some(directory.clone());
	match phase {
		"chapters-transfer-menu-export" => {
			open_slot_actions(app, true, SaveSlot::Two);
			menu_keys(app, &[Key::ArrowUp]);
			assert_menu_rows(app, &["> Export checkpoint file"]);
			assert!(fs::read_dir(&directory).unwrap().next().is_none());
			menu_keys(app, &[Key::Enter]);
			assert_menu_rows(app, &["Exported slot-2-"]);
			assert_eq!(fs::read_dir(directory).unwrap().count(), 1);
		}
		"chapters-transfer-menu-import" => {
			let slot = profile.join(CHAPTER_PROFILE).join("slot-2.json");
			open_slot_actions(app, false, SaveSlot::Two);
			menu_keys(app, &[Key::ArrowUp]);
			assert_menu_rows(app, &["> Import checkpoint file"]);
			menu_keys(app, &[Key::Enter]);
			assert_menu_rows(app, &["From transfers", "> Transfer 01: slot-2-"]);
			menu_keys(app, &[Key::Enter]);
			assert_menu_rows(
				app,
				&["Transfer 01:", "Chapter: Garden", "Checkpoint:", "> Cancel"],
			);
			assert!(!slot.exists());
			menu_keys(app, &[Key::Enter]);
			assert_menu_rows(app, &["From transfers", "> Transfer 01: slot-2-"]);
			assert!(!slot.exists());
			assert!(
				!profile
					.join(CHAPTER_PROFILE)
					.join("transfer-history.json")
					.exists()
			);
			menu_keys(app, &[Key::Enter, Key::ArrowRight]);
			assert_menu_rows(app, &["> Import checkpoint"]);
			assert!(!slot.exists());
			menu_keys(app, &[Key::Enter]);
			assert_menu_rows(app, &["> Continue (Ready)"]);
			assert!(slot.is_file());
			menu_keys(app, &[Key::ArrowUp, Key::ArrowUp, Key::Enter]);
			assert_eq!(
				app.player_controls.rows_with_capacity(1).unwrap(),
				["> Filter: All"]
			);
			menu_keys(app, &[Key::ArrowDown]);
			assert_eq!(
				app.player_controls.rows_with_capacity(1).unwrap(),
				["> Imported garden_entry (now)"]
			);
			menu_keys(app, &[Key::Enter]);
			assert_eq!(
				app.player_controls.rows_with_capacity(1).unwrap(),
				["> Imported garden_entry (now)"]
			);
			menu_keys(app, &[Key::ArrowRight, Key::ArrowDown]);
			assert_menu_rows(app, &["Filter: Exports", "> No matching transfers"]);
			menu_keys(
				app,
				&[Key::ArrowRight, Key::ArrowDown, Key::ArrowDown, Key::Enter],
			);
			assert_menu_rows(
				app,
				&["Slot 2 audit preview", "> Cancel", "Export 1 history rows"],
			);
			menu_keys(app, &[Key::Enter]);
			assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
			menu_keys(app, &[Key::Enter, Key::ArrowRight, Key::Enter]);
			assert_menu_rows(app, &["Exported 1 history rows"]);
			let audit = fs::read_dir(&directory)
				.unwrap()
				.map(|entry| entry.unwrap().path())
				.find(|path| {
					path.file_name()
						.unwrap()
						.to_string_lossy()
						.starts_with("history-slot-2-")
				})
				.unwrap();
			let audit: serde_json::Value =
				serde_json::from_slice(&fs::read(audit).unwrap()).unwrap();
			assert_eq!(audit["entries"].as_array().unwrap().len(), 1);
			assert_eq!(audit["entries"][0]["direction"], "import");

			menu_keys(app, &[Key::ArrowDown, Key::ArrowDown, Key::ArrowDown]);
			assert_eq!(
				app.player_controls.rows_with_capacity(1).unwrap(),
				["> Back"]
			);
			menu_keys(app, &[Key::Enter]);
			assert_eq!(
				app.player_controls.rows_with_capacity(1).unwrap(),
				["> Transfer history"]
			);
			let second = directory.join("second-export");
			fs::create_dir(&second).unwrap();
			app.transfer_directory = Some(second);
			assert!(
				app.export_checkpoint_transfer(SaveSlot::Two)
					.unwrap()
					.starts_with("Exported slot-2-")
			);
			menu_keys(app, &[Key::Enter]);
			let rows = app.player_controls.rows().unwrap();
			assert!(rows[2].starts_with("  Exported garden_entry"));
			assert!(rows[3].starts_with("  Imported garden_entry"));
			menu_keys(app, &[Key::ArrowRight]);
			let rows = app.player_controls.rows().unwrap();
			assert!(rows[2].contains("Exported garden_entry"));
			assert!(!rows.iter().any(|row| row.contains("Imported garden_entry")));
			menu_keys(app, &[Key::ArrowRight]);
			let rows = app.player_controls.rows().unwrap();
			assert!(rows[2].contains("Imported garden_entry"));
			assert!(!rows.iter().any(|row| row.contains("Exported garden_entry")));
		}
		"chapters-transfer-history-manage" => {
			exercise_history_management(app, &profile, &directory)
		}
		"chapters-transfer-history-relaunch" => {
			open_slot_actions(app, false, SaveSlot::Two);
			menu_keys(app, &[Key::ArrowUp, Key::ArrowUp, Key::Enter]);
			assert_menu_rows(app, &["No matching transfers", "Retention: 16"]);
		}
		_ => panic!("unknown checkpoint transfer phase {phase}"),
	}
	close_checkpoint_menu(app);
}

fn exercise_history_management(app: &mut GameApp, profile: &Path, directory: &Path) {
	use crate::checkpoint_transfer_history::Direction;
	app.with_checkpoint_history(|history| {
		for index in 0..17 {
			history.append(SaveSlot::Two, Direction::Import, format!("garden_{index}"))?;
		}
		history.append(SaveSlot::One, Direction::Export, "courtyard".into())
	})
		.unwrap();
	let history = profile.join(CHAPTER_PROFILE).join("transfer-history.json");
	let original = fs::read(&history).unwrap();
	let prior_audits: Vec<_> = fs::read_dir(directory)
		.unwrap()
		.map(|entry| entry.unwrap().path())
		.filter(|path| {
			path.file_name()
				.unwrap()
				.to_string_lossy()
				.starts_with("history-slot-")
		})
		.collect();

	open_slot_actions(app, false, SaveSlot::Two);
	menu_keys(app, &[Key::ArrowUp, Key::ArrowUp, Key::Enter]);
	select_history_row(app, "> Retention: 64");
	select_history_row(app, "> Keep 16 entries");
	assert_menu_rows(app, &["> Keep History", "Remove 4 oldest entries"]);
	menu_keys(app, &[Key::Enter]);
	assert_eq!(fs::read(&history).unwrap(), original);
	select_history_row(app, "> Keep 16 entries");
	menu_keys(app, &[Key::ArrowRight, Key::Enter]);
	assert_menu_rows(app, &["Keeping 16; removed 4"]);
	assert!(
		app.player_controls
			.rows_with_capacity(72)
			.unwrap()
			.iter()
			.any(|row| row.ends_with("Retention: 16"))
	);
	select_history_row(app, "> Export history audit");
	menu_keys(app, &[Key::ArrowRight, Key::Enter]);
	let audit = fs::read_dir(directory)
		.unwrap()
		.map(|entry| entry.unwrap().path())
		.find(|path| {
			path.file_name()
				.unwrap()
				.to_string_lossy()
				.starts_with("history-slot-")
				&& !prior_audits.contains(path)
		})
		.unwrap();
	let audit_bytes = fs::read(&audit).unwrap();
	let reviewed: serde_json::Value = serde_json::from_slice(&audit_bytes).unwrap();
	assert_eq!(reviewed["entries"].as_array().unwrap().len(), 15);
	assert_eq!(reviewed["entries"][0]["checkpoint_id"], "garden_16");

	select_history_row(app, "> Review Clear History");
	assert_eq!(
		app.player_controls.rows_with_capacity(1).unwrap(),
		["> Keep History"]
	);
	select_history_row(app, "> Slot 2");
	assert_menu_rows(
		app,
		&[
			"Slot 2: 15 entries",
			"> Keep History",
			"Clear Reviewed Entries",
		],
	);
	let before_clear = fs::read(&history).unwrap();
	menu_keys(app, &[Key::Enter]);
	assert_eq!(fs::read(&history).unwrap(), before_clear);
	select_history_row(app, "> Slot 2");
	menu_keys(app, &[Key::ArrowRight, Key::Enter]);
	assert_menu_rows(
		app,
		&["No matching transfers", "Cleared 15 transfer entries"],
	);
	let entries = history_rows(profile);
	assert_eq!(entries.len(), 1);
	assert_eq!(entries[0]["slot"], 1);

	select_history_row(app, "> Review Clear History");
	select_history_row(app, "> Current project");
	assert_menu_rows(app, &["Current project: 1 entries", "> Keep History"]);
	menu_keys(app, &[Key::ArrowRight, Key::Enter]);
	assert_menu_rows(app, &["Cleared 1 transfer entries"]);
	assert!(history_rows(profile).is_empty());
	assert_eq!(fs::read(audit).unwrap(), audit_bytes);
}

fn select_history_row(app: &mut GameApp, label: &str) {
	for _ in 0..70 {
		if app.player_controls.rows_with_capacity(1).unwrap() == [label] {
			menu_keys(app, &[Key::Enter]);
			return;
		}
		menu_keys(app, &[Key::ArrowDown]);
	}
	panic!("History row {label} unavailable");
}

fn assert_menu_rows(app: &GameApp, expected: &[&str]) {
	let rows = app.player_controls.rows().unwrap().join("\n");
	for text in expected {
		assert!(rows.contains(text), "expected {text:?} in:\n{rows}");
	}
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
