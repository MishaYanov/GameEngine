use super::*;

struct Scratch(PathBuf);

impl Scratch {
	fn new() -> Self {
		static NEXT: AtomicU64 = AtomicU64::new(0);
		let root = std::env::temp_dir().join(format!(
			"game-controls-{}-{}",
			std::process::id(),
			NEXT.fetch_add(1, Ordering::Relaxed),
		));
		fs::create_dir(&root).unwrap();
		Self(root)
	}
	fn path(&self) -> &Path {
		&self.0
	}
}

impl Drop for Scratch {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.0);
	}
}

#[test]
fn preferences_round_trip_and_replace_within_the_local_directory() {
	let root = Scratch::new();
	let settings = Preferences {
		root: root.path().to_owned(),
		filename: "controls.json".into(),
	};
	assert_eq!(settings.load().unwrap(), ControlsSettings::default());
	let controls = ControlsSettings {
		movement: Preset::Arrows,
		..ControlsSettings::default()
	};
	settings.save(controls).unwrap();
	assert_eq!(settings.load().unwrap(), controls);
	let controls = ControlsSettings {
		movement: Preset::Wasd,
		..controls
	};
	settings.save(controls).unwrap();
	assert_eq!(settings.load().unwrap(), controls);
	assert_eq!(
		fs::read_dir(root.path().join("GameEngine/controls"))
			.unwrap()
			.count(),
		1
	);
}

#[test]
fn oversized_and_invalid_settings_remain_available_for_diagnosis() {
	let root = Scratch::new();
	let settings = Preferences {
		root: root.path().to_owned(),
		filename: "controls.json".into(),
	};
	let path = settings
		.folder(true)
		.unwrap()
		.unwrap()
		.join("controls.json");
	for invalid in [
		vec![b' '; MAX_BYTES + 1],
		br#"{"schema_version":2,"movement":"arrows"}"#.to_vec(),
	] {
		fs::write(&path, &invalid).unwrap();
		assert!(settings.load().is_err());
		assert_eq!(fs::read(&path).unwrap(), invalid);
	}
}

#[test]
fn a_nonregular_destination_is_preserved() {
	let root = Scratch::new();
	let settings = Preferences {
		root: root.path().to_owned(),
		filename: "controls.json".into(),
	};
	let path = settings
		.folder(true)
		.unwrap()
		.unwrap()
		.join("controls.json");
	fs::create_dir(&path).unwrap();
	assert!(settings.save(ControlsSettings::default()).is_err());
	assert!(path.is_dir());
}

#[test]
fn reads_v1_without_rewriting_and_v2_persists_a_remap() {
	use runtime::input::FaceButton;
	let root = Scratch::new();
	let settings = Preferences {
		root: root.path().to_owned(),
		filename: "controls.json".into(),
	};
	let path = settings
		.folder(true)
		.unwrap()
		.unwrap()
		.join("controls.json");
	let legacy = br#"{"schema_version":1,"movement":"arrows"}"#;
	fs::write(&path, legacy).unwrap();
	let mut controls = settings.load().unwrap();
	assert_eq!(controls.movement, Preset::Arrows);
	assert_eq!(controls.controller, ControllerBindings::default());
	assert_eq!(fs::read(&path).unwrap(), legacy);
	controls.controller.interact = FaceButton::B;
	settings.save(controls).unwrap();
	let persisted = fs::read(&path).unwrap();
	assert_eq!(settings.load().unwrap(), controls);
	controls.controller.jump = FaceButton::B;
	assert!(settings.save(controls).is_err());
	assert_eq!(fs::read(&path).unwrap(), persisted);
}

#[test]
fn display_settings_promote_v2_to_v3_only_on_apply_and_preserve_other_fields() {
	use super::super::{PlayerControls, display::TextSize};
	use runtime::input::FaceButton;

	let root = Scratch::new();
	let prefs = Preferences {
		root: root.path().to_owned(),
		filename: "controls.json".into(),
	};
	let path = prefs.folder(true).unwrap().unwrap().join("controls.json");
	let legacy =
		br#"{"schema_version":2,"movement":"wasd","controller":{"jump":"a","interact":"b"}}"#;
	fs::write(&path, legacy).unwrap();
	let settings = prefs.load().unwrap();
	assert_eq!(settings.display, DisplaySettings::default());
	assert_eq!(settings.movement, Preset::Wasd);
	assert_eq!(settings.controller.interact, FaceButton::B);
	assert_eq!(fs::read(&path).unwrap(), legacy);
	let mut controls = PlayerControls {
		applied: settings,
		preferences: Some(prefs),
		..PlayerControls::default()
	};
	let display = DisplaySettings {
		text_size: TextSize::ExtraLarge,
		high_contrast: true,
		cue_captions: false,
	};
	controls.apply_display(display).unwrap();
	controls.apply_movement(Preset::Arrows, None).unwrap();
	let mut controller = settings.controller;
	controller.interact = FaceButton::Y;
	controls.apply_controller(controller).unwrap();
	let prefs = controls.preferences.unwrap();
	assert_eq!(prefs.load().unwrap().display, display);
	assert_eq!(prefs.load().unwrap().movement, Preset::Arrows);
	assert_eq!(prefs.load().unwrap().controller, controller);
	let mut bad = serde_json::from_slice::<serde_json::Value>(&fs::read(&path).unwrap()).unwrap();
	assert_eq!(bad["schema_version"], 3);
	bad["display"]["text_size"] = "huge".into();
	let bad = serde_json::to_vec(&bad).unwrap();
	fs::write(&path, &bad).unwrap();
	assert!(prefs.load().is_err());
	assert_eq!(fs::read(&path).unwrap(), bad);
}
