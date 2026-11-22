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
	settings.save(&controls).unwrap();
	assert_eq!(settings.load().unwrap(), controls);
	let controls = ControlsSettings {
		movement: Preset::Wasd,
		..controls
	};
	settings.save(&controls).unwrap();
	assert_eq!(settings.load().unwrap(), controls);
	let folder = root.path().join("GameEngine/controls");
	assert!(folder.join("controls.json").is_file());
	assert!(folder.join("controls.lock").is_file());
	assert_eq!(fs::read_dir(folder).unwrap().count(), 2);
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
	assert!(settings.save(&ControlsSettings::default()).is_err());
	assert!(path.is_dir());
}

#[test]
fn feedback_audio_apply_persists_level_beneath_mute_without_changing_other_preferences() {
	use super::super::{GameApp, Key};

	let root = Scratch::new();
	let prefs = Preferences {
		root: root.path().to_owned(),
		filename: "controls.json".into(),
	};
	let path = prefs.folder(true).unwrap().unwrap().join("controls.json");
	let legacy = br#"{"schema_version":3,"movement":"wasd","controller":{"jump":"a","interact":"b"},"display":{"text_size":"extra_large","high_contrast":true,"cue_captions":true}}"#;
	fs::write(&path, legacy).unwrap();
	let settings = prefs.load().unwrap();
	assert_eq!(settings.audio, FeedbackMix::default());
	assert_eq!(fs::read(&path).unwrap(), legacy);

	let mut app = GameApp::default();
	app.player_controls.applied = settings.clone();
	app.player_controls.preferences = Some(prefs);
	for key in [
		Key::Escape,
		Key::ArrowUp,
		Key::ArrowUp,
		Key::ArrowUp,
		Key::Enter,
	] {
		assert!(app.player_controls_key(key));
	}
	for _ in 0..7 {
		app.player_controls_key(Key::ArrowLeft);
	}
	for key in [Key::ArrowDown, Key::Enter, Key::ArrowDown, Key::Enter] {
		app.player_controls_key(key);
	}

	let prefs = app.player_controls.preferences.as_ref().unwrap();
	let saved = prefs.load().unwrap();
	assert_eq!(app.player_controls.feedback_mix(), saved.audio);
	assert_eq!(saved.audio.level_percent, 30);
	assert!(saved.audio.muted);
	assert_eq!(saved.audio.effective_percent(), 0);
	assert_eq!(saved.movement, settings.movement);
	assert_eq!(saved.controller, settings.controller);
	assert_eq!(saved.display, settings.display);
	assert_eq!(saved.actions, settings.actions);

	let mut malformed =
		serde_json::from_slice::<serde_json::Value>(&fs::read(&path).unwrap()).unwrap();
	assert_eq!(malformed["schema_version"], 4);
	malformed["audio"]["level_percent"] = 101.into();
	let malformed = serde_json::to_vec(&malformed).unwrap();
	fs::write(&path, &malformed).unwrap();
	assert!(prefs.load().is_err());
	assert_eq!(fs::read(&path).unwrap(), malformed);
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
	settings.save(&controls).unwrap();
	let persisted = fs::read(&path).unwrap();
	assert_eq!(settings.load().unwrap(), controls);
	controls.controller.jump = FaceButton::B;
	assert!(settings.save(&controls).is_err());
	assert_eq!(fs::read(&path).unwrap(), persisted);
}

#[test]
fn display_settings_promote_v2_to_v4_only_on_apply_and_preserve_other_fields() {
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
		applied: settings.clone(),
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
	assert_eq!(bad["schema_version"], 4);
	bad["display"]["text_size"] = "huge".into();
	let bad = serde_json::to_vec(&bad).unwrap();
	fs::write(&path, &bad).unwrap();
	assert!(prefs.load().is_err());
	assert_eq!(fs::read(&path).unwrap(), bad);
}

#[test]
fn v3_loads_unchanged_and_explicit_keyboard_apply_persists_a_v4_draft() {
	use super::super::{GameApp, Key};
	let root = Scratch::new();
	let prefs = Preferences {
		root: root.path().to_owned(),
		filename: "controls.json".into(),
	};
	let path = prefs.folder(true).unwrap().unwrap().join("controls.json");
	let legacy = br#"{"schema_version":3,"movement":"project","controller":{"jump":"a","interact":"b"},"display":{"text_size":"extra_large","high_contrast":true,"cue_captions":true}}"#;
	fs::write(&path, legacy).unwrap();
	let settings = prefs.load().unwrap();
	assert_eq!(fs::read(&path).unwrap(), legacy);
	assert!(settings.actions.is_empty());
	assert_eq!(settings.audio, FeedbackMix::default());
	let project = runtime::project::ProjectData::from_json(include_str!(
		"../../project.release-learning.json"
	))
	.unwrap();
	let mut app = GameApp::default();
	app.project = Some(project);
	app.player_controls.applied = settings;
	app.player_controls.preferences = Some(prefs);

	let open_keyboard = |app: &mut GameApp| {
		if !app.player_controls.is_open() {
			app.player_controls_key(Key::Escape);
		}
		for _ in 0..12 {
			if app.player_controls.rows_with_capacity(1).unwrap()[0] == "> Keyboard" {
				break;
			}
			assert!(app.player_controls_key(Key::ArrowDown));
		}
		assert_eq!(
			app.player_controls.rows_with_capacity(1).unwrap(),
			["> Keyboard"]
		);
		app.player_controls_key(Key::Enter);
	};
	open_keyboard(&mut app);
	app.player_controls_key(Key::Enter);
	app.player_controls_key(Key::F);
	app.player_controls_key(Key::ArrowDown);
	app.player_controls_key(Key::Enter);
	assert_eq!(
		app.effective_input_profile().unwrap().actions[0].key,
		Key::F
	);
	let persisted = fs::read(&path).unwrap();
	let record: serde_json::Value = serde_json::from_slice(&persisted).unwrap();
	assert_eq!(record["schema_version"], 4);
	assert_eq!(record["actions"][0]["action_id"], "interact");
	assert_eq!(record["display"]["text_size"], "extra_large");
	assert_eq!(record["controller"]["interact"], "b");
	assert_eq!(
		app.player_controls
			.preferences
			.as_ref()
			.unwrap()
			.load()
			.unwrap()
			.actions[0]
			.key,
		Key::F
	);

	open_keyboard(&mut app);
	assert_eq!(
		app.player_controls.rows_with_capacity(1).unwrap(),
		["> interact: F"]
	);
	app.player_controls_key(Key::ArrowDown);
	app.player_controls_key(Key::ArrowDown);
	app.player_controls_key(Key::Enter);
	assert_eq!(fs::read(&path).unwrap(), persisted);
	app.player_controls_key(Key::ArrowUp);
	app.player_controls_key(Key::Enter);
	assert_eq!(
		app.effective_input_profile().unwrap().actions[0].key,
		Key::E
	);
	assert!(app.player_controls.applied.actions.is_empty());
}
