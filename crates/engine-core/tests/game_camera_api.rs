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

#[test]
fn selecting_a_game_camera_changes_saved_and_play_pose_with_one_undo() {
	let mut data = two_cameras();
	let left = key("left-camera");
	let right = key("right-camera");
	data.default_game_camera = Some(left.clone());
	let project = ProjectData::for_scene("Cameras", "scenes/main.json", &data).unwrap();
	let mut engine = EngineBuilder::new(data, project).build().unwrap();
	let execute = |engine: &mut engine_core::Engine, command| {
		engine.execute(EditorCommandRequest {
			expected_revision: engine.session().revision(),
			command,
		})
	};
	assert!(
		execute(
			&mut engine,
			Command::SetDefaultGameCamera {
				key: key("missing")
			}
		)
		.is_err()
	);
	execute(
		&mut engine,
		Command::SetDefaultGameCamera { key: right.clone() },
	)
	.unwrap();
	let authored = engine.session().authored_document();
	assert_eq!(authored.default_game_camera.as_ref(), Some(&right));
	let reopened = engine_core::SceneData::from_json(&authored.to_json().unwrap()).unwrap();
	assert_eq!(reopened.default_game_camera.as_ref(), Some(&right));
	execute(&mut engine, Command::StartPlay).unwrap();
	assert_eq!(engine.session().render_camera().position, [6.0, 0.0, 0.0]);
	execute(&mut engine, Command::StopPlay).unwrap();
	let attachment = authored.entities[1].camera.as_ref().unwrap();
	execute(
		&mut engine,
		Command::SetComponentEnabled {
			key: right.as_str().into(),
			component_id: attachment.id.clone(),
			enabled: false,
		},
	)
	.unwrap();
	assert!(
		engine
			.session()
			.authored_document()
			.default_game_camera
			.is_none()
	);
	execute(&mut engine, Command::Undo).unwrap();
	assert_eq!(
		engine
			.session()
			.authored_document()
			.default_game_camera
			.as_ref(),
		Some(&right)
	);
	execute(&mut engine, Command::Undo).unwrap();
	assert_eq!(
		engine
			.session()
			.authored_document()
			.default_game_camera
			.as_ref(),
		Some(&left)
	);
}

#[test]
fn live_camera_switch_checks_run_scene_and_runtime_identity() {
	use engine_core::{game_session::GameSession, simulation_session::ConfiguredBehaviorFactory};
	let mut data = two_cameras();
	data.default_game_camera = Some(key("left-camera"));
	let project = Arc::new(ProjectData::for_scene("Cameras", "scenes/main.json", &data).unwrap());
	let data = Arc::new(data);
	let open = || {
		GameSession::with_factory_for_scene(
			Arc::clone(&project),
			project.startup_scene_id.clone(),
			Arc::clone(&data),
			Arc::new(ConfiguredBehaviorFactory),
		)
		.unwrap()
	};
	let mut active = open();
	let handle = |session: &GameSession| {
		session
			.active()
			.runtime_inspection(session.play_session_id(), session.run_id(), 0, 8)
			.unwrap()
			.entities
			.into_iter()
			.find(|item| item.handle.key.as_str() == "right-camera")
			.unwrap()
			.handle
	};
	let owner = active.run_id().to_owned();
	let scope = active.scene_generation();
	let replacement = handle(&active);
	assert!(
		active
			.set_runtime_camera(&owner, scope, Some(handle(&open())))
			.is_err()
	);
	assert!(
		active
			.set_runtime_camera("previous", scope, Some(replacement.clone()))
			.is_err()
	);
	assert!(
		active
			.set_runtime_camera(&owner, scope + 1, Some(replacement.clone()))
			.is_err()
	);
	assert_eq!(active.active().camera().position, [-4.0, 0.0, 0.0]);
	active
		.set_runtime_camera(&owner, scope, Some(replacement.clone()))
		.unwrap();
	assert_eq!(active.active().active_camera_object(), Some(&replacement));
	assert_eq!(active.active().camera().position, [6.0, 0.0, 0.0]);
	assert_eq!(active.active().camera().vertical_fov, 0.8);
	active.tick(1.0 / 60.0).unwrap();
	assert_eq!(
		active.active().document().default_game_camera.as_ref(),
		Some(&key("left-camera"))
	);
	active.set_runtime_camera(&owner, scope, None).unwrap();
	assert_eq!(active.active().camera().position, [-4.0, 0.0, 0.0]);
}

fn two_cameras() -> engine_core::SceneData {
	use engine_core::{
		components::{ComponentId, NO_MESH_PATH},
		game_camera::{CameraComponent, CameraSettings},
	};
	let default = scene(vec![]);
	let camera = |name: &str, x, fov| {
		let mut object = entity(name, [x, 0.0, 0.0]).with_renderable(false);
		object.model_path = NO_MESH_PATH.into();
		let mut settings = CameraSettings::from_legacy(&default.camera);
		settings.lens.vertical_fov = fov;
		object.camera = Some(Box::new(CameraComponent {
			id: ComponentId::generate(),
			enabled: true,
			settings,
		}));
		object
	};
	scene(vec![
		camera("left-camera", -4.0, 1.0),
		camera("right-camera", 6.0, 0.8),
	])
}

#[test]
fn losing_runtime_camera_pose_reverts_to_default_then_retains_the_last_view() {
	use engine_core::{
		game_session::GameSession,
		scene_catalog::SceneId,
		simulation_session::{
			ActionWriter, GameModule, GameModuleFactory, InputFrame, SimulationAction,
		},
	};
	struct ChangeCameraPose;
	impl GameModuleFactory for ChangeCameraPose {
		fn create(&self, _: &ProjectData, _: &SceneId) -> Result<Box<dyn GameModule>, String> {
			Ok(Box::new(Self))
		}
	}
	impl GameModule for ChangeCameraPose {
		fn needs_tick(&self, _: &InputFrame) -> bool {
			true
		}
		fn plan(
			&self,
			world: &RuntimeWorld,
			_: &InputFrame,
			_: f32,
			out: &mut ActionWriter<'_>,
		) -> Result<(), String> {
			for name in ["right-camera", "left-camera"] {
				let key = key(name);
				let object = &world.data().entities[world.entity_index(&key).unwrap()];
				if object.transform.rotation[2] == 0.0 {
					let mut transform = object.transform.clone();
					transform.rotation[2] = 0.4;
					return out.push(SimulationAction::SetTransform {
						entity: key,
						transform,
					});
				}
			}
			Ok(())
		}
	}

	let mut scene = two_cameras();
	scene.default_game_camera = Some(key("left-camera"));
	let project = ProjectData::for_scene("Cameras", "scenes/main.json", &scene).unwrap();
	let startup = project.startup_scene_id.clone();
	let mut session = GameSession::with_factory_for_scene(
		Arc::new(project),
		startup,
		Arc::new(scene),
		Arc::new(ChangeCameraPose),
	)
	.unwrap();
	let selected = session
		.active()
		.runtime_inspection(session.play_session_id(), session.run_id(), 0, 8)
		.unwrap()
		.entities[1]
		.handle
		.clone();
	let run = session.run_id().to_owned();
	session
		.set_runtime_camera(&run, session.scene_generation(), Some(selected))
		.unwrap();
	assert_eq!(session.active().camera().position, [6.0, 0.0, 0.0]);
	assert!(session.tick(1.0 / 60.0).unwrap().changes.camera_changed);
	assert_eq!(
		session
			.active()
			.active_camera_object()
			.unwrap()
			.key
			.as_str(),
		"left-camera"
	);
	let last_valid = session.active().camera().clone();
	assert!(session.tick(1.0 / 60.0).unwrap().camera_status_changed);
	assert!(session.active().active_camera_object().is_none());
	assert!(session.active().camera_object_diagnostic().is_some());
	assert_eq!(session.active().camera().position, last_valid.position);
	assert_eq!(
		session.active().camera().vertical_fov,
		last_valid.vertical_fov
	);
}
