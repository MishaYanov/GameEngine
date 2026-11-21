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
	assert_eq!(settings.load().unwrap(), Preset::Project);
	settings.save(Preset::Arrows).unwrap();
	assert_eq!(settings.load().unwrap(), Preset::Arrows);
	settings.save(Preset::Wasd).unwrap();
	assert_eq!(settings.load().unwrap(), Preset::Wasd);
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
	assert!(settings.save(Preset::Arrows).is_err());
	assert!(path.is_dir());
}
