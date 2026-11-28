use engine_core::{
	Camera, EntityKey, InputState, Key, SceneData,
	asset_catalog::{AssetCatalog, AssetId, AssetKind, PersistentAsset},
	character_controller::{CharacterControllerComponent, CharacterControllerSettings},
	components::{
		ComponentId, InputActionsRef, NO_MESH_PATH, NativeBehaviorComponent, NativeBehaviorSpec,
		ParticleBurstSettings, ParticleEmitterSettings, PlayerControllerComponent,
		PlayerControllerSettings, PlayerMovementSpace, ProjectileLauncherSettings,
		ProjectileSettings,
	},
	editor::PrefabData,
	game_session::GameSession,
	input::{GameInputBinding, InputProfile},
	project::ProjectData,
	scene_data::{CURRENT_SCHEMA_VERSION, CameraData, ColliderData, EntityData, TransformData},
	simulation_session::ConfiguredBehaviorFactory,
};
use sha2::{Digest, Sha256};
use std::{
	fs,
	path::{Path, PathBuf},
	sync::Arc,
};

const PROP: &str = "props/orb.json";
const SPRITE: &str = "textures/burst.png";
const SCENE: &str = "scenes/main.json";
const PNG: &[u8] = include_bytes!("fixtures/sky-panorama-4x2.png");

struct Fixture(PathBuf);

impl Fixture {
	fn new() -> Self {
		let root = Path::new(env!("CARGO_MANIFEST_DIR"))
			.join("../target")
			.join(format!(
				"qa-projectile-package-{}",
				AssetId::generate().as_str()
			));
		for directory in ["assets/scenes", "assets/props", "assets/textures"] {
			fs::create_dir_all(root.join(directory)).unwrap();
		}
		Self(root)
	}
}

impl Drop for Fixture {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.0);
	}
}

#[test]
fn launcher_prop_and_impact_sprite_run_from_the_staged_assets() {
	let fixture = Fixture::new();
	let root = fixture.0.join("assets");
	let sprite = AssetId::generate();
	let mut owner = object("launcher")
		.with_transform(TransformData {
			position: [0., 0.901, 0.],
			..Default::default()
		})
		.with_collider(Some(ColliderData::Box {
			half_extents: [0.3, 0.9, 0.3],
			offset: [0.; 3],
			interaction: Default::default(),
		}));
	owner.character_controller = Some(CharacterControllerComponent {
		id: ComponentId::new("motor").unwrap(),
		enabled: true,
		settings: CharacterControllerSettings::default(),
	});
	owner.player_controller = Some(Box::new(PlayerControllerComponent {
		id: ComponentId::new("input").unwrap(),
		enabled: true,
		settings: PlayerControllerSettings {
			input: InputActionsRef::ProjectDefault,
			local: true,
			space: PlayerMovementSpace::World,
			control_graph: None,
		},
	}));
	owner.native_behaviors.push(behavior(
		"launch",
		NativeBehaviorSpec::ProjectileLauncher {
			settings: ProjectileLauncherSettings {
				prefab_path: PROP.into(),
				action: "launch".into(),
				max_alive: 2,
				spawn_offset: [0., 0., -0.4],
			},
		},
	));
	let target = object("target")
		.with_transform(TransformData {
			position: [0., 0.9, -0.78],
			..Default::default()
		})
		.with_collider(Some(ColliderData::Box {
			half_extents: [0.1, 0.1, 0.05],
			offset: [0.; 3],
			interaction: Default::default(),
		}));
	let camera = Camera::default();
	let scene = SceneData {
		schema_version: CURRENT_SCHEMA_VERSION,
		organization: Default::default(),
		default_game_camera: None,
		environment: Default::default(),
		bindings: Default::default(),
		entities: vec![owner, target],
		lights: vec![],
		camera: CameraData {
			follow: None,
			orbit: None,
			position: camera.position,
			yaw: camera.yaw,
			pitch: camera.pitch,
			vertical_fov: camera.vertical_fov,
			near_plane: camera.near,
			far_plane: camera.far,
		},
	};
	fs::write(root.join(SCENE), scene.to_json().unwrap()).unwrap();
	let mut project_template = scene.clone();
	project_template.entities[0].player_controller = None;
	let mut project =
		ProjectData::for_scene("Projectile Fixture", SCENE, &project_template).unwrap();
	project.input_profile = Some(InputProfile {
		actions: vec![
			GameInputBinding {
				id: "launch".into(),
				key: Key::F,
				animation_parameter: None,
			},
			GameInputBinding {
				id: "jump".into(),
				key: Key::Space,
				animation_parameter: None,
			},
		],
		..Default::default()
	});
	project.fixed_hz = 60;
	let mut body = object("orb");
	body.native_behaviors.push(behavior(
		"motion",
		NativeBehaviorSpec::Projectile {
			settings: ProjectileSettings {
				speed: 24.,
				lifetime_ticks: 8,
				collision_mask: 1,
				impact: Some(ParticleBurstSettings {
					count: 3,
					settings: ParticleEmitterSettings {
						sprite_asset_id: Some(sprite.clone()),
						speed: 0.,
						gravity: 0.,
						..Default::default()
					},
				}),
			},
		},
	));
	let prefab = PrefabData {
		schema_version: 3,
		composition: Default::default(),
		entities: vec![body],
		gameplay: None,
		reference_slots: Default::default(),
	};
	let prop_bytes = prefab.to_json().unwrap().into_bytes();
	fs::write(root.join(PROP), &prop_bytes).unwrap();
	fs::write(root.join(SPRITE), PNG).unwrap();
	let catalog = AssetCatalog {
		assets: vec![
			record(AssetId::generate(), PROP, AssetKind::Prefab, &prop_bytes),
			record(sprite.clone(), SPRITE, AssetKind::Texture, PNG),
		],
		..Default::default()
	};
	fs::write(root.join("asset-catalog.json"), catalog.to_json().unwrap()).unwrap();
	let prepared = compiler::prepare_project(&project, &root).unwrap();
	let installed =
		compiler::stage_prepared_project(&prepared, fixture.0.to_str().unwrap()).unwrap();
	let assets = installed.path().join("assets");
	assert_eq!(fs::read(assets.join(SPRITE)).unwrap(), PNG);
	let game =
		ProjectData::from_json(&fs::read_to_string(installed.path().join("project.json")).unwrap())
			.unwrap();
	let mut keys = InputState::default();
	keys.press(Key::F);
	let input = game.input_profile.as_ref().unwrap().resolve_keyboard(&keys);
	let loaded = SceneData::from_json(&fs::read_to_string(assets.join(SCENE)).unwrap()).unwrap();
	let mut session = GameSession::with_factory_for_scene(
		Arc::new(game),
		project.startup_scene_id,
		Arc::new(loaded),
		Arc::new(ConfiguredBehaviorFactory),
	)
		.unwrap();
	let scope = session.active().local_input_scope().unwrap();
	session
		.active_mut()
		.submit_local_controls(&scope, 1, input)
		.unwrap();
	let step = 1.0 / f64::from(project.fixed_hz);
	session.tick(step).unwrap();
	let requests = session.active().pending_runtime_spawns();
	assert_eq!(requests.len(), 1);
	let launch = &requests[0];
	assert_eq!(launch.prefab_path, PROP);
	let file = fs::read_to_string(assets.join(&launch.prefab_path)).unwrap();
	let prop = PrefabData::from_json(&file).unwrap();
	let prepared = session
		.active()
		.prepare_runtime_spawn(
			&prop,
			&format!("{:x}", Sha256::digest(file.as_bytes())),
			launch.position,
			Some(launch.id),
		)
		.unwrap();
	let spawned = prepared.entities()[0].key.clone();
	session.active_mut().commit_runtime_spawn(prepared).unwrap();
	session.tick(0.).unwrap();
	session.tick(step).unwrap();
	assert!(session.active().runtime_handle(&spawned).is_none());
	let samples: Vec<_> = session.active().particles().collect();
	assert_eq!(samples.len(), 3);
	assert!(
		samples
			.iter()
			.all(|sample| sample.sprite_asset_id.as_deref() == Some(&sprite))
	);
}

fn object(id: &str) -> EntityData {
	EntityData::new(EntityKey::new(id).unwrap(), id, NO_MESH_PATH).with_renderable(false)
}

fn behavior(id: &str, behavior: NativeBehaviorSpec) -> NativeBehaviorComponent {
	NativeBehaviorComponent {
		id: ComponentId::new(id).unwrap(),
		enabled: true,
		behavior,
	}
}

fn record(id: AssetId, path: &str, kind: AssetKind, bytes: &[u8]) -> PersistentAsset {
	PersistentAsset {
		id,
		path: path.into(),
		kind,
		revision: 1,
		aliases: vec![],
		fingerprint: format!("{:x}", Sha256::digest(bytes)),
		bytes: bytes.len() as u64,
		source_path: None,
		meshes: vec![],
		dependencies: vec![],
	}
}
