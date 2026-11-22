use super::*;
use crate::player_controls::{audio::FeedbackMix, display::DisplaySettings};
use runtime::input::{FaceButton, GameInputBinding};
use std::{
	fs,
	sync::atomic::{AtomicU64, Ordering},
};

struct Fixture {
	root: PathBuf,
	manifest: PathBuf,
}

impl Fixture {
	fn new() -> Self {
		static NEXT: AtomicU64 = AtomicU64::new(0);
		let root = std::env::temp_dir().join(format!(
			"game-preset-tests-{}-{}",
			std::process::id(),
			NEXT.fetch_add(1, Ordering::Relaxed),
		));
		fs::create_dir(&root).unwrap();
		let manifest = root.join("project.json");
		let mut project =
			ProjectData::from_json(include_str!("../../project.courtyard-chapters.json")).unwrap();
		project
			.input_profile
			.as_mut()
			.unwrap()
			.actions
			.push(GameInputBinding {
				id: "extra".into(),
				key: Key::F,
				animation_parameter: None,
			});
		fs::write(&manifest, project.to_json().unwrap()).unwrap();
		Self { root, manifest }
	}

	fn profile(&self, name: &str) -> PresetTransfer {
		let folder = self.root.join(name);
		fs::create_dir(&folder).unwrap();
		PresetTransfer {
			manifest: self.manifest.clone(),
			scope: Scope::read(&self.manifest).unwrap().0,
			preferences: Preferences::test_profile(&folder),
		}
	}
}

impl Drop for Fixture {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.root);
	}
}

fn swap() -> ControlsSettings {
	ControlsSettings {
		movement: Preset::Arrows,
		controller: ControllerBindings {
			jump: FaceButton::X,
			interact: FaceButton::A,
		},
		actions: vec![
			ActionKeyOverride {
				action_id: "interact".into(),
				key: Key::F,
			},
			ActionKeyOverride {
				action_id: "extra".into(),
				key: Key::E,
			},
		],
		..ControlsSettings::default()
	}
}

#[test]
fn reviewed_swap_changes_only_input_and_keeps_the_portable_file() {
	let fixture = Fixture::new();
	let source = fixture.profile("source");
	let destination = fixture.profile("destination");
	let path = fixture.root.join("portable.input-preset.json");
	let saved = ControlsSettings {
		display: DisplaySettings {
			high_contrast: true,
			..DisplaySettings::default()
		},
		audio: serde_json::from_str::<FeedbackMix>(
			r#"{"level_percent":60,"muted":true,"bus_percent":{"collection":25,"mechanisms":75,"completion":0}}"#,
		)
			.unwrap(),
		..ControlsSettings::default()
	};
	destination.preferences.save(&saved).unwrap();
	source.preferences.save(&swap()).unwrap();
	source.export(&path).unwrap();
	assert!(source.export(&path).is_err());
	let original = fs::read(&path).unwrap();
	let record = String::from_utf8(original.clone()).unwrap();
	assert!(
		!record.contains("display") && !record.contains("audio") && !record.contains("destination")
	);
	let review = destination.preview(&path).unwrap();
	assert_eq!(
		review
			.actions
			.iter()
			.map(|value| value.id.as_str())
			.collect::<Vec<_>>(),
		["interact", "extra"]
	);
	assert_eq!(review.actions[0].keys.current, Key::E);
	assert_eq!(review.actions[0].keys.proposed, Key::F);
	let (_, effective) = destination
		.apply_reviewed(&path, review.source_sha256, review.destination_sha256)
		.unwrap();
	assert_eq!(effective.actions[1].key, Key::E);
	let loaded = destination.preferences.load().unwrap();
	assert_eq!(loaded.display, saved.display);
	assert_eq!(loaded.audio, saved.audio);
	assert_eq!(loaded.controller, swap().controller);
	assert_eq!(loaded.movement, Preset::Arrows);
	assert_eq!(fs::read(&path).unwrap(), original);
}

#[test]
fn changed_source_destination_and_profile_never_authorize_the_write() {
	let fixture = Fixture::new();
	let source = fixture.profile("source");
	let destination = fixture.profile("destination");
	let unrelated = fixture.profile("unrelated");
	let path = fixture.root.join("portable.input-preset.json");
	source.export_applied(&path, &swap()).unwrap();
	let original = fs::read(&path).unwrap();
	let review = destination.preview(&path).unwrap();
	assert!(
		unrelated
			.apply_reviewed(&path, review.source_sha256, review.destination_sha256)
			.is_err()
	);
	let mut changed = original.clone();
	changed.push(b'\n');
	fs::write(&path, &changed).unwrap();
	assert!(
		destination
			.apply_reviewed(&path, review.source_sha256, review.destination_sha256)
			.is_err()
	);
	assert_eq!(
		destination.preferences.load().unwrap(),
		ControlsSettings::default()
	);
	fs::write(&path, original).unwrap();
	let current = ControlsSettings {
		movement: Preset::Arrows,
		..ControlsSettings::default()
	};
	destination.preferences.save(&current).unwrap();
	let folder = destination
		.preferences
		.profile_root()
		.join("GameEngine/controls/test-controls.json");
	let bytes = fs::read(&folder).unwrap();
	assert!(
		destination
			.apply_reviewed(&path, review.source_sha256, review.destination_sha256)
			.is_err()
	);
	assert_eq!(fs::read(&folder).unwrap(), bytes);
}

#[test]
fn preview_rejects_shape_size_wrong_authoring_and_composed_collisions() {
	let fixture = Fixture::new();
	let owner = fixture.profile("source");
	let path = fixture.root.join("invalid.input-preset.json");
	let mut record = owner.scope.record(&swap()).unwrap();
	for key in [Key::ArrowUp, Key::Escape] {
		record.input.actions[0].key = key;
		fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
		assert!(owner.preview(&path).is_err());
	}
	record.input.actions[0].key = Key::F;
	record.input.actions[0].action_id = "missing".into();
	fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
	assert!(owner.preview(&path).unwrap_err().contains("missing"));
	record.input.actions[0].action_id = "interact".into();
	record.authored_input_sha256 = "0".repeat(64);
	fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
	assert!(
		owner
			.preview(&path)
			.unwrap_err()
			.contains("different authored input profile")
	);
	for invalid in [
		br#"{"schema_version":1,"extra":true}"#.to_vec(),
		vec![b' '; MAX_PRESET_BYTES + 1],
	] {
		fs::write(&path, invalid).unwrap();
		assert!(owner.preview(&path).is_err());
	}
	assert_eq!(
		owner.preferences.load().unwrap(),
		ControlsSettings::default()
	);
}
