use engine_core::{EntityKey, SceneData, asset_catalog::AssetId, linked_prefab::resolve_scene};
use serde_json::{Value, json};
use std::{
	fs,
	path::{Path, PathBuf},
	process::Command,
};

const WORKSHOP: &str = "tests/fixtures/primitive-workshop";
const SCENE: &str = "scenes/workshop.json";

struct Scratch(PathBuf);

impl Scratch {
	fn new() -> Self {
		let path = std::env::temp_dir().canonicalize().unwrap().join(format!(
			"primitive-workshop-{}",
			AssetId::generate().as_str()
		));
		fs::create_dir(&path).unwrap();
		Self(path)
	}
}

impl Drop for Scratch {
	fn drop(&mut self) {
		if self
			.0
			.canonicalize()
			.is_ok_and(|resolved| resolved == self.0)
		{
			let _ = fs::remove_dir_all(&self.0);
		}
	}
}

fn replay(project: &Path, route: &Path, state: &Path, foreign: &Path) -> (Value, SceneData) {
	let output = Command::new(env!("CARGO_BIN_EXE_engine-headless"))
		.current_dir(foreign)
		.arg("--project")
		.arg(project)
		.arg("--replay")
		.arg(route)
		.arg("--state-output")
		.arg(state)
		.output()
		.unwrap();
	assert!(
		output.status.success(),
		"{}",
		String::from_utf8_lossy(&output.stderr)
	);
	let scene = SceneData::from_json(&fs::read_to_string(state).unwrap()).unwrap();
	(serde_json::from_slice(&output.stdout).unwrap(), scene)
}

fn gate_yaw(scene: &SceneData, key: &EntityKey) -> f32 {
	scene
		.entities
		.iter()
		.find(|entity| &entity.key == key)
		.unwrap()
		.transform
		.rotation[1]
}

fn onsets(report: &Value) -> Vec<&Value> {
	report["scene_audio_events"]
		.as_array()
		.unwrap()
		.iter()
		.filter(|entry| entry["kind"] == "trigger")
		.map(|entry| &entry["onset"])
		.collect()
}

#[test]
fn workshop_opens_nested_gate_and_restarts_after_source_and_package_replays() {
	let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(WORKSHOP);
	let assets = root.join("assets");
	let manifest = root.join("project.json");
	let route = root.join("route.replay.json");
	let path = assets.join(SCENE);
	let authored = fs::read_to_string(&path).unwrap();
	let scene = SceneData::from_json(&authored).unwrap();
	let floor = &scene.organization.folders[0];
	assert_eq!(floor.name, "Floor");
	assert_eq!(scene.organization.root_membership.len(), 2);
	assert!(
		scene
			.organization
			.root_membership
			.values()
			.all(|id| id == &floor.id)
	);
	let player = scene.bindings.get("player").unwrap();
	assert_eq!(player.as_str(), "player");
	assert_eq!(
		scene.default_game_camera.as_ref().unwrap().as_str(),
		"camera"
	);
	let gate = scene
		.entities
		.iter()
		.find_map(|entity| entity.linked_prefab.as_ref())
		.unwrap();
	assert_eq!(gate.source.prefab.composition.placements.len(), 1);
	let gate_key = &gate.entity_map[&EntityKey::new("gate").unwrap()];
	let resolved = resolve_scene(&scene);
	assert!(
		resolved
			.entities
			.iter()
			.any(|entity| entity.name == "Signal Cap" && entity.parent.is_some())
	);

	let scratch = Scratch::new();
	let destination = scratch.0.join("staged");
	let unrelated = scratch.0.join("unrelated");
	fs::create_dir(&destination).unwrap();
	fs::create_dir(&unrelated).unwrap();
	let project = engine_core::project::ProjectData::load(&manifest).unwrap();
	let prepared = compiler::prepare_project(&project, &assets).unwrap();
	let staged =
		compiler::stage_prepared_project(&prepared, destination.to_str().unwrap()).unwrap();
	let packaged = staged.path().join("project.json");
	let canonical =
		Path::new(env!("CARGO_MANIFEST_DIR")).join("../editor-host/src-tauri/assets/primitives");
	for name in ["plane", "cube", "sphere", "capsule"] {
		let relative = format!("models/primitives/v1/{name}.glb");
		let model = fs::read(assets.join(&relative)).unwrap();
		assert_eq!(
			model,
			fs::read(canonical.join(format!("{name}.glb"))).unwrap()
		);
		assert_eq!(
			model,
			fs::read(staged.path().join("assets").join(relative)).unwrap()
		);
	}

	let (source_report, opened) = replay(
		&manifest,
		&route,
		&scratch.0.join("open-source.json"),
		&unrelated,
	);
	let (package_report, opened_package) = replay(
		&packaged,
		&route,
		&scratch.0.join("open-package.json"),
		&unrelated,
	);
	assert_eq!(source_report["markers"], package_report["markers"]);
	assert_eq!(source_report["markers"][1]["label"], "gate-open");
	assert_eq!(
		source_report["camera_runtime_hash"],
		package_report["camera_runtime_hash"]
	);
	assert_eq!(source_report["state_hash"], package_report["state_hash"]);
	assert_eq!(opened, opened_package);
	assert_eq!(
		source_report["native_components"]["characters"]["states"]["player"]["grounded"],
		true
	);
	assert!((gate_yaw(&opened, gate_key) - std::f32::consts::FRAC_PI_2).abs() < 0.01);
	let onsets_source = onsets(&source_report);
	assert_eq!(onsets_source, onsets(&package_report));
	assert_eq!(onsets_source.len(), 1);
	assert_eq!(onsets_source[0]["voice"]["entity"], "workshop-bell");
	assert_eq!(onsets_source[0]["cause"]["marker"], "bell-contact");

	let mut restarted_route: Value = serde_json::from_slice(&fs::read(&route).unwrap()).unwrap();
	restarted_route["actions"].as_array_mut().unwrap().extend([
		json!({"type":"restart_run"}),
		json!({"type":"tick", "count":4}),
		json!({"type":"mark", "label":"restarted"}),
	]);
	let restarted_path = scratch.0.join("restart.replay.json");
	fs::write(
		&restarted_path,
		serde_json::to_vec(&restarted_route).unwrap(),
	)
	.unwrap();
	let (restarted_source, reset_scene) = replay(
		&manifest,
		&restarted_path,
		&scratch.0.join("reset-source.json"),
		&unrelated,
	);
	let (restarted_package, reset_package) = replay(
		&packaged,
		&restarted_path,
		&scratch.0.join("reset-package.json"),
		&unrelated,
	);
	assert_eq!(restarted_source["markers"], restarted_package["markers"]);
	assert_eq!(restarted_source["markers"][2]["simulation_tick"], 4);
	assert_eq!(reset_scene, reset_package);
	assert!(gate_yaw(&reset_scene, gate_key).abs() < 0.0001);
	assert_eq!(fs::read_to_string(path).unwrap(), authored);
}
