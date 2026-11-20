mod support;
use engine_core::{
	EngineBuilder,
	command::{EditorCommandDto as Command, EditorCommandRequest},
	game_camera::{FollowCameraSettings, GameCameraController},
	project::{BehaviorSpec, ProjectData},
	scene_data::TransformData,
	world::RuntimeWorld,
};
use std::sync::Arc;
use support::{close, entity, key, scene};

fn followed() -> engine_core::SceneData {
	let mut data = scene(vec![
		entity("parent", [2.0, 0.0, 0.0]),
		entity("actor", [1.0, 0.0, 0.0]).with_parent(Some(key("parent"))),
	]);
	data.camera.follow = Some(FollowCameraSettings {
		target: key("actor"),
		offset: [0.0, 2.0, 5.0],
		look_offset: [0.0; 3],
		smoothing_seconds: 0.2,
	});
	data
}

#[test]
fn authored_follow_settings_roundtrip_and_reject_missing_or_degenerate_targets() {
	let original = followed();
	assert_eq!(
		engine_core::SceneData::from_json(&original.to_json().unwrap()).unwrap(),
		original
	);
	let mut invalid = original.clone();
	invalid.camera.follow.as_mut().unwrap().target = key("absent");
	assert!(invalid.validate().unwrap_err().contains("target"));
	for (offset, look, smoothing) in [
		([0.0; 3], [0.0; 3], 0.1),
		([0.0, 1.0, 0.0], [0.0; 3], 0.1),
		([0.0, 0.0, 5.0], [0.0; 3], -1.0),
		([0.0, 0.0, 5.0], [0.0; 3], 10.1),
	] {
		let mut invalid = original.clone();
		let follow = invalid.camera.follow.as_mut().unwrap();
		follow.offset = offset;
		follow.look_offset = look;
		follow.smoothing_seconds = smoothing;
		assert!(invalid.validate().is_err());
	}
}

#[test]
fn follow_uses_world_space_snaps_on_reset_and_smoothing_settles_without_permanent_ticks() {
	let mut world = RuntimeWorld::new(Arc::new(followed())).unwrap();
	let mut camera = GameCameraController::new(&world).unwrap();
	assert_eq!(camera.camera().position, [3.0, 2.0, 5.0]);
	assert!(!camera.needs_tick(&world));
	world
		.set_transform(
			&key("parent"),
			TransformData {
				position: [12.0, 0.0, 0.0],
				..Default::default()
			},
		)
		.unwrap();
	assert!(camera.needs_tick(&world));
	let mut partitioned = camera.clone();
	camera.update(&world, 0.2).unwrap();
	partitioned.update(&world, 0.1).unwrap();
	partitioned.update(&world, 0.1).unwrap();
	close(
		camera.camera().position[0],
		partitioned.camera().position[0],
	);
	assert!(camera.camera().position[0] > 3.0 && camera.camera().position[0] < 13.0);
	world
		.set_transform(
			&key("parent"),
			TransformData {
				position: [-20.0, 0.0, 0.0],
				..Default::default()
			},
		)
		.unwrap();
	for _ in 0..300 {
		camera.update(&world, 1.0 / 60.0).unwrap();
	}
	assert!(!camera.needs_tick(&world));
	assert_eq!(camera.camera().position, [-19.0, 2.0, 5.0]);
	camera.camera().view_projection(1.0).unwrap();
	assert_eq!(
		GameCameraController::new(&world).unwrap().camera().position,
		[-19.0, 2.0, 5.0]
	);
}

#[test]
fn camera_runtime_motion_respects_pause_restart_and_authored_delete_undo() {
	let document = followed();
	let mut project = ProjectData::for_scene("Follow", "scenes/main.json", &document).unwrap();
	project.add_behavior(BehaviorSpec::ControlledMove {
		entity: key("actor"),
		speed: 2.0,
		space: Default::default(),
	});
	let mut engine = EngineBuilder::new(document.clone(), project)
		.build()
		.unwrap();
	let command = |engine: &mut engine_core::Engine, command| {
		engine
			.execute(EditorCommandRequest {
				expected_revision: engine.session().revision(),
				command,
			})
			.unwrap()
	};
	command(&mut engine, Command::StartPlay);
	let original = engine.session().render_camera();
	command(
		&mut engine,
		Command::SetPlayInput {
			movement: [1.0, 0.0, 0.0],
		},
	);
	let outcome = engine.tick(1.0 / 60.0).unwrap();
	assert!(outcome.changes.camera_changed);
	assert!(engine.session().render_camera().position[0] > original.position[0]);
	command(&mut engine, Command::SetPaused { paused: true });
	let paused = engine.session().render_camera();
	engine.tick(0.2).unwrap();
	assert_eq!(engine.session().render_camera().position, paused.position);
	let status = engine.play_status().unwrap();
	let (restart, _) = engine
		.prepare_restart_play(&status.run_id, engine.session().revision())
		.unwrap();
	engine.commit_play_scene(restart).unwrap();
	assert_eq!(engine.session().render_camera().position, original.position);
	command(&mut engine, Command::StopPlay);
	assert_eq!(engine.session().authored_document().as_ref(), &document);
	command(
		&mut engine,
		Command::Select {
			keys: vec!["actor".into()],
		},
	);
	command(&mut engine, Command::DeleteSelection);
	assert!(engine.session().authored_document().camera.follow.is_none());
	command(&mut engine, Command::Undo);
	assert_eq!(
		engine.session().authored_document().camera.follow,
		document.camera.follow
	);
}
