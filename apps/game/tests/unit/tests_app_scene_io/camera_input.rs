use super::*;

#[test]
fn player_uses_authored_keys_without_running_conflicting_camera_or_pause_shortcuts() {
	let (mut app, [actor, _]) = fixture();
	let mut project = app.project.clone().unwrap();
	project.input_profile = Some(InputProfile {
		movement: MovementBindings::ARROWS,
		actions: vec![runtime::input::GameInputBinding {
			id: "interact".into(),
			key: Key::Space,
			animation_parameter: Some("IsInteracting".into()),
		}],
		..InputProfile::default()
	});
	app.editor.set_initial_project(project).unwrap();
	app.editor.start_play(&mut app.scene).unwrap();
	app.standalone_runtime = true;
	app.input.reset();
	app.input.press(Key::ArrowRight);
	app.input.press(Key::Space);
	let before_camera = app.camera.position;
	let before_actor = app.scene.get(actor).unwrap().transform().position;
	app.update_player(1.0 / 30.0);
	assert!(app.scene.get(actor).unwrap().transform().position[0] > before_actor[0]);
	assert_eq!(app.camera.position, before_camera);
	assert!(!app.editor.simulation_paused());
}

#[test]
fn generic_hud_uses_module_status_and_follow_camera_ignores_debug_camera_input_while_paused() {
	use runtime::editor::command::{GameplayStatus, PlayStatusDto};
	let status = PlayStatusDto {
		play_session_id: "play".into(),
		run_id: "run".into(),
		scene_generation: 0,
		scene_id: SceneId::new("race").unwrap(),
		paused: true,
		pending_scene_id: None,
		failed_scene_id: None,
		failed_scene_attempt_id: None,
		sequence: None,
		resident_regions: Vec::new(),
		gameplay: Some(GameplayStatus {
			objective: "Win the race".into(),
			detail: Some("Lap three".into()),
			score: Some(42),
			completed: true,
		}),
	};
	assert_eq!(
		gameplay_hud_rows(Some(&status)),
		[
			"Win the race",
			"Lap three",
			"Score 42",
			"F10 to restart",
			"Paused"
		]
	);
	assert!(gameplay_hud_rows(None).is_empty());
	let (mut app, ids) = fixture();
	let fixed = app.camera.clone();
	let actor = app.scene.key(ids[0]).unwrap().clone();
	app.scene
		.set_camera_follow(Some(runtime::scene_data::FollowCameraSettings {
			target: actor,
			offset: [0.0, 2.0, 5.0],
			look_offset: [0.0; 3],
			smoothing_seconds: 0.2,
		}));
	app.editor =
		SceneEditor::for_project(&app.scene, &fixed, app.project.clone().unwrap()).unwrap();
	app.editor.start_play(&mut app.scene).unwrap();
	app.standalone_runtime = true;
	app.input.reset();
	app.input.press(Key::ArrowRight);
	app.update_player(0.0);
	let follow = app.editor.render_camera().unwrap();
	assert_eq!(app.camera.position, follow.position);
	app.input.reset();
	app.input.press(Key::Space);
	app.update_player(0.0);
	assert!(app.editor.simulation_paused());
	app.input.reset();
	app.input.press(Key::D);
	app.input.press(Key::ArrowRight);
	app.update_player(0.5);
	assert_eq!(app.camera.position, follow.position);
	assert!(!app.needs_continuous_redraw());
}

#[test]
fn removing_the_target_from_the_render_scene_keeps_the_runtime_camera_in_control() {
	let (mut app, [target, _]) = orbit_player();
	let accepted = app.editor.render_camera().unwrap();
	assert!(app.scene.remove(target).is_some());
	assert!(app.scene.camera_orbit().is_none());
	// A structural update can remove the render-side binding. Camera ownership
	// comes from the running rig even when that binding is absent.
	app.input.press(Key::ArrowRight);
	app.update_keyboard_camera(0.5);
	app.captured_motion(150.0, 50.0);
	assert_eq!(app.camera.position, accepted.position);
	assert_eq!(app.camera.yaw, accepted.yaw);
	assert_eq!(app.camera.pitch, accepted.pitch);

	app.camera = runtime::Camera::default();
	app.render().unwrap();
	assert_eq!(app.camera.position, accepted.position);
	assert_eq!(app.camera.yaw, accepted.yaw);
	assert_eq!(app.camera.pitch, accepted.pitch);
}

#[test]
fn player_captured_motion_and_wheel_commit_on_step_and_release_clears_pending_input() {
	let (mut app, ids) = orbit_player();
	let initial = app.camera.clone();
	// These fields are set only after a successful native cursor grab.
	app.mouse_captured = true;
	app.orbit_capture = Some(app.editor.camera_input_owner().unwrap());
	app.captured_motion(125.0, 50.0);
	app.submit_captured_orbit(normalize_wheel(WheelSample::Pixels(40.0)));
	assert_eq!(app.camera.position, initial.position);
	app.update_player(0.001);
	assert_eq!(app.camera.yaw, initial.yaw);
	app.update_player(1.0 / 60.0);
	assert!((app.camera.yaw - 0.25).abs() < 1e-6);
	assert!((app.camera.pitch + 0.1).abs() < 1e-6);
	let pivot = app.scene.get(ids[0]).unwrap().transform().position;
	let distance = app
		.camera
		.position
		.into_iter()
		.zip(pivot)
		.map(|(eye, target)| (eye - target).powi(2))
		.sum::<f32>()
		.sqrt();
	assert!((distance - 4.0 / 1.1).abs() < 1e-5);
	let committed = app.camera.position;
	app.captured_motion(500.0, -500.0);
	app.set_mouse_capture(false);
	assert!(app.orbit_capture.is_none());
	assert!(!app.mouse_captured);
	app.update_player(0.05);
	assert_eq!(committed, app.camera.position);

	// An obsolete grab cannot move the replacement camera or erase its queued input.
	let current = app.editor.camera_input_owner().unwrap();
	let mut obsolete = current.clone();
	obsolete.run_id.push_str("-obsolete");
	app.orbit_capture = Some(obsolete);
	app.mouse_captured = true;
	app.editor
		.submit_camera_input(
			&current,
			CameraIntent {
				yaw_delta: 0.25,
				..Default::default()
			},
		)
		.unwrap();
	app.captured_motion(500.0, 0.0);
	assert!(!app.mouse_captured);
	app.update_player(1.0 / 60.0);
	assert!((app.camera.yaw - 0.5).abs() < 1e-6);
}
