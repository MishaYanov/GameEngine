use super::*;
use std::{
	fs,
	process::{Command, Stdio},
};

const WRITER_PROFILE: &str = "GAME_ENGINE_QA_PRESET_LIBRARY_WRITER_PROFILE";
const WRITER_RANGE: &str = "GAME_ENGINE_QA_PRESET_LIBRARY_WRITER_RANGE";
const BACKUP_RENAME_PROFILE: &str = "GAME_ENGINE_QA_BACKUP_RENAME_PROFILE";

struct Fixture {
	root: PathBuf,
	owner: PresetTransfer,
}

impl Fixture {
	fn new() -> Self {
		let root = std::env::temp_dir().join(format!(
			"game-preset-library-{}",
			runtime::EntityKey::generate().as_str()
		));
		fs::create_dir(&root).unwrap();
		let manifest = root.join("project.json");
		fs::write(
			&manifest,
			include_str!("../../project.courtyard-chapters.json"),
		)
			.unwrap();
		let owner = test_owner(&root);
		Self { root, owner }
	}
}

impl Drop for Fixture {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.root);
	}
}

fn test_owner(root: &Path) -> PresetTransfer {
	let manifest = root.join("project.json");
	PresetTransfer {
		scope: Scope::read(&manifest).unwrap().0,
		manifest,
		preferences: Preferences::test_profile(root),
	}
}

#[test]
fn local_layout_library_merges_competing_writers_and_validates_bounds() {
	let settings = ControlsSettings::default();
	if let Some(root) = std::env::var_os(WRITER_PROFILE) {
		let owner = test_owner(&PathBuf::from(root));
		let range = std::env::var(WRITER_RANGE).unwrap();
		let (start, end) = range.split_once('-').unwrap();
		for index in start.parse::<usize>().unwrap()..end.parse::<usize>().unwrap() {
			owner
				.save_layout(&format!("Test-{index}"), &settings)
				.unwrap();
		}
		return;
	}
	let fixture = Fixture::new();
	let owner = &fixture.owner;
	assert!(owner.library().layouts().unwrap().is_empty());
	owner.save_layout("  my  layout ", &settings).unwrap();
	assert_eq!(owner.library().layouts().unwrap()[0].label, "MY LAYOUT");
	assert!(owner.save_layout("My Layout", &settings).is_err());
	let executable = std::env::current_exe().unwrap();
	let children = ["1-4", "4-8"].map(|range| {
		Command::new(&executable)
			.args(["--exact", "player_controls::input_preset::library_tests::local_layout_library_merges_competing_writers_and_validates_bounds"])
			.env(WRITER_PROFILE, &fixture.root)
			.env(WRITER_RANGE, range)
			.stdout(Stdio::piped())
			.stderr(Stdio::piped())
			.spawn()
			.unwrap()
	});
	for child in children {
		let output = child.wait_with_output().unwrap();
		assert!(
			output.status.success(),
			"writer failed:\n{}\n{}",
			String::from_utf8_lossy(&output.stdout),
			String::from_utf8_lossy(&output.stderr)
		);
		assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
	}
	let saved = owner.library().layouts().unwrap();
	assert_eq!(saved.len(), 8);
	for index in 1..8 {
		assert!(
			saved
				.iter()
				.any(|entry| entry.label == format!("TEST-{index}"))
		);
	}

	let path = owner.preferences.preset_library_path().unwrap().unwrap();
	let original = fs::read(&path).unwrap();
	assert!(owner.save_layout("Too Many", &settings).is_err());
	assert!(owner.library().rename(&saved[0], &"X".repeat(33)).is_err());
	assert!(owner.library().rename(&saved[0], &saved[1].label).is_err());
	assert_eq!(fs::read(&path).unwrap(), original);

	let invalid = b"{ incomplete job output";
	fs::write(&path, invalid).unwrap();
	assert!(owner.library().layouts().is_err());
	assert!(owner.save_layout("Other", &settings).is_err());
	assert_eq!(fs::read(path).unwrap(), invalid);
	assert_eq!(
		owner.preferences.load().unwrap().movement,
		settings.movement
	);
}

#[test]
fn named_layout_review_rechecks_entry_revision_and_destination_under_shared_lock() {
	let fixture = Fixture::new();
	let owner = &fixture.owner;
	let layout = ControlsSettings {
		actions: vec![ActionKeyOverride {
			action_id: "interact".into(),
			key: Key::F,
		}],
		..ControlsSettings::default()
	};
	owner.save_layout("Night", &layout).unwrap();
	let library = owner.library();
	let original = library.layouts().unwrap().remove(0);
	let review = owner.preview_layout(&original).unwrap();
	assert_eq!(review.actions[0].keys.current, Key::E);
	assert_eq!(review.actions[0].keys.proposed, Key::F);

	library.rename(&original, "Temporary").unwrap();
	let renamed = library.layouts().unwrap().remove(0);
	library.rename(&renamed, "Night").unwrap();
	assert!(owner.apply_layout(&original, &review).is_err());

	let current = library.layouts().unwrap().remove(0);
	let review = owner.preview_layout(&current).unwrap();
	library.delete(&current).unwrap();
	owner.save_layout("Night", &layout).unwrap();
	assert!(owner.apply_layout(&current, &review).is_err());
	assert!(owner.preferences.load().unwrap().actions.is_empty());

	let current = library.layouts().unwrap().remove(0);
	let review = owner.preview_layout(&current).unwrap();
	let mut settings = owner.preferences.load().unwrap();
	settings.display.high_contrast = true;
	owner.preferences.save(&settings).unwrap();
	assert!(owner.apply_layout(&current, &review).is_err());
	let fresh = owner.preview_layout(&current).unwrap();
	let (applied, profile) = owner.apply_layout(&current, &fresh).unwrap();
	assert!(applied.display.high_contrast);
	assert_eq!(profile.actions[0].key, Key::F);
	let reopened = owner.preferences.load().unwrap();
	assert_eq!(reopened.actions[0].key, Key::F);
	assert!(reopened.display.high_contrast);
}

#[test]
fn portable_library_export_is_sorted_bounded_and_uses_a_separate_shape() {
	let fixture = Fixture::new();
	let owner = &fixture.owner;
	let empty = fixture.root.join("empty.input-library.json");
	assert_eq!(owner.export_library(&empty).unwrap(), 0);
	let empty: serde_json::Value = serde_json::from_slice(&fs::read(empty).unwrap()).unwrap();
	assert_eq!(empty["schema_version"], 1);
	assert_eq!(empty["entries"].as_array().unwrap().len(), 0);

	let backup = fixture.root.join("full.input-library.json");
	let settings = ControlsSettings::default();
	for index in (0..8).rev() {
		owner
			.save_layout(&format!("  saved  {index} "), &settings)
			.unwrap();
	}
	assert_eq!(owner.export_library(&backup).unwrap(), 8);
	let bytes = fs::read(&backup).unwrap();
	assert!(bytes.len() < 40 * 1024);
	let decoded: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
	for (index, entry) in decoded["entries"].as_array().unwrap().iter().enumerate() {
		assert_eq!(entry.as_object().unwrap().len(), 2);
		assert_eq!(entry["label"], format!("SAVED {index}"));
		assert_eq!(entry["preset"]["schema_version"], 1);
		assert!(entry["preset"]["input"]["display"].is_null());
	}
	assert!(owner.export_library(&backup).is_err());
	assert_eq!(fs::read(&backup).unwrap(), bytes);
}

#[test]
fn portable_library_restore_rejects_stale_reviews_and_merges_with_fresh_identities() {
	let settings = ControlsSettings::default();
	if let Some(root) = std::env::var_os(BACKUP_RENAME_PROFILE) {
		let owner = test_owner(&PathBuf::from(root));
		let layout = owner.library().layouts().unwrap().remove(0);
		owner
			.library()
			.rename(&layout, "Renamed Elsewhere")
			.unwrap();
		return;
	}
	let source = Fixture::new();
	let destination = Fixture::new();
	let owner = &destination.owner;
	let backup = source.root.join("saved.input-library.json");
	let different = ControlsSettings {
		actions: vec![ActionKeyOverride {
			action_id: "interact".into(),
			key: Key::F,
		}],
		..settings.clone()
	};
	source.owner.save_layout("Shared", &different).unwrap();
	source.owner.save_layout("Extra", &different).unwrap();
	source.owner.export_library(&backup).unwrap();
	owner.save_layout("Shared", &settings).unwrap();
	let prior = owner.library().layouts().unwrap().remove(0);

	let stale = owner.preview_library_backup(&backup).unwrap();
	let output = Command::new(std::env::current_exe().unwrap())
		.args(["--exact", "player_controls::input_preset::library_tests::portable_library_restore_rejects_stale_reviews_and_merges_with_fresh_identities"])
		.env(BACKUP_RENAME_PROFILE, &destination.root)
		.output()
		.unwrap();
	assert!(
		output.status.success(),
		"{}",
		String::from_utf8_lossy(&output.stdout)
	);
	assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
	assert!(
		owner
			.restore_library_backup(&backup, &stale)
			.unwrap_err()
			.contains("Local saved layouts changed")
	);
	let renamed = owner.library().layouts().unwrap().remove(0);
	owner.library().rename(&renamed, &prior.label).unwrap();
	assert!(owner.restore_library_backup(&backup, &stale).is_err());

	let reviewed = owner.preview_library_backup(&backup).unwrap();
	assert_eq!(reviewed.additions, ["EXTRA"]);
	assert_eq!(reviewed.conflicts, ["SHARED"]);
	let original = fs::read(&backup).unwrap();
	fs::write(&backup, [original.as_slice(), b"\n"].concat()).unwrap();
	assert!(
		owner
			.restore_library_backup(&backup, &reviewed)
			.unwrap_err()
			.contains("backup changed")
	);
	assert_eq!(owner.library().layouts().unwrap().len(), 1);
	fs::write(&backup, original).unwrap();

	owner.save_layout("Scrapped", &settings).unwrap();
	let scrapped = owner.library().layouts().unwrap().remove(1);
	owner.library().delete(&scrapped).unwrap();
	let reviewed = owner.preview_library_backup(&backup).unwrap();
	assert_eq!(owner.restore_library_backup(&backup, &reviewed).unwrap(), 1);
	let reopened = test_owner(&destination.root);
	let layouts = reopened.library().layouts().unwrap();
	assert_eq!(
		layouts
			.iter()
			.map(|item| item.label.as_str())
			.collect::<Vec<_>>(),
		["SHARED", "EXTRA"]
	);
	assert!(
		reopened
			.apply_layout(&scrapped, &reopened.preview_layout(&layouts[1]).unwrap())
			.is_err()
	);
	let path = reopened.preferences.preset_library_path().unwrap().unwrap();
	let record: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
	assert_eq!(record["entries"][0]["layout"]["id"], 1);
	assert_eq!(record["entries"][1]["layout"]["id"], 3);
	assert!(reopened.preferences.load().unwrap().actions.is_empty());
	let shared = reopened.preview_layout(&layouts[0]).unwrap();
	assert_eq!(shared.actions[0].keys.proposed, Key::E);
	let extra = reopened.preview_layout(&layouts[1]).unwrap();
	assert_eq!(extra.actions[0].keys.proposed, Key::F);
	assert_eq!(
		reopened
			.apply_layout(&layouts[1], &extra)
			.unwrap()
			.1
			.actions[0]
			.key,
		Key::F
	);
}

#[test]
fn portable_library_review_validates_scope_duplicate_labels_and_capacity() {
	let source = Fixture::new();
	let destination = Fixture::new();
	let owner = &destination.owner;
	let settings = ControlsSettings::default();
	for index in 0..7 {
		owner
			.save_layout(&format!("Local {index}"), &settings)
			.unwrap();
	}
	let path = source.root.join("backup.input-library.json");
	source.owner.save_layout("One", &settings).unwrap();
	source.owner.save_layout("Two", &settings).unwrap();
	source.owner.export_library(&path).unwrap();
	let overflow = owner.preview_library_backup(&path).unwrap();
	assert_eq!(overflow.available, 1);
	assert_eq!(overflow.blocked(), Some("Library capacity exceeded"));
	assert!(owner.restore_library_backup(&path, &overflow).is_err());
	assert_eq!(owner.library().layouts().unwrap().len(), 7);

	let mut backup: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
	backup["entries"][1]["label"] = "  one  ".into();
	fs::write(&path, serde_json::to_vec(&backup).unwrap()).unwrap();
	assert!(
		owner
			.preview_library_backup(&path)
			.unwrap_err()
			.contains("repeated layout names")
	);
	backup["entries"][1]["label"] = "Two".into();
	backup["entries"][1]["preset"]["authored_input_sha256"] = "other".into();
	fs::write(&path, serde_json::to_vec(&backup).unwrap()).unwrap();
	assert!(
		owner
			.preview_library_backup(&path)
			.unwrap_err()
			.contains("different authored input profile")
	);
}
