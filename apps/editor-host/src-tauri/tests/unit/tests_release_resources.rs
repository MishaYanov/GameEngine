use super::*;
use std::{
	fs,
	sync::atomic::{AtomicU64, Ordering},
};

struct TestDirectory(PathBuf);

impl TestDirectory {
	fn new() -> Self {
		static NEXT: AtomicU64 = AtomicU64::new(0);
		let root = std::env::temp_dir().join(format!(
			"game-engine-release-resource-{}-{}",
			std::process::id(),
			NEXT.fetch_add(1, Ordering::Relaxed),
		));
		fs::create_dir(&root).unwrap();
		Self(root)
	}

	fn directory(&self, relative: &str) -> PathBuf {
		let path = self.0.join(relative);
		fs::create_dir_all(&path).unwrap();
		path
	}
}

impl Drop for TestDirectory {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.0);
	}
}

#[test]
fn installed_resources_use_only_the_fixed_bundle_paths() {
	let root = TestDirectory::new();
	let installed = EditorResources::Installed(root.0.clone());
	assert!(EditorResources::Development.player().unwrap().is_none());

	let exports = root.directory("exports");
	let destination = exports.to_str().unwrap();
	assert!(installed
		.player()
		.unwrap_err()
		.contains("bundled Windows Player"));
	let missing_sample = installed
		.create_sample("My Sample", destination)
		.unwrap_err();
	assert!(missing_sample.contains("bundled Cinder Oath sample"));
	assert!(!exports.join("My Sample").exists());

	let player = root.0.join(BUNDLED_PLAYER);
	fs::create_dir_all(player.parent().unwrap()).unwrap();
	fs::write(&player, b"bundled player fixture").unwrap();
	assert_eq!(
		installed.player().unwrap().unwrap(),
		player.canonicalize().unwrap()
	);
}

#[test]
fn installed_sample_opens_an_editable_copy_outside_the_bundle() {
	let root = TestDirectory::new();
	let sample = root.0.join(BUNDLED_SAMPLE);
	let sample_root = sample.parent().unwrap();
	let sample_parent = sample_root.parent().unwrap();
	fs::create_dir_all(sample_parent).unwrap();
	compiler::create_authoring_project("cinder-oath", sample_parent.to_str().unwrap()).unwrap();
	let sample_bytes = fs::read(&sample).unwrap();

	let projects = root.directory("projects");
	let installed = EditorResources::Installed(root.0.clone());
	let created = installed
		.create_sample("My Sample", projects.to_str().unwrap())
		.unwrap();
	let copy = created.join("project.json");
	let project = runtime::project::ProjectData::load(&copy).unwrap();
	assert_eq!(project.name, "My Sample");
	assert_eq!(project.gameplay_module.as_deref(), Some("cinder_oath"));
	assert!(created.starts_with(&projects));
	assert_eq!(fs::read(&sample).unwrap(), sample_bytes);
}
