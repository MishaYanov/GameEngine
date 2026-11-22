use engine_core::{
	EntityKey, asset_catalog::AssetId, project::ProjectData, scene_data::SceneData,
	simulation_session::SimulationSession,
};
use std::{
	fs,
	path::{Path, PathBuf},
	sync::Arc,
};

struct Fixture(PathBuf);
impl Fixture {
	fn new() -> Self {
		let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
			.join("../target")
			.join(format!(
				"qa-animation-package-{}",
				AssetId::generate().as_str()
			));
		fs::create_dir(&root).unwrap();
		Self(root)
	}
}
impl Drop for Fixture {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.0);
	}
}
fn source() -> PathBuf {
	Path::new(env!("CARGO_MANIFEST_DIR")).join("../apps/game")
}
fn simulation(root: &Path, project: ProjectData, scene: SceneData) -> SimulationSession {
	let mut session = SimulationSession::new(project, Arc::new(scene)).unwrap();
	let root = root.to_path_buf();
	session
		.set_animation_resolver(Arc::new(move |scene| {
			bridge::animation_asset::prepare_scene(&root, scene)
		}))
		.unwrap();
	session
}
#[test]
fn staged_animation_preserves_pins_clips_geometry_and_native_outcome() {
	let fixture = Fixture::new();
	let source = source();
	let project = ProjectData::from_json(&test_project::manifest("animation-room")).unwrap();
	let prepared = compiler::prepare_project(&project, &source.join("assets")).unwrap();
	let staged = compiler::stage_prepared_project(&prepared, fixture.0.to_str().unwrap()).unwrap();
	let root = staged.path().join("assets");
	assert_eq!(
		fs::read(source.join("assets/characters/two-joint.glb")).unwrap(),
		fs::read(root.join("characters/two-joint.glb")).unwrap()
	);
	let original = SceneData::from_json(
		&fs::read_to_string(source.join("assets/scenes/animation-room.json")).unwrap(),
	)
	.unwrap();
	let installed =
		SceneData::from_json(&fs::read_to_string(root.join("scenes/animation-room.json")).unwrap())
			.unwrap();
	assert_eq!(original, installed);
	let mut left = simulation(&source.join("assets"), project.clone(), original);
	let mut right = simulation(&root, project, installed);
	let player = EntityKey::new("player").unwrap();
	for session in [&mut left, &mut right] {
		for _ in 0..2 {
			session.tick(1. / 60.).unwrap();
		}
		session
			.set_character_input(&player, [1., 0.], true)
			.unwrap();
		session.tick(1. / 60.).unwrap();
		session
			.set_character_input(&player, [0., 0.], false)
			.unwrap();
		for _ in 0..90 {
			session.tick(1. / 60.).unwrap();
		}
	}
	assert_eq!(left.document(), right.document());
	assert_eq!(
		left.animations().replay_state(),
		right.animations().replay_state()
	);
	assert!(left.animations().pose(&player).unwrap().elapsed_ticks > 0);
}

#[test]
fn export_rejects_invalid_controller_and_stale_animation_before_output() {
	let fixture = Fixture::new();
	let source = source();
	let assets = fixture.0.join("assets");
	for directory in ["scenes", "characters", "props"] {
		fs::create_dir_all(assets.join(directory)).unwrap();
	}
	for file in [
		"asset-catalog.json",
		"characters/two-joint.glb",
		"props/cube.glb",
		"props/capsule.glb",
	] {
		fs::copy(source.join("assets").join(file), assets.join(file)).unwrap();
	}
	let project = ProjectData::from_json(&test_project::manifest("animation-room")).unwrap();
	let mut scene = SceneData::from_json(
		&fs::read_to_string(source.join("assets/scenes/animation-room.json")).unwrap(),
	)
	.unwrap();
	let scene_path = assets.join("scenes/animation-room.json");
	let player = scene
		.entities
		.iter_mut()
		.find(|entity| entity.key.as_str() == "player")
		.unwrap();
	player.transform.scale = [2., 2., 2.];
	fs::write(&scene_path, scene.to_json().unwrap()).unwrap();
	let error = compiler::prepare_project(&project, &assets).err().unwrap();
	assert!(error.contains("unit-scale"), "{error}");
	scene
		.entities
		.iter_mut()
		.find(|entity| entity.key.as_str() == "player")
		.unwrap()
		.transform
		.scale = [1., 1., 1.];
	scene
		.entities
		.iter_mut()
		.find(|entity| entity.key.as_str() == "player")
		.unwrap()
		.animation
		.as_mut()
		.unwrap()
		.settings
		.source
		.expected_sha256 = "0".repeat(64);
	fs::write(scene_path, scene.to_json().unwrap()).unwrap();
	assert!(compiler::prepare_project(&project, &assets).is_err());
	assert!(!fixture.0.join(&project.name).exists());
}

#[test]
fn moved_animation_model_keeps_reserved_path_alias_in_runtime_and_export() {
	let fixture = Fixture::new();
	let source = source();
	let assets = fixture.0.join("assets");
	for directory in ["scenes", "characters", "props"] {
		fs::create_dir_all(assets.join(directory)).unwrap();
	}
	for file in [
		"scenes/animation-room.json",
		"props/cube.glb",
		"props/capsule.glb",
	] {
		fs::copy(source.join("assets").join(file), assets.join(file)).unwrap();
	}
	fs::copy(
		source.join("assets/characters/two-joint.glb"),
		assets.join("characters/renamed.glb"),
	)
	.unwrap();
	let mut catalog: serde_json::Value = serde_json::from_str(
		&fs::read_to_string(source.join("assets/asset-catalog.json")).unwrap(),
	)
	.unwrap();
	catalog["assets"][0]["path"] = serde_json::json!("characters/renamed.glb");
	catalog["assets"][0]["aliases"] = serde_json::json!(["characters/two-joint.glb"]);
	fs::write(
		assets.join("asset-catalog.json"),
		serde_json::to_vec_pretty(&catalog).unwrap(),
	)
	.unwrap();
	let project = ProjectData::from_json(&test_project::manifest("animation-room")).unwrap();
	let scene = SceneData::from_json(
		&fs::read_to_string(assets.join("scenes/animation-room.json")).unwrap(),
	)
	.unwrap();
	let mut runtime = simulation(&assets, project.clone(), scene);
	runtime.tick(1. / 60.).unwrap();
	let prepared = compiler::prepare_project(&project, &assets).unwrap();
	let output = fixture.0.join("export");
	fs::create_dir(&output).unwrap();
	let staged = compiler::stage_prepared_project(&prepared, output.to_str().unwrap()).unwrap();
	assert!(
		staged
			.path()
			.join("assets/characters/renamed.glb")
			.is_file()
	);
	assert!(
		!staged
			.path()
			.join("assets/characters/two-joint.glb")
			.exists()
	);
	let installed = SceneData::from_json(
		&fs::read_to_string(staged.path().join("assets/scenes/animation-room.json")).unwrap(),
	)
	.unwrap();
	let mut exported = simulation(&staged.path().join("assets"), project, installed);
	exported.tick(1. / 60.).unwrap();
	assert_eq!(
		runtime.animations().replay_state(),
		exported.animations().replay_state()
	);
}

#[path = "../../tests/support/project_fixture.rs"]
mod test_project;
