use engine_core::{
	EntityKey, SceneData,
	asset_catalog::{
		AssetCatalog, AssetId, AssetKind, CATALOG_FILE, MeshSignature, PersistentAsset,
	},
	project::ProjectData,
	scene_data::{CURRENT_SCHEMA_VERSION, EntityData},
};
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf};

const WAVE: &[u8] = include_bytes!("../../projects/cinder-oath/assets/audio/blade-hit.wav");
const CUBE: &[u8] = include_bytes!("../../apps/game/assets/props/cube.glb");

struct Fixture(PathBuf);
impl Drop for Fixture {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.0);
	}
}

fn asset(kind: AssetKind, path: &str, bytes: &[u8]) -> PersistentAsset {
	PersistentAsset {
		id: AssetId::generate(),
		kind,
		path: path.into(),
		revision: 1,
		fingerprint: format!("{:x}", Sha256::digest(bytes)),
		bytes: bytes.len() as u64,
		aliases: Vec::new(),
		source_path: None,
		meshes: Vec::new(),
		dependencies: Vec::new(),
	}
}

#[test]
fn packaging_copies_only_reachable_verified_audio_clips() {
	let fixture = Fixture(std::env::temp_dir().join(format!(
		"engine-audio-package-{}",
		AssetId::generate().as_str()
	)));
	let root = fixture.0.join("assets");
	let output = fixture.0.join("exports");
	fs::create_dir_all(root.join("scenes")).unwrap();
	fs::create_dir_all(&output).unwrap();
	fs::write(root.join("cube.glb"), CUBE).unwrap();
	fs::write(root.join("bell.wav"), WAVE).unwrap();
	fs::write(root.join("unused.wav"), WAVE).unwrap();
	let clip = asset(AssetKind::AudioClip, "bell.wav", WAVE);
	let unused = asset(AssetKind::AudioClip, "unused.wav", WAVE);
	let mut model = asset(AssetKind::Model, "cube.glb", CUBE);
	model.meshes.push(MeshSignature {
		index: 0,
		name: "cube".into(),
		primitives: 1,
	});
	// Use the generic typed dependency edge until scenes gain Audio Source components.
	model.dependencies.push(clip.id.clone());
	let cube = EntityData::new(EntityKey::new("cube").unwrap(), "Cube", "cube.glb")
		.with_asset_id(Some(model.id.clone()));
	let scene = SceneData {
		schema_version: CURRENT_SCHEMA_VERSION,
		bindings: Default::default(),
		entities: vec![cube],
		camera: Default::default(),
		default_game_camera: None,
		lights: Vec::new(),
		organization: Default::default(),
	};
	let catalog = AssetCatalog {
		schema_version: 1,
		assets: vec![clip.clone(), unused.clone(), model],
	};
	fs::write(root.join(CATALOG_FILE), catalog.to_json().unwrap()).unwrap();
	fs::write(root.join("scenes/main.json"), scene.to_json().unwrap()).unwrap();
	let project = ProjectData::for_scene("Audio Closure", "scenes/main.json", &scene).unwrap();
	let prepared = compiler::prepare_project(&project, &root).unwrap();
	let staged = compiler::stage_prepared_project(&prepared, output.to_str().unwrap()).unwrap();
	let copied = staged.path().join("assets");
	assert_eq!(fs::read(copied.join("bell.wav")).unwrap(), WAVE);
	assert!(!copied.join("unused.wav").exists());
	let installed =
		AssetCatalog::from_json(&fs::read_to_string(copied.join(CATALOG_FILE)).unwrap()).unwrap();
	assert!(installed.by_id(&unused.id).is_none());
	assert_eq!(installed.by_id(&clip.id), Some(&clip));
	fs::write(root.join("bell.wav"), b"no longer the reviewed bytes").unwrap();
	assert!(
		compiler::prepare_project(&project, &root)
			.err()
			.unwrap()
			.contains("changed outside")
	);
	// A Player reads the packaged bytes against the copied root after the
	// authoring file has changed or disappeared.
	let copied_clip = bridge::audio_clip::read_project(&copied, &clip.path).unwrap();
	assert_eq!(copied_clip.wav_bytes(), WAVE);
	assert_eq!(copied_clip.info().sample_rate_hz, 16_000);
	let mut invalid = WAVE.to_vec();
	invalid[20] = 3; // IEEE float is outside the supported signed PCM format.
	fs::write(root.join("bell.wav"), &invalid).unwrap();
	let mut catalog = catalog;
	let mut invalid_record = clip;
	invalid_record.fingerprint = format!("{:x}", Sha256::digest(&invalid));
	catalog.replace(invalid_record).unwrap();
	fs::write(root.join(CATALOG_FILE), catalog.to_json().unwrap()).unwrap();
	assert!(
		compiler::prepare_project(&project, &root)
			.err()
			.unwrap()
			.contains("16-bit mono")
	);
}
