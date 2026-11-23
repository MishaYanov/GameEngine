use engine_core::{
	EntityKey, SceneData,
	asset_catalog::{AssetCatalog, AssetId, AssetKind, CATALOG_FILE, PersistentAsset},
	components::{
		ComponentData, ComponentId, ComponentRecord, NO_MESH_PATH, SkyBackground, SkySettings,
	},
	project::ProjectData,
	scene_data::{CURRENT_SCHEMA_VERSION, EntityData},
};
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf};

const SKY: &[u8] = include_bytes!("fixtures/sky-panorama-4x2.png");

struct Fixture(PathBuf);
impl Drop for Fixture {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.0);
	}
}

#[test]
fn authored_skies_share_exact_staged_png_and_broken_asset_reports_the_owner() {
	let fixture =
		Fixture(std::env::temp_dir().join(format!("sky-package-{}", AssetId::generate().as_str())));
	let root = fixture.0.join("assets");
	let output = fixture.0.join("exports");
	fs::create_dir_all(root.join("scenes")).unwrap();
	fs::create_dir_all(&output).unwrap();
	fs::write(root.join("panorama.png"), SKY).unwrap();
	let image = AssetId::generate();
	let asset = PersistentAsset {
		id: image.clone(),
		path: "panorama.png".into(),
		kind: AssetKind::SkyPanorama,
		revision: 1,
		fingerprint: format!("{:x}", Sha256::digest(SKY)),
		bytes: SKY.len() as u64,
		aliases: Vec::new(),
		source_path: None,
		meshes: Vec::new(),
		dependencies: Vec::new(),
	};
	fs::write(
		root.join(CATALOG_FILE),
		AssetCatalog {
			schema_version: 1,
			assets: vec![asset.clone()],
		}
		.to_json()
		.unwrap(),
	)
	.unwrap();
	let objects = [("morning", 0.0), ("evening", 179.0)]
		.into_iter()
		.map(|(key, yaw_degrees)| {
			let mut sky = EntityData::new(EntityKey::new(key).unwrap(), key, NO_MESH_PATH)
				.with_renderable(false);
			sky.put_component(
				ComponentRecord {
					id: ComponentId::new("sky").unwrap(),
					schema_version: 1,
					enabled: true,
					data: ComponentData::Sky {
						settings: SkySettings {
							background: SkyBackground::Panorama {
								image: image.clone(),
							},
							yaw_degrees,
							..Default::default()
						},
					},
				},
				false,
			)
			.unwrap();
			sky
		})
		.collect();
	let scene = SceneData {
		schema_version: CURRENT_SCHEMA_VERSION,
		entities: objects,
		bindings: Default::default(),
		camera: Default::default(),
		default_game_camera: None,
		lights: Vec::new(),
		organization: Default::default(),
		environment: engine_core::scene_data::SceneEnvironment {
			active_sky: Some(EntityKey::new("morning").unwrap()),
			..Default::default()
		},
	};
	let project = ProjectData::for_scene("Panorama", "scenes/main.json", &scene).unwrap();
	fs::write(root.join("scenes/main.json"), scene.to_json().unwrap()).unwrap();
	let prepared = compiler::prepare_project(&project, &root).unwrap();
	let staged = compiler::stage_prepared_project(&prepared, output.to_str().unwrap()).unwrap();
	let staged_root = staged.path().join("assets");
	fs::remove_file(root.join("panorama.png")).unwrap();
	let file = bridge::sky_panorama::read_project_bytes(&staged_root, &asset.path).unwrap();
	assert_eq!(file, SKY);
	let image = bridge::sky_panorama::decode(&file).unwrap();
	assert_eq!((image.info.width, image.info.height), (4, 2));
	assert_eq!(&image.rgba8[..4], &[255, 0, 0, 255]);
	let error = compiler::prepare_project(&project, &root).err().unwrap();
	assert!(
		error.contains("morning") && error.contains("panorama.png"),
		"{error}"
	);
	let mut nonsquare = file;
	nonsquare[16..20].copy_from_slice(&3u32.to_be_bytes());
	assert!(
		bridge::sky_panorama::decode(&nonsquare)
			.err()
			.unwrap()
			.contains("exact 2:1")
	);
}
