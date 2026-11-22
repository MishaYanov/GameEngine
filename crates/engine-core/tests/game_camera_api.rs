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
fn camera_authoring_creates_aligns_and_edits_one_undoable_object_at_a_time() {
	use engine_core::{
		command::{TransformDto, TransformEditDto, TransformSpace},
		components::ComponentData,
	};
	let mut parent = entity("parent", [4.0, 0.0, 2.0]);
	parent.transform.rotation[1] = 0.45;
	let data = scene(vec![parent]);
	let project = ProjectData::for_scene("Camera", "scenes/main.json", &data).unwrap();
	let mut engine = EngineBuilder::new(data, project).build().unwrap();
	let run = |engine: &mut engine_core::Engine, command| {
		engine
			.execute(EditorCommandRequest {
				expected_revision: engine.session().revision(),
				command,
			})
			.unwrap();
	};
	run(&mut engine, Command::CreateCamera);
	let default = engine
		.session()
		.authored_document()
		.default_game_camera
		.clone()
		.unwrap();
	assert_eq!(engine.session().selection(), std::slice::from_ref(&default));
	run(&mut engine, Command::CreateCamera);
	let second = engine.session().selection()[0].clone();
	assert_ne!(second, default);
	assert_eq!(
		engine.session().authored_document().default_game_camera,
		Some(default.clone())
	);
	assert_eq!(engine.session().authored_document().entities.len(), 3);
	run(&mut engine, Command::Undo);
	assert_eq!(engine.session().authored_document().entities.len(), 2);
	assert_eq!(engine.session().selection(), std::slice::from_ref(&default));
	run(&mut engine, Command::Redo);
	assert_eq!(engine.session().selection(), std::slice::from_ref(&second));
	run(
		&mut engine,
		Command::ReparentObjects {
			keys: vec![second.clone()],
			parent: Some(key("parent")),
			keep_world: true,
		},
	);
	let command = Command::AlignCameraToSceneView {
		key: second.clone(),
		position: [9.0, 3.0, -4.0],
		yaw: 0.28,
		pitch: -0.25,
	};
	let before_align = engine.session().authored_document();
	let revision = engine.session().revision();
	assert!(
		engine
			.execute(EditorCommandRequest {
				expected_revision: revision - 1,
				command: command.clone()
			})
			.is_err()
	);
	assert_eq!(engine.session().authored_document(), before_align);
	run(&mut engine, command);
	let aligned = engine.session().authored_document();
	let pose = aligned
		.object_camera(&second, &aligned.world_matrices().unwrap())
		.unwrap();
	for (value, expected) in pose.position.into_iter().zip([9.0, 3.0, -4.0]) {
		close(value, expected);
	}
	close(pose.yaw, 0.28);
	close(pose.pitch, -0.25);
	assert_eq!(aligned.camera, before_align.camera);
	assert_eq!(
		aligned.entities.last().unwrap().camera,
		before_align.entities.last().unwrap().camera
	);
	run(&mut engine, Command::Undo);
	assert_eq!(engine.session().authored_document(), before_align);
	run(&mut engine, Command::Redo);
	assert_eq!(engine.session().authored_document(), aligned);

	let mut component = aligned
		.entities
		.last()
		.unwrap()
		.component_records()
		.remove(0);
	let ComponentData::Camera { settings } = &mut component.data else {
		panic!("camera record expected")
	};
	settings.lens.vertical_fov = 0.65;
	run(
		&mut engine,
		Command::ReplaceComponent {
			key: second.as_str().into(),
			component,
		},
	);
	let previous = engine.session().authored_document();
	assert_eq!(
		previous
			.entities
			.last()
			.unwrap()
			.camera
			.as_ref()
			.unwrap()
			.settings
			.lens
			.vertical_fov,
		0.65
	);
	assert_eq!(
		previous.entities.last().unwrap().transform,
		aligned.entities.last().unwrap().transform
	);
	run(&mut engine, Command::Undo);
	assert_eq!(engine.session().authored_document(), aligned);
	run(&mut engine, Command::Redo);
	assert_eq!(engine.session().authored_document(), previous);
	engine.mark_scene_saved().unwrap();
	run(
		&mut engine,
		Command::BeginTransaction {
			label: "Camera transform".into(),
		},
	);
	let old = &previous.entities.last().unwrap().transform;
	run(
		&mut engine,
		Command::SetTransforms {
			edits: vec![TransformEditDto {
				key: second.as_str().into(),
				transform: TransformDto {
					position: [0.0; 3],
					rotation: old.rotation,
					scale: old.scale,
				},
			}],
			space: TransformSpace::Local,
			snap: false,
		},
	);
	assert_ne!(engine.session().authored_document(), previous);
	run(&mut engine, Command::CancelTransaction);
	assert_eq!(engine.session().authored_document(), previous);
	assert!(!engine.session().is_dirty());
}

#[test]
fn migrated_camera_drives_play_and_legacy_edits_read_and_write_the_object() {
	let legacy = followed();
	let mut wire: serde_json::Value = serde_json::from_str(&legacy.to_json().unwrap()).unwrap();
	wire["schema_version"] = 5.into();
	let migrated = engine_core::SceneData::from_json(&wire.to_string()).unwrap();
	let default_key = migrated.default_game_camera.clone().unwrap();
	let mut standalone = migrated.clone();
	standalone.camera.follow = None;
	standalone.camera.position = [100.0; 3];
	let world = RuntimeWorld::new(Arc::new(standalone)).unwrap();
	assert_eq!(
		GameCameraController::new(&world).unwrap().camera().position,
		[3.0, 2.0, 5.0]
	);
	let project = ProjectData::for_scene("Camera", "scenes/main.json", &migrated).unwrap();
	let mut engine = EngineBuilder::new(migrated.clone(), project)
		.build()
		.unwrap();
	let run = |engine: &mut engine_core::Engine, command| {
		engine
			.execute(EditorCommandRequest {
				expected_revision: engine.session().revision(),
				command,
			})
			.unwrap();
	};
	let mut camera = migrated.camera.clone();
	camera.position = [4.0, 1.0, 8.0];
	camera.vertical_fov = 0.9;
	run(&mut engine, Command::SetCamera { camera });
	let edited = engine.session().authored_document();
	let object = edited.entities.last().unwrap();
	assert_eq!(object.key, default_key);
	assert_eq!(object.transform.position, [4.0, 1.0, 8.0]);
	assert_eq!(
		object.camera.as_ref().unwrap().settings.lens.vertical_fov,
		0.9
	);
	run(
		&mut engine,
		Command::Select {
			keys: vec![default_key.as_str().into()],
		},
	);
	run(&mut engine, Command::DeleteSelection);
	assert!(
		engine
			.session()
			.authored_document()
			.default_game_camera
			.is_none()
	);
	run(&mut engine, Command::Undo);
	assert_eq!(
		engine.session().authored_document().as_ref(),
		edited.as_ref()
	);
	run(&mut engine, Command::Undo);
	assert_eq!(engine.session().authored_document().as_ref(), &migrated);
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
