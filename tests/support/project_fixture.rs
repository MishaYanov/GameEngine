//! Generate temporary engine test projects from reusable scene fixtures.

#![allow(dead_code)] // Each integration-test binary uses a subset of this module.

use serde_json::Value;
use std::{
	fs,
	path::{Path, PathBuf},
	sync::atomic::{AtomicU64, Ordering},
	time::{SystemTime, UNIX_EPOCH},
};

pub fn manifest(scene: &str) -> String {
	let (name, behavior) = match scene {
		"animation-room" => ("Animated Character Room", None),
		"camera-room" => ("Camera Acceptance Room", None),
		"controller-room" => ("Character Controller Room", None),
		"component-lab" => ("Component Lifecycle Lab", Some("lab-player")),
		"event-lab" => ("Event Interaction Lab", Some("event-player")),
		_ => panic!("unknown test scene: {scene}"),
	};
	let mut scenes: Vec<Value> = vec![serde_json::json!({
		"id": scene, "asset_path": format!("scenes/{scene}.json"),
	})];
	if scene == "component-lab" {
		scenes.push(
			serde_json::json!({"id": "component-rest", "asset_path": "scenes/component-rest.json"}),
		);
	}
	let behaviors: Vec<_> = behavior
		.map(|entity| {
			serde_json::json!({
				"scene_id": scene,
				"behavior": {"kind": "controlled_move", "entity": entity, "speed": 2, "space": "world"},
			})
		})
		.into_iter()
		.collect();
	let mut project = serde_json::json!({
		"schema_version": 3, "name": name, "fixed_hz": 60, "max_catchup_steps": 8,
		"behaviors": behaviors, "scenes": scenes, "startup_scene_id": scene,
	});
	if behavior.is_none() {
		project["input_profile"] = serde_json::json!({
			"schema_version": 1,
			"movement": {"negative_x": "a", "positive_x": "d", "negative_y": "s", "positive_y": "w"},
			"actions": [{"id": "jump", "key": "space"}],
		});
	}
	project.to_string()
}

pub struct ProjectFixture {
	root: PathBuf,
}

impl ProjectFixture {
	pub fn new(scene: &str) -> Self {
		static NEXT: AtomicU64 = AtomicU64::new(0);
		let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
		let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
			"engine-project-fixture-{}-{}-{}",
			std::process::id(),
			now.as_nanos(),
			NEXT.fetch_add(1, Ordering::Relaxed),
		));
		fs::create_dir(&root).unwrap();
		let fixture = Self { root };
		let source = Path::new(env!("CARGO_MANIFEST_DIR"))
			.ancestors()
			.map(|path| path.join("apps/game/assets"))
			.find(|path| path.is_dir())
			.expect("shared engine scene test assets");
		let destination = fixture.root.join("assets");
		fs::create_dir(&destination).unwrap();
		for directory in ["characters", "prefabs", "props", "scenes", "textures"] {
			fs::create_dir(destination.join(directory)).unwrap();
			for entry in fs::read_dir(source.join(directory)).unwrap() {
				let entry = entry.unwrap();
				assert!(entry.file_type().unwrap().is_file());
				fs::copy(
					entry.path(),
					destination.join(directory).join(entry.file_name()),
				)
				.unwrap();
			}
		}
		fs::copy(
			source.join("asset-catalog.json"),
			destination.join("asset-catalog.json"),
		)
		.unwrap();
		fs::write(fixture.manifest_path(), manifest(scene)).unwrap();
		fixture
	}

	pub fn manifest_path(&self) -> PathBuf {
		self.root.join("project.json")
	}
}

impl Drop for ProjectFixture {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.root);
	}
}
