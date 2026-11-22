use std::{
	collections::{BTreeMap, HashSet},
	sync::Arc,
};

use engine_core::{
	Engine, EngineBuilder, EntityKey, SceneData,
	camera_input::CameraIntent,
	command::{EditorCommandDto, EditorCommandRequest},
	game_camera::{FollowCameraSettings, OrbitCameraSettings},
	input::{GameInputBinding, InputProfile, Key},
	project::{BehaviorSpace, BehaviorSpec, ProjectData, SceneBehavior},
	scene_catalog::{SceneDescriptor, SceneId},
	scene_data::{CURRENT_SCHEMA_VERSION, CameraData, ColliderData, EntityData},
};
use glam::Vec3;

use super::{CINDER_OATH_ID, CinderProgress, Phase, resolve};

const STEP: f64 = 1.0 / 60.0;

fn entity_key(name: &str) -> EntityKey {
	EntityKey::new(name).unwrap()
}

fn camera() -> CameraData {
	CameraData {
		position: [0.0, 1.6, 0.25],
		yaw: 0.0,
		pitch: 0.0,
		vertical_fov: 66.0_f32.to_radians(),
		near_plane: 0.02,
		far_plane: 40.0,
		follow: None,
		orbit: None,
	}
}

fn box_entity(key: &str, position: [f32; 3], half_extents: [f32; 3]) -> EntityData {
	let mut entity = EntityData::new(entity_key(key), key, "props/test.glb").with_collider(Some(
		ColliderData::Box {
			half_extents,
			offset: [0.0, 0.85, 0.0],
			interaction: Default::default(),
		},
	));
	entity.transform.position = position;
	entity
}

fn arena(enemy_z: f32, wall: bool) -> SceneData {
	let mut camera = camera();
	camera.orbit = Some(OrbitCameraSettings {
		target: entity_key("player"),
		pivot_offset: [0.0, 1.6, 0.0],
		initial_yaw: 0.0,
		initial_pitch: 0.0,
		initial_distance: 0.25,
		min_distance: 0.25,
		max_distance: 0.25,
		min_pitch: -1.3,
		max_pitch: 1.3,
		follow_seconds: 0.0,
		recovery_seconds: 0.0,
		teleport_distance: 10.0,
		probe_radius: 0.06,
		obstruction_mask: 1,
	});
	let mut entities = vec![
		box_entity("player", [0.0; 3], [0.33, 0.85, 0.33]).with_renderable(false),
		box_entity("warden", [0.0, 0.0, enemy_z], [0.43, 0.9, 0.43]),
		EntityData::new(entity_key("sword"), "Sword", "props/test.glb"),
	];
	if wall {
		entities.push(box_entity("wall", [0.0, 0.0, -1.15], [2.2, 1.1, 0.14]));
	}
	SceneData {
		schema_version: CURRENT_SCHEMA_VERSION,
		organization: Default::default(),
		default_game_camera: None,
		bindings: BTreeMap::from([
			("player".into(), entity_key("player")),
			("enemy".into(), entity_key("warden")),
			("sword".into(), entity_key("sword")),
		]),
		entities,
		camera,
		lights: Vec::new(),
	}
}

fn intro() -> SceneData {
	let mut camera = camera();
	camera.follow = Some(FollowCameraSettings {
		target: entity_key("track"),
		offset: [0.0, 1.55, 5.1],
		look_offset: [0.0, 1.55, 4.1],
		smoothing_seconds: 0.0,
	});
	SceneData {
		schema_version: CURRENT_SCHEMA_VERSION,
		organization: Default::default(),
		default_game_camera: None,
		bindings: BTreeMap::from([
			("camera_track".into(), entity_key("track")),
			("knight".into(), entity_key("knight")),
		]),
		entities: vec![
			EntityData::new(entity_key("track"), "Camera Track", "props/test.glb")
				.with_renderable(false),
			EntityData::new(entity_key("knight"), "Knight", "props/test.glb"),
		],
		camera,
		lights: Vec::new(),
	}
}

fn session(scene: SceneData, startup: &str) -> Engine {
	let startup_id = SceneId::new(startup).unwrap();
	let mut project = ProjectData::for_scene_with_id(
		"Cinder tests",
		startup_id,
		format!("scenes/{startup}.json"),
		&scene,
	)
	.unwrap();
	let other = if startup == "intro" { "arena" } else { "intro" };
	project
		.scenes
		.insert(
			SceneDescriptor::new(SceneId::new(other).unwrap(), format!("scenes/{other}.json"))
				.unwrap(),
		)
		.unwrap();
	project.behaviors.push(SceneBehavior {
		scene_id: SceneId::new("arena").unwrap(),
		behavior: BehaviorSpec::ControlledMove {
			entity: entity_key("player"),
			speed: 4.2,
			space: BehaviorSpace::World,
		},
	});
	project.gameplay_module = Some(CINDER_OATH_ID.into());
	project.gameplay_settings = Some(serde_json::json!({
		"schema_version": 1,
		"intro_scene": "intro",
		"arena_scene": "arena",
		"intro_camera_travel": [0.0, 0.0, -3.2]
	}));
	project.input_profile = Some(InputProfile {
		actions: vec![
			GameInputBinding {
				id: "attack".into(),
				key: Key::F,
				animation_parameter: None,
			},
			GameInputBinding {
				id: "start".into(),
				key: Key::Enter,
				animation_parameter: None,
			},
		],
		..InputProfile::default()
	});
	let mut engine = EngineBuilder::new(scene, project)
		.module_resolver(Arc::new(resolve))
		.build()
		.unwrap();
	engine
		.execute(EditorCommandRequest {
			expected_revision: engine.session().revision(),
			command: EditorCommandDto::StartPlay,
		})
		.unwrap();
	engine
}

fn keys(engine: &mut Engine, keys: &[&str]) {
	let held: HashSet<String> = keys.iter().map(|key| (*key).into()).collect();
	engine.set_game_keyboard_input_from_codes(&held).unwrap();
}

fn advance(engine: &mut Engine, ticks: usize) {
	for _ in 0..ticks {
		engine.tick(STEP).unwrap();
	}
}

fn progress(engine: &Engine) -> CinderProgress {
	*engine.session().play_progress::<CinderProgress>().unwrap()
}

fn position(engine: &Engine, key: &str) -> Vec3 {
	let document = engine.session().render_document();
	let key = entity_key(key);
	Vec3::from_array(
		document
			.entities
			.iter()
			.find(|entity| entity.key == key)
			.unwrap()
			.transform
			.position,
	)
}

#[test]
fn intro_moves_the_camera_and_opens_the_arena_on_time_or_enter() {
	let mut timed = session(intro(), "intro");
	let initial_z = timed.session().render_camera().position[2];
	advance(&mut timed, 100);
	assert!(timed.session().render_camera().position[2] < initial_z - 0.75);
	assert_eq!(progress(&timed).phase(), Phase::Intro);
	advance(&mut timed, 121);
	assert_eq!(
		timed.tick(STEP).unwrap().requested_scene.unwrap().as_str(),
		"arena"
	);

	let mut skipped = session(intro(), "intro");
	keys(&mut skipped, &["Enter"]);
	assert_eq!(
		skipped
			.tick(STEP)
			.unwrap()
			.requested_scene
			.unwrap()
			.as_str(),
		"arena"
	);
}

#[test]
fn three_committed_hits_win_and_holding_attack_does_not_repeat() {
	let mut engine = session(arena(-2.1, false), "arena");
	for expected_hits in 1..=3 {
		keys(&mut engine, &["KeyF"]);
		advance(&mut engine, 19);
		assert_eq!(progress(&engine).enemy_hits(), expected_hits);
		advance(&mut engine, 32);
		assert_eq!(progress(&engine).swings(), expected_hits);
		keys(&mut engine, &[]);
		advance(&mut engine, 1);
	}
	let gameplay = engine.play_status().unwrap().gameplay.unwrap();
	assert_eq!(progress(&engine).phase(), Phase::Victory);
	assert_eq!(gameplay.objective, "VICTORY");
	assert!(gameplay.completed);
	let document = engine.session().render_document();
	assert!(
		!document
			.entities
			.iter()
			.find(|entity| entity.key.as_str() == "warden")
			.unwrap()
			.collision_enabled
	);
}

#[test]
fn rear_targets_and_a_solid_wall_prevent_sword_damage() {
	for (enemy_z, wall) in [(2.1, false), (-2.1, true), (-8.0, false)] {
		let mut engine = session(arena(enemy_z, wall), "arena");
		keys(&mut engine, &["KeyF"]);
		advance(&mut engine, 22);
		assert_eq!(progress(&engine).swings(), 1);
		assert_eq!(progress(&engine).enemy_hits(), 0);
	}
	let mut above = session(arena(-2.1, false), "arena");
	let owner = above.camera_input_owner().unwrap();
	above
		.submit_camera_input(
			&owner.run_id,
			owner.scene_generation,
			CameraIntent {
				pitch_delta: 1.2,
				..Default::default()
			},
		)
		.unwrap();
	advance(&mut above, 1);
	keys(&mut above, &["KeyF"]);
	advance(&mut above, 22);
	assert_eq!(progress(&above).swings(), 1);
	assert_eq!(progress(&above).enemy_hits(), 0);
}

#[test]
fn forward_input_tracks_mouse_yaw_and_the_sword_uses_the_committed_eye_pose() {
	let mut engine = session(arena(-5.0, false), "arena");
	let owner = engine.camera_input_owner().unwrap();
	engine
		.submit_camera_input(
			&owner.run_id,
			owner.scene_generation,
			CameraIntent {
				yaw_delta: std::f64::consts::FRAC_PI_2,
				pitch_delta: 0.32,
				..Default::default()
			},
		)
		.unwrap();
	advance(&mut engine, 1);
	keys(&mut engine, &["KeyW"]);
	advance(&mut engine, 5);
	assert!(position(&engine, "player").x > 0.3);
	assert!(position(&engine, "player").z.abs() < 0.01);
	let eye = engine.session().render_camera();
	assert_eq!(position(&engine, "sword"), Vec3::from_array(eye.position));
	let document = engine.session().render_document();
	let sword = document
		.entities
		.iter()
		.find(|entity| entity.key.as_str() == "sword")
		.unwrap();
	let sight = sword.transform.matrix().transform_vector3(-Vec3::Z);
	assert!(sight.abs_diff_eq(Vec3::from_array(eye.forward()), 1e-5));
}

#[test]
fn three_warden_strikes_defeat_a_stationary_player() {
	let mut engine = session(arena(-1.65, false), "arena");
	advance(&mut engine, 200);
	assert_eq!(progress(&engine).player_hits(), 3);
	assert_eq!(progress(&engine).phase(), Phase::Defeat);
	assert!(!engine.play_status().unwrap().gameplay.unwrap().completed);
	advance(&mut engine, 30);
	assert_eq!(progress(&engine).player_hits(), 3);
}
