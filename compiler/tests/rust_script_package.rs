//! Exercise project script source through Editor Play and a staged Player project.

use std::{
	fs,
	path::{Path, PathBuf},
	sync::Arc,
};

use engine_core::{
	EntityKey, SceneData,
	asset_catalog::{AssetCatalog, AssetId, AssetKind, CATALOG_FILE, PersistentAsset},
	command::{EditorCommandDto, EditorCommandRequest},
	components::{
		ComponentId, NO_MESH_PATH, NativeBehaviorComponent, NativeBehaviorSpec,
		ParticleBurstSettings, ProjectileLauncherSettings, ProjectileSettings, ScriptComponent,
		ScriptProperty, ScriptReference, ScriptSettings, project_script_id,
	},
	editor::{EditorSession, PrefabData},
	game_session::GameSession,
	project::ProjectData,
	scene_data::{CURRENT_SCHEMA_VERSION, ColliderData, EntityData, TransformData},
	world::RuntimeWorld,
};
use gameplay_catalog::{project_resolver, project_scripts::ScriptSource};
use sha2::{Digest, Sha256};

const SCENE_PATH: &str = "scenes/main.json";
const SCRIPT_PATH: &str = "scripts/turn.rs";
const SPRITE_PATH: &str = "textures/script-burst.png";
const SPRITE_BYTES: &[u8] = include_bytes!("fixtures/sky-panorama-4x2.png");
const PROP_PATH: &str = "props/orb.json";
const SCRIPT: &str = r#"// engine-script {"label":"Turn","write_owner_transform":true,"fields":[{"kind":"number","key":"speed","label":"Speed","default":1.0,"min":0.0,"max":8.0}],"references":[{"key":"anchor","label":"Anchor","requires":"entity"}],"particle_sprites":["<sprite>"]}
use engine_script_sdk::{Context, Script};

#[derive(Default)]
struct Turn {
    yaw: f32,
}

impl Script for Turn {
    fn on_start(&mut self, context: &mut Context<'_>) {
        self.yaw = context.reference_transform("anchor").unwrap().rotation[1];
        context.set_local_yaw(self.yaw);
    }

    fn on_fixed_update(&mut self, context: &mut Context<'_>) {
        if context.tick() == 1 {
            return;
        }
        let speed = context.number("speed").unwrap_or(1.0);
        self.yaw += speed * context.step_seconds();
        context.set_local_yaw(self.yaw);
    }

    fn read_state(&mut self, state: &[u8]) -> Result<(), String> {
        if !state.is_empty() {
            self.yaw = f32::from_le_bytes(state.try_into().map_err(|_| "Invalid Turn state")?);
        }
        Ok(())
    }

    fn write_state(&self) -> Vec<u8> {
        self.yaw.to_le_bytes().to_vec()
    }
}

engine_script_sdk::export_script!(Turn);
"#;
const IMPACT_SCRIPT: &str = r#"// engine-script {"label":"Impact","particle_sprites":["<sprite>"]}
use engine_script_sdk::{Context, Hit, Script};

#[derive(Default)]
struct Impact {
    hit: bool,
}

impl Script for Impact {
    fn on_hit(&mut self, context: &mut Context<'_>, hit: Hit<'_>) {
        if hit.target == "target" {
            self.hit = true;
            context.emit_particle_burst("<sprite>", hit.position);
        }
    }

    fn on_destroy(&mut self, _context: &mut Context<'_>) {
        assert!(self.hit);
    }

    fn read_state(&mut self, state: &[u8]) -> Result<(), String> {
        self.hit = state == [1];
        Ok(())
    }

    fn write_state(&self) -> Vec<u8> {
        vec![u8::from(self.hit)]
    }
}

engine_script_sdk::export_script!(Impact);
"#;

struct Fixture(PathBuf);

impl Fixture {
	fn new() -> Self {
		let root = Path::new(env!("CARGO_MANIFEST_DIR"))
			.join("../target")
			.join(format!(
				"qa-rust-script-package-{}",
				AssetId::generate().as_str()
			));
		for directory in [
			"assets/scenes",
			"assets/scripts",
			"assets/textures",
			"assets/props",
			"exports",
			"retry",
		] {
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
#[ignore = "compiles a project script using the rustc wasm32-unknown-unknown target"]
fn project_script_from_assets_runs_in_editor_and_staged_player() {
	let fixture = Fixture::new();
	let assets = fixture.0.join("assets");
	let script_id = AssetId::generate();
	let sprite_id = AssetId::generate();
	let script = SCRIPT.replace("<sprite>", sprite_id.as_str());
	fs::write(assets.join(SCRIPT_PATH), &script).unwrap();
	fs::write(assets.join(SPRITE_PATH), SPRITE_BYTES).unwrap();
	let catalog = AssetCatalog {
		assets: vec![
			asset(
				script_id.clone(),
				SCRIPT_PATH,
				AssetKind::RustScript,
				script.as_bytes(),
			),
			asset(
				sprite_id.clone(),
				SPRITE_PATH,
				AssetKind::Texture,
				SPRITE_BYTES,
			),
		],
		..Default::default()
	};
	fs::write(assets.join(CATALOG_FILE), catalog.to_json().unwrap()).unwrap();
	let scene = SceneData {
		schema_version: CURRENT_SCHEMA_VERSION,
		bindings: Default::default(),
		entities: vec![
			object("fast", &script_id, 4.),
			object("slow", &script_id, 2.),
			invisible_object("anchor").with_transform(TransformData {
				rotation: [0., 0.25, 0.],
				..Default::default()
			}),
		],
		camera: Default::default(),
		default_game_camera: None,
		environment: Default::default(),
		lights: vec![],
		organization: Default::default(),
	};
	let original_scene = scene.to_json().unwrap();
	fs::write(assets.join(SCENE_PATH), &original_scene).unwrap();
	let project = ProjectData::for_scene("Script Fixture", SCENE_PATH, &scene).unwrap();
	let dt = 1. / f64::from(project.fixed_hz);

	let mut editor = EditorSession::for_project(scene.clone(), project.clone()).unwrap();
	editor
		.set_module_resolver(project_resolver(&assets, ScriptSource::Rust))
		.unwrap();
	editor
		.execute_command(EditorCommandRequest {
			expected_revision: editor.revision(),
			command: EditorCommandDto::StartPlay,
		})
		.unwrap();
	let editor_trace: Vec<_> = (0..3)
		.map(|_| {
			editor.tick(dt).unwrap();
			yaw_pair(editor.render_world().unwrap())
		})
		.collect();
	assert_eq!(editor_trace[0], [0.25, 0.25]);
	for (yaw, rate) in editor_trace[2].into_iter().zip([4., 2.]) {
		assert!(
			(yaw - (0.25 + rate * (2. * dt) as f32)).abs() < 1e-6,
			"{editor_trace:?}"
		);
	}
	assert_eq!(*editor.authored_document(), scene);

	let edited_source = format!("{script}\n// Comment added by the project's code editor.\n");
	fs::write(assets.join(SCRIPT_PATH), &edited_source).unwrap();
	let prepared = compiler::prepare_project(&project, &assets).unwrap();
	let staged =
		compiler::stage_prepared_project(&prepared, fixture.0.join("exports").to_str().unwrap())
			.unwrap();
	let packaged_assets = staged.path().join("assets");
	assert_eq!(
		fs::read_to_string(packaged_assets.join(SCRIPT_PATH)).unwrap(),
		edited_source
	);
	assert_eq!(
		fs::read(packaged_assets.join(SPRITE_PATH)).unwrap(),
		SPRITE_BYTES
	);
	let packaged_catalog = bridge::script_assets::catalog(&packaged_assets).unwrap();
	let packaged_script = packaged_catalog.by_id(&script_id).unwrap();
	assert_eq!(
		packaged_script.fingerprint,
		format!("{:x}", Sha256::digest(&edited_source))
	);
	assert_eq!(packaged_script.dependencies, [sprite_id]);
	let module =
		fs::read(packaged_assets.join(bridge::script_assets::artifact_path(&script_id))).unwrap();
	assert!(module.starts_with(b"\0asm"));
	let fixture_player = fixture.0.join("fixture-player.exe");
	fs::write(&fixture_player, b"fixture Player executable").unwrap();
	let installed = compiler::complete_staged_project_with_player(staged, &fixture_player).unwrap();
	let installed_player = installed.verified_executable().unwrap();
	assert_eq!(
		fs::read(installed_player).unwrap(),
		b"fixture Player executable"
	);

	editor
		.execute_command(EditorCommandRequest {
			expected_revision: editor.revision(),
			command: EditorCommandDto::StopPlay,
		})
		.unwrap();
	fs::write(assets.join(SCRIPT_PATH), "fn broken(\n").unwrap();
	let rejected_play = editor
		.execute_command(EditorCommandRequest {
			expected_revision: editor.revision(),
			command: EditorCommandDto::StartPlay,
		})
		.err()
		.unwrap()
		.to_string();
	assert!(
		rejected_play.contains("scripts/turn.rs:1:"),
		"{rejected_play}"
	);
	assert!(editor.render_world().is_none());
	let rejected =
		compiler::stage_prepared_project(&prepared, fixture.0.join("retry").to_str().unwrap())
			.err()
			.unwrap();
	assert!(rejected.contains("turn.rs"), "{rejected}");
	assert!(!fixture.0.join("retry").join(&project.name).exists());
	let diagnostic = compiler::prepare_project(&project, &assets).err().unwrap();
	assert!(diagnostic.contains("scripts/turn.rs:1:"), "{diagnostic}");
	assert_eq!(
		fs::read_to_string(assets.join(SCENE_PATH)).unwrap(),
		original_scene
	);

	let packaged_project = ProjectData::load(installed.path().join("project.json")).unwrap();
	let packaged_scene =
		SceneData::from_json(&fs::read_to_string(packaged_assets.join(SCENE_PATH)).unwrap())
			.unwrap();
	let factory =
		project_resolver(&packaged_assets, ScriptSource::Packaged)(&packaged_project).unwrap();
	let scene_id = packaged_project.startup_scene_id.clone();
	let mut player = GameSession::with_factory_for_scene(
		Arc::new(packaged_project),
		scene_id,
		Arc::new(packaged_scene),
		factory,
	)
		.unwrap();
	let player_trace: Vec<_> = (0..3)
		.map(|_| {
			player.tick(dt).unwrap();
			yaw_pair(player.active().world())
		})
		.collect();
	assert_eq!(player_trace, editor_trace);
}

#[test]
#[ignore = "compiles a project script using the rustc wasm32-unknown-unknown target"]
fn staged_projectile_runs_its_rust_hit_hook_and_keeps_the_impact_sprite() {
	let fixture = Fixture::new();
	let root = fixture.0.join("assets");
	let script_id = AssetId::generate();
	let sprite_id = AssetId::generate();
	let script = IMPACT_SCRIPT.replace("<sprite>", sprite_id.as_str());
	let mut launcher = invisible_object("launcher");
	launcher.native_behaviors.push(behavior(
		"launch",
		NativeBehaviorSpec::ProjectileLauncher {
			settings: ProjectileLauncherSettings {
				prefab_path: PROP_PATH.into(),
				action: "launch".into(),
				max_alive: 2,
				spawn_offset: [0.; 3],
			},
		},
	));
	let target = invisible_object("target")
		.with_transform(TransformData {
			position: [0., 0., -0.35],
			..Default::default()
		})
		.with_collider(Some(ColliderData::Box {
			half_extents: [0.1, 0.1, 0.05],
			offset: [0.; 3],
			interaction: Default::default(),
		}));
	let scene = SceneData {
		schema_version: CURRENT_SCHEMA_VERSION,
		bindings: Default::default(),
		entities: vec![launcher, target],
		camera: Default::default(),
		default_game_camera: None,
		environment: Default::default(),
		lights: vec![],
		organization: Default::default(),
	};
	let mut orb = invisible_object("orb");
	orb.scripts.push(attachment(&script_id));
	orb.native_behaviors.push(behavior(
		"motion",
		NativeBehaviorSpec::Projectile {
			settings: ProjectileSettings {
				speed: 24.,
				lifetime_ticks: 8,
				collision_mask: 1,
				impact: Some(ParticleBurstSettings {
					count: 3,
					..Default::default()
				}),
			},
		},
	));
	let prefab = PrefabData {
		schema_version: 3,
		composition: Default::default(),
		entities: vec![orb],
		gameplay: None,
		reference_slots: Default::default(),
	};
	let prop_source = prefab.to_json().unwrap();
	let catalog = AssetCatalog {
		assets: vec![
			asset(
				script_id,
				SCRIPT_PATH,
				AssetKind::RustScript,
				script.as_bytes(),
			),
			asset(
				sprite_id.clone(),
				SPRITE_PATH,
				AssetKind::Texture,
				SPRITE_BYTES,
			),
			asset(
				AssetId::generate(),
				PROP_PATH,
				AssetKind::Prefab,
				prop_source.as_bytes(),
			),
		],
		..Default::default()
	};
	fs::write(root.join(CATALOG_FILE), catalog.to_json().unwrap()).unwrap();
	fs::write(root.join(SCENE_PATH), scene.to_json().unwrap()).unwrap();
	fs::write(root.join(SCRIPT_PATH), &script).unwrap();
	fs::write(root.join(SPRITE_PATH), SPRITE_BYTES).unwrap();
	fs::write(root.join(PROP_PATH), &prop_source).unwrap();
	let project = ProjectData::for_scene("Scripted Impact Fixture", SCENE_PATH, &scene).unwrap();
	let prepared = compiler::prepare_project(&project, &root).unwrap();
	let staged =
		compiler::stage_prepared_project(&prepared, fixture.0.join("exports").to_str().unwrap())
			.unwrap();
	let assets = staged.path().join("assets");
	let packaged = ProjectData::load(staged.path().join("project.json")).unwrap();
	let launch_scene = packaged.startup_scene_id.clone();
	let loaded =
		SceneData::from_json(&fs::read_to_string(assets.join(SCENE_PATH)).unwrap()).unwrap();
	let resolver = project_resolver(&assets, ScriptSource::Packaged);
	let factory = resolver(&packaged).unwrap();
	let mut player = GameSession::with_factory_for_scene(
		Arc::new(packaged),
		launch_scene,
		Arc::new(loaded),
		factory,
	)
		.unwrap();
	let prop_bytes = fs::read(assets.join(PROP_PATH)).unwrap();
	let prop = PrefabData::from_json(std::str::from_utf8(&prop_bytes).unwrap()).unwrap();
	let spawn = player
		.active()
		.prepare_runtime_spawn(
			&prop,
			&format!("{:x}", Sha256::digest(&prop_bytes)),
			[0.; 3],
			None,
		)
		.unwrap();
	let projectile = spawn.entities()[0].key.clone();
	player.active_mut().commit_runtime_spawn(spawn).unwrap();
	player.tick(1. / f64::from(project.fixed_hz)).unwrap();
	assert!(player.active().runtime_handle(&projectile).is_none());
	let particles: Vec<_> = player.active().particles().collect();
	assert_eq!(particles.len(), 12);
	assert!(
		particles
			.iter()
			.all(|sample| sample.sprite_asset_id.as_deref() == Some(&sprite_id))
	);
}

fn object(key: &str, id: &AssetId, speed: f32) -> EntityData {
	let mut entity = invisible_object(key);
	let mut script = attachment(id);
	script
		.settings
		.properties
		.insert("speed".into(), ScriptProperty::Number(speed));
	script.settings.references.insert(
		"anchor".into(),
		ScriptReference::Entity {
			entity: EntityKey::new("anchor").unwrap(),
		},
	);
	entity.scripts.push(script);
	entity
}

fn invisible_object(key: &str) -> EntityData {
	EntityData::new(EntityKey::new(key).unwrap(), key, NO_MESH_PATH).with_renderable(false)
}

fn attachment(id: &AssetId) -> ScriptComponent {
	ScriptComponent {
		id: ComponentId::new("script").unwrap(),
		enabled: true,
		settings: ScriptSettings {
			script_id: project_script_id(id),
			config_version: 1,
			properties: Default::default(),
			references: Default::default(),
		},
	}
}

fn behavior(id: &str, behavior: NativeBehaviorSpec) -> NativeBehaviorComponent {
	NativeBehaviorComponent {
		id: ComponentId::new(id).unwrap(),
		enabled: true,
		behavior,
	}
}

fn asset(id: AssetId, path: &str, kind: AssetKind, bytes: &[u8]) -> PersistentAsset {
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

fn yaw_pair(world: &RuntimeWorld) -> [f32; 2] {
	["fast", "slow"].map(|key| {
		let index = world.entity_index(&EntityKey::new(key).unwrap()).unwrap();
		world.data().entities[index].transform.rotation[1]
	})
}
