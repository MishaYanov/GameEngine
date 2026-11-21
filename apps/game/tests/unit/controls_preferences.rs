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
