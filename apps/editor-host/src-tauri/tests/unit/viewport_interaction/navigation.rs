use super::*;

#[test]
fn navigation_and_view_preferences_never_change_authored_camera_revision_or_history() {
	let fixture = Fixture::new(2);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	select(&mut workspace, &["entity-0"]);
	let original = serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap();
	let original_position = controller.camera.position();
	for action in [
		ViewportActionDto::Zoom { factor: 0.8 },
		ViewportActionDto::FrameSelection,
		ViewportActionDto::Orient {
			orientation: ViewportOrientation::Top,
		},
		ViewportActionDto::SetGrid { visible: false },
		ViewportActionDto::SetGridSpacing { spacing: 0.25 },
		ViewportActionDto::SetSnap { enabled: true },
		ViewportActionDto::SetPlane {
			plane: ViewportPlane::Xy,
		},
		ViewportActionDto::SetSpace {
			space: ViewportSpace::Local,
		},
		ViewportActionDto::SetPivot {
			pivot: ViewportPivot::Center,
		},
	] {
		controller.action(action, &mut workspace, EXTENT).unwrap();
	}
	let mut down = event("pointer_down", [200.0, 200.0], Some("left"));
	down.modifiers.alt = true;
	send(&mut controller, &mut workspace, down);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_move", [240.0, 220.0], None),
	);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_up", [240.0, 220.0], Some("left")),
	);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_down", [200.0, 200.0], Some("middle")),
	);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_move", [220.0, 250.0], None),
	);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_up", [220.0, 250.0], Some("middle")),
	);
	let mut wheel = event("wheel_lines", [200.0, 200.0], None);
	wheel.delta = Some([0.0, 2.0]);
	send(&mut controller, &mut workspace, wheel);
	assert_ne!(controller.camera.position(), original_position);
	assert_eq!(
		serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap(),
		original
	);
	assert!(!workspace.backend.session().is_dirty());
	assert!(!workspace.backend.session().state().can_undo);
}

#[test]
fn scene_switches_restore_editor_camera_bookmarks_without_authoring_camera_data() {
	let fixture = Fixture::new(1);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	let first_id = workspace.backend.session().active_scene_id().clone();
	let second_id = runtime::scene_catalog::SceneId::new("second").unwrap();
	let document = (*workspace.backend.session().authored_document()).clone();
	command(
		&mut workspace,
		Command::AddScene {
			id: second_id.clone(),
			asset_path: "scenes/second.json".into(),
			document,
		},
	);
	controller.camera.pivot = Vec3::new(2.0, 3.0, 4.0);
	controller.camera.distance = 7.0;
	let first_pose = controller.camera.position();
	command(
		&mut workspace,
		Command::SwitchScene {
			scene_id: second_id,
		},
	);
	controller.sync(&mut workspace).unwrap();
	controller.camera.pivot = Vec3::new(-5.0, 8.0, 1.0);
	controller.camera.distance = 12.0;
	command(&mut workspace, Command::SwitchScene { scene_id: first_id });
	let authored_camera = workspace
		.backend
		.session()
		.authored_document()
		.camera
		.clone();
	let revision = workspace.backend.session().revision();
	controller.sync(&mut workspace).unwrap();
	assert!((controller.camera.position() - first_pose).length() < 1e-5);
	assert_eq!(
		workspace.backend.session().authored_document().camera,
		authored_camera
	);
	assert_eq!(workspace.backend.session().revision(), revision);
}

#[test]
fn scene_keyboard_requires_focus_and_preserves_transform_shortcuts() {
	let fixture = Fixture::new(1);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	let initial = controller.settings.tool;
	send(&mut controller, &mut workspace, key("KeyR"));
	assert_eq!(controller.settings.tool, initial);
	focus_keyboard(&mut controller, &mut workspace);
	for (code, tool) in [
		("KeyQ", ViewportTool::Hand),
		("KeyW", ViewportTool::Move),
		("KeyE", ViewportTool::Rotate),
		("KeyR", ViewportTool::Scale),
	] {
		send(&mut controller, &mut workspace, key(code));
		assert_eq!(controller.settings.tool, tool);
	}
	send(&mut controller, &mut workspace, key("Escape"));
	send(&mut controller, &mut workspace, key("KeyQ"));
	assert!(!controller.state().focused);
	assert_eq!(controller.settings.tool, ViewportTool::Scale);
}

#[test]
fn middle_mouse_modifiers_choose_orbit_pan_and_dolly() {
	let fixture = Fixture::new(1);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	let initial = controller.camera.clone();
	let start = [300.0, 300.0];
	let end = [340.0, 330.0];
	send(
		&mut controller,
		&mut workspace,
		event("pointer_down", start, Some("middle")),
	);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_up", start, Some("right")),
	);
	assert!(controller.captured());
	send(
		&mut controller,
		&mut workspace,
		event("pointer_move", end, None),
	);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_up", end, Some("middle")),
	);
	assert!(!controller.captured());
	assert_ne!(controller.camera.yaw, initial.yaw);
	assert_eq!(controller.camera.pivot, initial.pivot);
	assert_eq!(controller.camera.distance, initial.distance);
	let orbit = controller.camera.clone();
	let mut pan = event("pointer_down", start, Some("middle"));
	pan.modifiers.shift = true;
	send(&mut controller, &mut workspace, pan);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_move", end, None),
	);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_up", end, Some("middle")),
	);
	assert_eq!(controller.camera.yaw, orbit.yaw);
	assert_eq!(controller.camera.distance, orbit.distance);
	assert_ne!(controller.camera.pivot, orbit.pivot);
	let panned = controller.camera.clone();
	let mut dolly = event("pointer_down", start, Some("middle"));
	dolly.modifiers.control = true;
	send(&mut controller, &mut workspace, dolly);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_move", end, None),
	);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_up", end, Some("middle")),
	);
	assert_eq!(controller.camera.pivot, panned.pivot);
	assert!(controller.camera.distance > panned.distance);
	assert!(controller.camera.ortho_height > panned.ortho_height);
}

#[test]
fn walk_uses_wasd_and_mouse_look_and_only_explicit_cancel_restores_the_camera() {
	let fixture = Fixture::new(1);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	focus_keyboard(&mut controller, &mut workspace);
	send(&mut controller, &mut workspace, key("KeyE"));
	let initial = controller.camera.clone();
	for boundary in ["Escape", "right", "Enter"] {
		start_walk(&mut controller, &mut workspace);
		controller.look_from_fly_motion([50.0, 30.0]);
		assert_ne!(controller.camera.yaw, initial.yaw);
		assert!(controller
			.camera
			.position()
			.abs_diff_eq(initial.position(), 1.0e-5));
		send(&mut controller, &mut workspace, key("KeyW"));
		assert!(controller.needs_camera_tick());
		controller.camera_tick(0.05);
		assert!(!controller
			.camera
			.position()
			.abs_diff_eq(initial.position(), 1.0e-5));
		assert_eq!(controller.settings.tool, ViewportTool::Rotate);
		if boundary == "right" {
			send(
				&mut controller,
				&mut workspace,
				event("pointer_down", [300.0, 300.0], Some("right")),
			);
		} else {
			send(&mut controller, &mut workspace, key(boundary));
		}
		assert!(!controller.captured());
		assert!(!controller.needs_camera_tick());
		assert_eq!(controller.camera.yaw == initial.yaw, boundary != "Enter");
		if boundary == "Escape" {
			assert!(!controller.state().focused);
			focus_keyboard(&mut controller, &mut workspace);
		}
	}
	assert!(!workspace.backend.session().is_dirty());
}

#[test]
fn numpad_changes_orientation_and_projection() {
	let fixture = Fixture::new(1);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	focus_keyboard(&mut controller, &mut workspace);
	send(&mut controller, &mut workspace, key("Numpad1"));
	assert!(controller.camera.forward().abs_diff_eq(-Vec3::Z, 1.0e-5));
	let mut back = key("Numpad1");
	back.modifiers.control = true;
	send(&mut controller, &mut workspace, back);
	assert!(controller.camera.forward().abs_diff_eq(Vec3::Z, 1.0e-5));
	send(&mut controller, &mut workspace, key("Numpad9"));
	assert!(controller.camera.forward().abs_diff_eq(-Vec3::Z, 1.0e-5));
	send(&mut controller, &mut workspace, key("Numpad5"));
	assert_eq!(
		controller.settings.projection,
		ViewportProjection::Perspective
	);
}
