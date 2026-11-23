use engine_core::{
	EntityKey, PrimitiveKind, SceneData,
	asset_catalog::{
		AssetCatalog, AssetId, AssetKind, CATALOG_FILE, MeshSignature, PersistentAsset,
	},
	editor::{EditorSession, PrefabData},
	linked_prefab::LinkedPrefabSource,
	project::ProjectData,
	scene_catalog::{SceneDescriptor, SceneId},
	scene_data::{CURRENT_SCHEMA_VERSION, EntityData},
};
use sha2::{Digest, Sha256};
use std::{
	fs,
	path::{Path, PathBuf},
};

const PLANE: &[u8] = include_bytes!("../../apps/editor-host/src-tauri/assets/primitives/plane.glb");
const CUBE: &[u8] = include_bytes!("../../apps/editor-host/src-tauri/assets/primitives/cube.glb");
const SPHERE: &[u8] =
	include_bytes!("../../apps/editor-host/src-tauri/assets/primitives/sphere.glb");
const CAPSULE: &[u8] =
	include_bytes!("../../apps/editor-host/src-tauri/assets/primitives/capsule.glb");
const IMPORTED: &[u8] = include_bytes!("../../apps/game/assets/props/cube.glb");

struct Fixture(PathBuf);

impl Fixture {
	fn new() -> Self {
		let root = Path::new(env!("CARGO_MANIFEST_DIR"))
			.join("../target")
			.join(format!("qa-primitives-{}", AssetId::generate().as_str()));
		fs::create_dir(&root).unwrap();
		Self(root)
	}
}

impl Drop for Fixture {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.0);
	}
}

fn scene(entities: Vec<EntityData>) -> SceneData {
	SceneData {
		schema_version: CURRENT_SCHEMA_VERSION,
		camera: Default::default(),
		default_game_camera: None,
		environment: Default::default(),
		organization: Default::default(),
		bindings: Default::default(),
		entities,
		lights: Vec::new(),
	}
}

fn asset(path: String, kind: AssetKind, bytes: &[u8]) -> PersistentAsset {
	PersistentAsset {
		id: AssetId::generate(),
		path,
		kind,
		aliases: Vec::new(),
		revision: 1,
		fingerprint: format!("{:x}", Sha256::digest(bytes)),
		bytes: bytes.len() as u64,
		source_path: None,
		meshes: Vec::new(),
		dependencies: Vec::new(),
	}
}

#[test]
fn primitives_survive_pinned_prop_two_scenes_and_export() {
	let fixture = Fixture::new();
	let assets = fixture.0.join("assets");
	let output = fixture.0.join("exports");
	fs::create_dir_all(assets.join("scenes")).unwrap();
	fs::create_dir_all(assets.join("prefabs")).unwrap();
	fs::create_dir_all(assets.join("models/primitives/v1")).unwrap();
	fs::create_dir(&output).unwrap();

	let meshes = [
		(PrimitiveKind::Plane, PLANE),
		(PrimitiveKind::Cube, CUBE),
		(PrimitiveKind::Sphere, SPHERE),
		(PrimitiveKind::Capsule, CAPSULE),
	];
	let mut catalog = AssetCatalog::default();
	let mut entities = Vec::new();
	let mut expected = Vec::new();
	for (kind, bytes) in meshes {
		let path = format!("models/primitives/v1/{}.glb", kind.as_str());
		fs::write(assets.join(&path), bytes).unwrap();
		let mut model = asset(path.clone(), AssetKind::Model, bytes);
		model.meshes.push(MeshSignature {
			index: 0,
			name: kind.display_name().into(),
			primitives: 1,
		});
		entities.push(
			EntityData::new(
				EntityKey::new(kind.as_str()).unwrap(),
				kind.display_name(),
				&path,
			)
			.with_asset_id(Some(model.id.clone())),
		);
		catalog.replace(model).unwrap();
		expected.push((path, bytes));
	}
	fs::write(assets.join("imported-cube.glb"), IMPORTED).unwrap();
	let imported = EntityData::new(
		EntityKey::new("imported").unwrap(),
		"Imported Cube",
		"imported-cube.glb",
	);
	let [plane, cube, sphere, capsule] = entities.try_into().ok().unwrap();
	let sphere_id = sphere.asset_id.clone().unwrap();
	let prefab = PrefabData {
		schema_version: 3,
		entities: vec![sphere],
		gameplay: None,
		reference_slots: Default::default(),
		composition: Default::default(),
	};
	let source_path = "prefabs/sphere.json";
	let bytes = prefab.to_json().unwrap();
	fs::write(assets.join(source_path), &bytes).unwrap();
	let mut source = asset(source_path.into(), AssetKind::Prefab, bytes.as_bytes());
	source.dependencies.push(sphere_id);
	catalog.replace(source.clone()).unwrap();
	fs::write(assets.join(CATALOG_FILE), catalog.to_json().unwrap()).unwrap();
	let main = scene(vec![plane, cube, imported]);
	let mut project =
		ProjectData::for_scene("Primitive Workshop", "scenes/main.json", &main).unwrap();
	let mut editor = EditorSession::for_project(main, project.clone()).unwrap();
	editor
		.instantiate_linked_prefab(
			LinkedPrefabSource {
				source_id: source.id,
				path: source.path,
				fingerprint: source.fingerprint,
				prefab,
			},
			[0.0; 3],
			Default::default(),
		)
		.unwrap();
	let main = editor.authored_document();
	fs::write(assets.join("scenes/main.json"), main.to_json().unwrap()).unwrap();

	let second = scene(vec![capsule]);
	let scene_id = SceneId::new("annex").unwrap();
	project
		.scenes
		.insert(SceneDescriptor::new(scene_id, "scenes/annex.json").unwrap())
		.unwrap();
	fs::write(assets.join("scenes/annex.json"), second.to_json().unwrap()).unwrap();

	let prepared = compiler::prepare_project(&project, &assets).unwrap();
	let staged = compiler::stage_prepared_project(&prepared, output.to_str().unwrap()).unwrap();
	let staged_assets = staged.path().join("assets");
	for (path, bytes) in &expected {
		assert_eq!(fs::read(staged_assets.join(path)).unwrap(), *bytes);
	}
	assert_eq!(
		fs::read(staged_assets.join("imported-cube.glb")).unwrap(),
		IMPORTED
	);
	assert!(staged_assets.join("scenes/annex.json").is_file());
	let exported_main =
		SceneData::from_json(&fs::read_to_string(staged_assets.join("scenes/main.json")).unwrap())
			.unwrap();
	assert!(
		exported_main
			.entities
			.iter()
			.any(|entity| { entity.renderable && entity.model_path == expected[2].0 })
	);

	let capsule_path = &expected[3].0;
	fs::remove_file(assets.join(capsule_path)).unwrap();
	let error = compiler::prepare_project(&project, &assets)
		.err()
		.expect("missing capsule must fail preflight");
	assert!(error.contains(capsule_path), "{error}");
}
