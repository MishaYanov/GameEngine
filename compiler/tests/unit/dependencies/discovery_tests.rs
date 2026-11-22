use super::prefab::prefab_paths_with_budget;
use super::*;
use crate::{prepare_project, stage_prepared_project};
use engine_core::{asset_catalog::CATALOG_FILE, editor::PrefabData};
struct Fixture {
	parent: PathBuf,
	root: PathBuf,
	project: ProjectData,
	scene: SceneData,
}
impl Fixture {
	fn new() -> Self {
		let parent = std::env::temp_dir().join(format!(
			"legacy-prefab-discovery-{}",
			AssetId::generate().as_str()
		));
		fs::create_dir_all(parent.join("assets/scenes")).unwrap();
		let root = parent.join("assets").canonicalize().unwrap();
		let mut scene = SceneData::from_json(include_str!(
			"../../../../apps/game/assets/scenes/robots.json"
		))
		.unwrap();
		scene.entities.clear();
		scene.default_game_camera = None;
		scene.bindings.clear();
		scene.camera.follow = None;
		fs::write(root.join("scenes/main.json"), scene.to_json().unwrap()).unwrap();
		let project =
			ProjectData::for_scene("Legacy prefab discovery", "scenes/main.json", &scene).unwrap();
		Self {
			parent,
			root,
			project,
			scene,
		}
	}
}
impl Drop for Fixture {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.parent);
	}
}
#[test]
fn uncataloged_prefabs_anywhere_in_assets_stage_with_their_model_dependencies() {
	let fixture = Fixture::new();
	fs::create_dir(fixture.root.join("models")).unwrap();
	fs::create_dir_all(fixture.root.join("old-assemblies/deep")).unwrap();
	let model = Path::new(env!("CARGO_MANIFEST_DIR")).join("../apps/game/assets/props/cube.glb");
	fs::copy(&model, fixture.root.join("models/prefab-only.glb")).unwrap();
	fs::copy(&model, fixture.root.join("models/unreferenced.glb")).unwrap();
	let prefab = PrefabData {
		composition: Default::default(),
		reference_slots: Default::default(),
		schema_version: 1,
		entities: vec![EntityData::new(
			engine_core::EntityKey::new("legacy").unwrap(),
			"Legacy cube",
			"models/prefab-only.glb",
		)],
		gameplay: None,
	};
	let source = prefab.to_json().unwrap();
	for path in ["legacy-root.json", "old-assemblies/deep/assembly.json"] {
		fs::write(fixture.root.join(path), &source).unwrap();
	}
	fs::write(
		fixture.root.join("other-scene.json"),
		fixture.scene.to_json().unwrap(),
	)
	.unwrap();
	fs::write(
		fixture.root.join("metadata.json"),
		r#"{"note":"not a prefab","entities":[]}"#,
	)
	.unwrap();
	let prepared = prepare_project(&fixture.project, &fixture.root).unwrap();
	let staged = stage_prepared_project(&prepared, fixture.parent.to_str().unwrap()).unwrap();
	let output = staged.path().join("assets");
	for path in ["legacy-root.json", "old-assemblies/deep/assembly.json"] {
		assert_eq!(fs::read(output.join(path)).unwrap(), source.as_bytes());
	}
	assert!(output.join("models/prefab-only.glb").is_file());
	assert!(output.join("scenes/main.json").is_file());
	assert!(!output.join("models/unreferenced.glb").exists());
	assert!(!output.join("metadata.json").exists());
	assert!(!output.join("other-scene.json").exists());
	assert!(!fixture.root.join(CATALOG_FILE).exists());
}
#[test]
fn global_discovery_charges_unknown_json_and_skips_registered_scene_documents() {
	let fixture = Fixture::new();
	for (name, bytes) in [
		("ordinary.json", b"{}".as_slice()),
		("malformed.json", b"no".as_slice()),
		("other.json", b"[]".as_slice()),
	] {
		fs::write(fixture.root.join(name), bytes).unwrap();
	}
	assert!(
		prefab_paths_with_budget(&fixture.root, &fixture.project, 5)
			.unwrap_err()
			.contains("JSON byte budget")
	);
	assert!(
		prefab_paths_with_budget(&fixture.root, &fixture.project, 6)
			.unwrap()
			.is_empty()
	);
}
#[test]
fn recognized_invalid_legacy_assemblies_are_reported_instead_of_silently_omitted() {
	let fixture = Fixture::new();
	fs::write(
		fixture.root.join("invalid-assembly.json"),
		r#"{"schema_version":1,"entities":[]}"#,
	)
	.unwrap();
	let error = prepare_project(&fixture.project, &fixture.root)
		.err()
		.unwrap();
	assert!(error.contains("invalid-assembly.json"), "{error}");
}
