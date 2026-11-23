use super::*;
use runtime::{
	EntityKey, SceneData, Transform, assets::Model, model_instance::ModelInstance, scene::EntityId,
};
use std::{
	fs,
	sync::atomic::{AtomicU64, Ordering},
	time::Duration,
};

struct TestDirectory(PathBuf);

impl TestDirectory {
	fn new() -> Self {
		static NEXT_ID: AtomicU64 = AtomicU64::new(0);
		// Workspace target permits rename; the Windows sandbox's Temp does not.
		let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
			.join("../../target")
			.join(format!(
				"game-engine-app-scene-{}-{}",
				std::process::id(),
				NEXT_ID.fetch_add(1, Ordering::Relaxed)
			));
		fs::create_dir(&directory).unwrap();
		Self(directory)
	}
}

impl Drop for TestDirectory {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.0);
	}
}

fn fixture() -> (GameApp, [EntityId; 2]) {
	let model = Arc::new(Model::new(Vec::new()));
	let mut app = GameApp::default();
	let ids = [-0.8, 0.8].map(|x| {
		app.scene
			.spawn_keyed_model(
				EntityKey::new(format!("robot-{x}")).unwrap(),
				format!("Robot at {x}"),
				"props/robot.glb",
				ModelInstance::new(
					Arc::clone(&model),
					Transform {
						position: [x, 0.0, 0.0],
						scale: [0.7; 3],
						..Transform::default()
					},
				),
			)
			.unwrap()
	});
	app.scene.set_binding("robot_a", ids[0]).unwrap();
	app.scene.set_binding("robot_b", ids[1]).unwrap();
	app.editor = SceneEditor::from_scene(&app.scene);
	let data = app.scene.to_data(&app.camera).unwrap();
	let mut project =
		runtime::scene_editor::ProjectData::for_scene("QA authored project", "robots.json", &data)
			.unwrap();
	use runtime::project::BehaviorSpec;
	for behavior in [
		BehaviorSpec::ControlledMove {
			entity: app.scene.key(ids[0]).unwrap().clone(),
			speed: 1.0,
			space: Default::default(),
		},
		BehaviorSpec::Spin {
			entity: app.scene.key(ids[0]).unwrap().clone(),
			angular_velocity: [0.6, 0.9, 0.0],
		},
		BehaviorSpec::Spin {
			entity: app.scene.key(ids[1]).unwrap().clone(),
			angular_velocity: [-0.5, 0.75, 0.2],
		},
	] {
		project.add_behavior(behavior);
	}
	app.project = Some(project.clone());
	app.editor.set_initial_project(project).unwrap();
	app.input.press(Key::W);
	app.input.press(Key::ArrowRight);
	(app, ids)
}

fn snapshot(app: &GameApp) -> String {
	app.scene.to_data(&app.camera).unwrap().to_json().unwrap()
}

fn prime_clock(app: &mut GameApp) -> Instant {
	let start = Instant::now();
	assert_eq!(app.clock.tick(start), Duration::ZERO);
	start
}

fn orbit_player() -> (GameApp, [EntityId; 2]) {
	let (mut app, ids) = fixture();
	app.scene
		.set_camera_orbit(Some(runtime::scene_data::OrbitCameraSettings {
			target: app.scene.key(ids[0]).unwrap().clone(),
			pivot_offset: [0.0; 3],
			initial_yaw: 0.0,
			initial_pitch: 0.0,
			initial_distance: 4.0,
			min_distance: 1.0,
			max_distance: 10.0,
			min_pitch: -1.0,
			max_pitch: 1.0,
			follow_seconds: 0.0,
			recovery_seconds: 0.0,
			teleport_distance: 5.0,
			probe_radius: 0.3,
			obstruction_mask: 1,
		}));
	app.editor =
		SceneEditor::for_project(&app.scene, &app.camera, app.project.clone().unwrap()).unwrap();
	app.standalone_runtime = true;
	app.input.reset();
	app.editor.start_play(&mut app.scene).unwrap();
	app.update_player(0.0);
	(app, ids)
}

#[test]
fn helpers_report_missing_configuration_without_mutating_live_state() {
	let directory = TestDirectory::new();
	let (mut app, _) = fixture();
	let before = snapshot(&app);
	assert!(app.save_scene().unwrap_err().contains("Scene path"));
	assert!(app.reload_scene().unwrap_err().contains("Asset root"));
	app.asset_root = Some(directory.0.clone());
	let project = app.project.take().unwrap();
	assert!(
		app.reload_scene()
			.unwrap_err()
			.contains("Project configuration")
	);
	let scene_path = directory.0.join(project.startup_scene_path().unwrap());
	fs::create_dir_all(scene_path.parent().unwrap()).unwrap();
	fs::write(&scene_path, &before).unwrap();
	app.project = Some(project);
	assert!(app.reload_scene().unwrap_err().contains("Renderer"));
	assert_eq!(snapshot(&app), before);
	assert_eq!(fs::read_to_string(&scene_path).unwrap(), before);
}

#[test]
fn installed_player_ignores_authoring_shortcuts_while_simulation_and_camera_continue() {
	let directory = TestDirectory::new();
	let path = directory.0.join("robots.json");
	fs::write(&path, "installed project scene").unwrap();
	let (mut app, [left, _]) = fixture();
	app.scene_path = Some(path.clone());
	app.project_path = Some(directory.0.join("project.json"));
	app.asset_root = Some(directory.0.clone());
	app.project_name = Some("Installed project".to_owned());
	app.standalone_runtime = true;
	app.start_play().unwrap();
	app.input.reset();
	let start = prime_clock(&mut app);
	app.input.press(Key::F5);
	app.input.press(Key::F9);
	app.input.press(Key::W);
	app.input.press(Key::ArrowRight);
	let before = app.scene.get(left).unwrap().transform().position;
	app.update(0.1);
	assert!(app.editor.is_playing());
	assert!(app.scene.get(left).unwrap().transform().position[1] > before[1]);
	assert!((app.camera.position[0] - 0.2).abs() < 1e-5);
	assert_eq!(fs::read_to_string(path).unwrap(), "installed project scene");
	assert_eq!(
		app.clock.tick(start + Duration::from_millis(16)),
		Duration::from_millis(16)
	);
}

#[test]
fn structural_shortcuts_obey_priority_consume_the_frame_and_reset_the_clock() {
	for (keys, expected_count) in [(vec![Key::Delete, Key::C], 1), (vec![Key::C], 3)] {
		let (mut app, _) = fixture();
		let start = prime_clock(&mut app);
		for key in keys {
			app.input.press(key);
		}
		app.update(0.5);
		assert_eq!(app.scene.instances().count(), expected_count);
		assert_eq!(app.camera.position, [0.0, 0.0, 3.0]);
		assert_eq!(
			app.clock.tick(start + Duration::from_secs(1)),
			Duration::ZERO
		);
		app.input.end_frame();
		app.update(0.0);
		assert_eq!(app.scene.instances().count(), expected_count);
	}
}

#[test]
fn save_has_priority_over_structural_commands_and_keeps_the_original_entity_list() {
	let directory = TestDirectory::new();
	let (mut app, _) = fixture();
	let path = directory.0.join("robots.json");
	app.scene_path = Some(path.clone());
	app.project_path = Some(directory.0.join("project.json"));
	let before = snapshot(&app);
	for key in [Key::F5, Key::Delete, Key::C] {
		app.input.press(key);
	}
	app.update(1.0);
	assert_eq!(snapshot(&app), before);
	let saved = SceneData::from_json(&fs::read_to_string(path).unwrap()).unwrap();
	assert_eq!(saved.entities.len(), 2);
	assert_eq!(saved.to_json().unwrap(), before);
}

fn press(app: &mut GameApp, keys: &[Key], delta: f32) {
	app.input.reset();
	for key in keys {
		app.input.press(*key);
	}
	app.update(delta);
	app.input.end_frame();
}

#[test]
fn edit_is_default_and_edit_movement_does_not_run_robot_animation() {
	let (mut app, [left, right]) = fixture();
	assert!(app.authored.is_none());
	app.update(0.5);
	assert_eq!(
		app.scene.get(left).unwrap().transform().position,
		[-0.8, 0.5, 0.0]
	);
	for id in [left, right] {
		assert_eq!(app.scene.get(id).unwrap().transform().rotation, [0.0; 3]);
	}
	assert_eq!(app.editor.selected_entity(), Some(left));
}

#[test]
fn play_clones_transforms_reuses_models_and_stop_restores_authored_state_and_edit_history() {
	let (mut app, [left, right]) = fixture();
	press(&mut app, &[Key::W], 0.2);
	let before = snapshot(&app);
	let model_pointer = app.scene.get(left).unwrap().model() as *const Model;
	app.start_play().unwrap();
	assert!(app.editor.is_playing());
	assert!(std::ptr::eq(
		app.scene.get(left).unwrap().model(),
		app.authored
			.as_ref()
			.unwrap()
			.scene
			.get(left)
			.unwrap()
			.model()
	));
	assert_eq!(
		app.scene.get(left).unwrap().model() as *const Model,
		model_pointer
	);
	app.start_play().unwrap(); // Repeated start cannot replace the authored snapshot.
	press(&mut app, &[Key::W, Key::ArrowRight], 0.5);
	assert_ne!(snapshot(&app), before);
	assert_eq!(
		app.authored
			.as_ref()
			.unwrap()
			.scene
			.get(left)
			.unwrap()
			.transform()
			.position,
		[-0.8, 0.2, 0.0]
	);
	assert_ne!(app.scene.get(right).unwrap().transform().rotation, [0.0; 3]);
	app.stop_play().unwrap();
	assert!(app.authored.is_none());
	assert_eq!(snapshot(&app), before);
	press(&mut app, &[Key::ControlLeft, Key::Z], 0.0);
	assert_eq!(
		app.scene.get(left).unwrap().transform().position,
		[-0.8, 0.0, 0.0]
	);
	press(&mut app, &[Key::ControlRight, Key::Y], 0.0);
	assert_eq!(snapshot(&app), before);
}

#[test]
fn f5_during_play_saves_authored_scene_and_camera_instead_of_simulation_values() {
	let directory = TestDirectory::new();
	let (mut app, _) = fixture();
	let path = directory.0.join("robots.json");
	app.scene_path = Some(path.clone());
	app.project_path = Some(directory.0.join("project.json"));
	let authored = snapshot(&app);
	app.start_play().unwrap();
	press(&mut app, &[Key::W, Key::ArrowRight], 0.5);
	let simulated = snapshot(&app);
	assert_ne!(simulated, authored);
	press(&mut app, &[Key::F5], 1.0);
	assert_eq!(
		SceneData::from_json(&fs::read_to_string(path).unwrap())
			.unwrap()
			.to_json()
			.unwrap(),
		authored
	);
	assert_eq!(snapshot(&app), simulated);
	assert!(app.authored.is_some());
}

#[test]
fn generic_empty_or_unbound_scenes_can_play_and_edit_without_a_robot_game() {
	for empty in [false, true] {
		let (mut app, ids) = fixture();
		if empty {
			app.scene = Scene::default();
		} else {
			app.scene.remove(ids[0]).unwrap(); // Leaves only robot_b bound.
		}
		app.editor = SceneEditor::from_scene(&app.scene);
		let before = snapshot(&app);
		app.start_play().unwrap();
		assert!(app.authored.is_some());
		press(&mut app, &[Key::W, Key::Tab, Key::E], 0.5);
		assert_eq!(snapshot(&app), before);
		app.stop_play().unwrap();
		assert_eq!(snapshot(&app), before);
	}
}

#[test]
fn f6_is_edge_triggered_resets_input_clock_and_blocks_structural_edits_during_play() {
	let (mut app, _) = fixture();
	let start = prime_clock(&mut app);
	press(&mut app, &[Key::F6, Key::Delete, Key::W], 0.5);
	assert!(app.authored.is_some());
	assert_eq!(app.scene.instances().count(), 2);
	assert_eq!(
		app.clock.tick(start + Duration::from_secs(1)),
		Duration::ZERO
	);
	assert!(!app.input.is_held(Key::F6));
	for key in [Key::Delete, Key::C] {
		let before = snapshot(&app);
		press(&mut app, &[key], 1.0);
		assert_eq!(snapshot(&app), before);
		assert_eq!(app.scene.instances().count(), 2);
	}
	press(&mut app, &[Key::F6], 0.0);
	assert!(app.authored.is_none());
	press(&mut app, &[Key::Delete], 0.0);
	assert_eq!(app.scene.instances().count(), 1);
	press(&mut app, &[Key::ControlLeft, Key::Z], 0.0);
	assert_eq!(app.scene.instances().count(), 2);
}

#[test]
fn play_mode_boundaries_suppress_mouse_buttons_until_release() {
	use runtime::MouseButton;
	let (mut app, _) = fixture();
	for button in [MouseButton::Left, MouseButton::Other(7)] {
		app.input.press(button);
	}
	app.start_play().unwrap();
	for button in [MouseButton::Left, MouseButton::Other(7)] {
		assert!(!app.input.is_held(button));
		assert!(!app.input.just_pressed(button));
		app.input.press(button);
		assert!(!app.input.is_held(button));
		app.input.release(button);
		app.input.press(button);
		assert!(app.input.is_held(button));
	}
	app.stop_play().unwrap();
	for button in [MouseButton::Left, MouseButton::Other(7)] {
		app.input.press(button);
		assert!(!app.input.is_held(button));
		app.input.release(button);
		app.input.press(button);
		assert!(app.input.just_pressed(button));
	}
}

#[test]
fn failed_reload_during_play_preserves_mode_authored_snapshot_editor_history_and_disk() {
	let directory = TestDirectory::new();
	let (mut app, [left, _]) = fixture();
	let path = directory.0.join("robots.json");
	fs::write(&path, "external edits").unwrap();
	app.scene_path = Some(path.clone());
	app.project_path = Some(directory.0.join("project.json"));
	app.asset_root = Some(directory.0.clone());
	press(&mut app, &[Key::W], 0.2);
	app.start_play().unwrap();
	press(&mut app, &[Key::W], 0.3);
	let simulation = snapshot(&app);
	let authored = app
		.authored
		.as_ref()
		.unwrap()
		.scene
		.to_data(&app.authored.as_ref().unwrap().camera)
		.unwrap()
		.to_json()
		.unwrap();
	press(&mut app, &[Key::F9, Key::F5, Key::F6], 0.5);
	assert!(app.authored.is_some());
	assert_eq!(snapshot(&app), simulation);
	assert_eq!(
		app.authored
			.as_ref()
			.unwrap()
			.scene
			.to_data(&app.authored.as_ref().unwrap().camera)
			.unwrap()
			.to_json()
			.unwrap(),
		authored
	);
	assert_eq!(fs::read_to_string(path).unwrap(), "external edits");
	app.stop_play().unwrap();
	press(&mut app, &[Key::ControlLeft, Key::Z], 0.0);
	assert_eq!(
		app.scene.get(left).unwrap().transform().position,
		[-0.8, 0.0, 0.0]
	);
}

#[test]
fn holding_f6_does_not_toggle_modes_again_on_keyboard_repeat() {
	let (mut app, _) = fixture();
	app.input.reset();
	app.input.press(Key::F6);
	app.update(0.0);
	app.input.end_frame();
	assert!(app.authored.is_some());
	// Keyboard repeat sends another Pressed event without a Released event.
	app.input.press(Key::F6);
	app.update(0.0);
	app.input.end_frame();
	assert!(
		app.authored.is_some(),
		"a held F6 must not leave Play on OS key repeat"
	);
	app.input.release(Key::F6);
	app.input.end_frame();
	app.input.press(Key::F6);
	app.update(0.0);
	assert!(app.authored.is_none());
}

#[test]
fn dirty_state_tracks_edits_undo_save_failures_and_authored_state_during_play() {
	let directory = TestDirectory::new();
	let (mut app, [left, _]) = fixture();
	app.input.reset();
	app.tools.saved = Some(app.scene.to_data(&app.camera).unwrap());
	assert!(!app.is_dirty());
	app.editor
		.rename_selected(&mut app.scene, "Changed")
		.unwrap();
	assert!(app.is_dirty());
	press(&mut app, &[Key::ControlLeft, Key::Z], 0.0);
	assert!(!app.is_dirty());
	app.scene_path = Some(directory.0.join("robots.json"));
	app.project_path = Some(directory.0.join("project.json"));
	app.save_scene().unwrap();
	app.start_play().unwrap();
	press(&mut app, &[Key::W, Key::ArrowRight], 0.5);
	assert!(!app.is_dirty(), "simulation must not dirty authored state");
	app.stop_play().unwrap();
	let mut transform = *app.scene.get(left).unwrap().transform();
	transform.position[0] += 1.0;
	app.editor
		.set_selected_transform(&mut app.scene, transform)
		.unwrap();
	assert!(app.is_dirty());
	let baseline = app.tools.saved.as_ref().unwrap().to_json().unwrap();
	app.scene_path = Some(directory.0.join("missing/robots.json"));
	app.project_path = Some(directory.0.join("project.json"));
	assert!(app.save_scene().is_err());
	assert!(app.is_dirty());
	assert_eq!(
		app.tools.saved.as_ref().unwrap().to_json().unwrap(),
		baseline
	);
}

#[test]
fn catalog_placement_uses_shared_cached_models_and_is_undoable() {
	let (mut app, [left, _]) = fixture();
	app.input.reset();
	let template = app.scene.get(left).unwrap().clone();
	app.tools.models = vec![runtime::assets::catalog::ModelAsset {
		name: "Robot".into(),
		path: "props/robot.glb".into(),
	}];
	app.tools
		.templates
		.insert("props/robot.glb".into(), template);
	app.camera.position = [1.0, 2.0, 5.0];
	app.place_model("props/robot.glb").unwrap();
	let placed = app.editor.selected_entity().unwrap();
	assert_eq!(
		app.scene.get(placed).unwrap().transform().position,
		[1.0, 2.0, 2.0]
	);
	assert!(std::ptr::eq(
		app.scene.get(placed).unwrap().model(),
		app.scene.get(left).unwrap().model()
	));
	press(&mut app, &[Key::ControlLeft, Key::Z], 0.0);
	assert!(!app.scene.contains(placed));
	press(&mut app, &[Key::ControlLeft, Key::Y], 0.0);
	assert!(app.scene.contains(placed));
	assert!(app.place_model("../outside.glb").is_err());
	app.start_play().unwrap();
	assert!(app.place_model("props/robot.glb").is_err());
}

#[test]
fn property_commit_targets_the_original_entity_and_rejects_a_deleted_entity() {
	use crate::editor_panel::{PanelAction, Property};
	let (mut app, [left, right]) = fixture();
	app.input.reset();
	app.panel_action(PanelAction::Edit(Property::Position(1)))
		.unwrap();
	app.tools.panel.edit.as_mut().unwrap().text = "4".into();
	app.editor.select(&app.scene, right).unwrap();
	app.commit_property().unwrap();
	assert_eq!(app.editor.selected_entity(), Some(left));
	assert_eq!(app.scene.get(left).unwrap().transform().position[1], 4.0);
	assert_eq!(app.scene.get(right).unwrap().transform().position[1], 0.0);

	app.panel_action(PanelAction::Edit(Property::Name)).unwrap();
	app.scene.remove(left).unwrap();
	let before = snapshot(&app);
	assert!(app.commit_property().is_err());
	assert_eq!(snapshot(&app), before);
}

#[test]
fn panel_edit_suppresses_held_movement_until_release_and_ends_the_gesture() {
	use crate::editor_panel::{PanelAction, Property};
	let (mut app, [left, _]) = fixture();
	press(&mut app, &[Key::W], 0.2);
	app.panel_action(PanelAction::Edit(Property::Name)).unwrap();
	app.tools.panel.edit = None; // Cancel the text field without committing.
	app.input.press(Key::W); // OS repeat must not restart movement.
	app.update(0.2);
	assert_eq!(app.scene.get(left).unwrap().transform().position[1], 0.2);
	app.input.release(Key::W);
	app.input.end_frame();
	app.input.press(Key::W);
	app.update(0.3);
	assert_eq!(app.scene.get(left).unwrap().transform().position[1], 0.5);
	press(&mut app, &[Key::ControlLeft, Key::Z], 0.0);
	assert_eq!(app.scene.get(left).unwrap().transform().position[1], 0.2);
	press(&mut app, &[Key::ControlLeft, Key::Z], 0.0);
	assert_eq!(app.scene.get(left).unwrap().transform().position[1], 0.0);
}

#[test]
fn successful_panel_commands_reset_timing_and_require_fresh_movement_presses() {
	use crate::editor_panel::PanelAction;
	let directory = TestDirectory::new();
	for action_index in 0..4 {
		let (mut app, [left, right]) = fixture();
		app.update(0.2);
		app.input.end_frame();
		app.scene_path = Some(directory.0.join("robots.json"));
		app.project_path = Some(directory.0.join("project.json"));
		app.asset_root = Some(directory.0.clone());
		app.tools.models = vec![runtime::assets::catalog::ModelAsset {
			name: "Robot".into(),
			path: "props/robot.glb".into(),
		}];
		app.tools.templates.insert(
			"props/robot.glb".into(),
			app.scene.get(left).unwrap().clone(),
		);
		let start = prime_clock(&mut app);
		let action = match action_index {
			0 => PanelAction::Save,
			1 => PanelAction::RefreshAssets,
			2 => PanelAction::Place("props/robot.glb".into()),
			_ => PanelAction::Select(right),
		};
		app.panel_action(action).unwrap();
		assert_eq!(
			app.clock.tick(start + Duration::from_secs(60)),
			Duration::ZERO
		);
		assert_eq!(
			app.clock
				.tick(start + Duration::from_secs(60) + Duration::from_millis(16)),
			Duration::from_millis(16)
		);
		let after = snapshot(&app);
		for key in [Key::W, Key::ArrowRight] {
			assert!(!app.input.is_held(key));
			assert!(!app.input.just_pressed(key));
			app.input.press(key); // A repeated key-down is still suppressed.
			assert!(!app.input.is_held(key));
		}
		app.update(0.5);
		app.input.end_frame();
		assert_eq!(snapshot(&app), after);
		for key in [Key::W, Key::ArrowRight] {
			app.input.release(key);
		}
		app.input.end_frame();
		for key in [Key::W, Key::ArrowRight] {
			app.input.press(key);
		}
		let selected = app.editor.selected_entity().unwrap();
		let position = app.scene.get(selected).unwrap().transform().position;
		let camera_x = app.camera.position[0];
		app.update(0.1);
		assert!(
			(app.scene.get(selected).unwrap().transform().position[1] - position[1] - 0.1).abs()
				< 1e-5
		);
		assert!((app.camera.position[0] - camera_x - 0.2).abs() < 1e-5);
		press(&mut app, &[Key::ControlLeft, Key::Z], 0.0);
		assert_eq!(
			app.scene.get(selected).unwrap().transform().position,
			position
		);
	}
}

#[test]
fn failed_panel_commands_still_reset_timing_suppress_input_and_end_movement_gestures() {
	use crate::editor_panel::PanelAction;
	let directory = TestDirectory::new();
	for action in [
		PanelAction::Save,
		PanelAction::Place("props/missing.glb".into()),
		PanelAction::RefreshAssets,
		PanelAction::Reload,
	] {
		let (mut app, [left, _]) = fixture();
		app.update(0.2);
		app.input.end_frame();
		app.scene_path = Some(directory.0.join("missing/robots.json"));
		app.project_path = Some(directory.0.join("project.json"));
		let before = snapshot(&app);
		let start = prime_clock(&mut app);
		assert!(app.panel_action(action).is_err());
		assert_eq!(
			app.clock.tick(start + Duration::from_secs(60)),
			Duration::ZERO
		);
		for key in [Key::W, Key::ArrowRight] {
			assert!(!app.input.is_held(key));
			assert!(!app.input.just_pressed(key));
			app.input.press(key);
			assert!(!app.input.is_held(key));
		}
		app.update(0.5);
		app.input.end_frame();
		assert_eq!(snapshot(&app), before);
		app.input.release(Key::W);
		app.input.end_frame();
		app.input.press(Key::W);
		app.update(0.3);
		assert_eq!(app.scene.get(left).unwrap().transform().position[1], 0.5);
		press(&mut app, &[Key::ControlLeft, Key::Z], 0.0);
		assert_eq!(app.scene.get(left).unwrap().transform().position[1], 0.2);
		press(&mut app, &[Key::ControlLeft, Key::Z], 0.0);
		assert_eq!(app.scene.get(left).unwrap().transform().position[1], 0.0);
	}
	assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 0);
}

#[test]
fn panel_actions_guard_play_and_installed_projects_but_allow_play_save_and_stop() {
	use crate::editor_panel::{PanelAction, Property, TextEdit};
	use runtime::gizmo::GizmoMode;
	let directory = TestDirectory::new();
	let (mut app, [left, right]) = fixture();
	app.scene_path = Some(directory.0.join("robots.json"));
	app.project_path = Some(directory.0.join("project.json"));
	app.asset_root = Some(directory.0.clone());
	app.start_play().unwrap();
	let before = snapshot(&app);
	let selected = app.editor.selected_entity();
	for action in [
		PanelAction::Select(right),
		PanelAction::Edit(Property::Name),
		PanelAction::Mode(GizmoMode::Rotate),
		PanelAction::Place("props/robot.glb".into()),
		PanelAction::RefreshAssets,
	] {
		assert!(app.panel_action(action).unwrap_err().contains("Stop Play"));
		assert_eq!(snapshot(&app), before);
		assert_eq!(app.editor.selected_entity(), selected);
		assert!(app.tools.panel.edit.is_none());
		assert_eq!(app.tools.panel.mode, GizmoMode::Translate);
	}
	// A pending callback must still be rejected if mode changed after editing began.
	app.tools.panel.edit = Some(TextEdit {
		entity: left,
		property: Property::Name,
		text: "Forbidden".into(),
		replace: false,
	});
	assert!(app.commit_property().unwrap_err().contains("Stop Play"));
	assert_eq!(snapshot(&app), before);
	app.panel_action(PanelAction::Save).unwrap();
	assert!(app.scene_path.as_ref().unwrap().exists());
	// Reload reaches its configuration check instead of the Play guard.
	assert!(
		app.panel_action(PanelAction::Reload)
			.unwrap_err()
			.contains("Renderer")
	);
	assert!(app.authored.is_some());
	app.panel_action(PanelAction::TogglePlay).unwrap();
	assert!(app.authored.is_none());

	app.project_name = Some("Installed".into());
	app.standalone_runtime = true;
	let before = snapshot(&app);
	for action in [
		PanelAction::Select(right),
		PanelAction::Edit(Property::Name),
		PanelAction::Mode(GizmoMode::Scale),
		PanelAction::Place("props/robot.glb".into()),
		PanelAction::RefreshAssets,
		PanelAction::Save,
		PanelAction::Reload,
		PanelAction::TogglePlay,
	] {
		assert!(
			app.panel_action(action)
				.unwrap_err()
				.contains("Player mode")
		);
		assert_eq!(snapshot(&app), before);
		assert!(app.authored.is_none());
	}
}

#[test]
fn cancelled_and_zero_distance_drags_preserve_existing_redo() {
	use runtime::gizmo::{GizmoDrag, GizmoMode};
	let (mut app, [left, _]) = fixture();
	app.input.reset();
	app.editor
		.rename_selected(&mut app.scene, "Redo label")
		.unwrap();
	let renamed = snapshot(&app);
	press(&mut app, &[Key::ControlLeft, Key::Z], 0.0);
	let before = *app.scene.get(left).unwrap().transform();
	for cancel in [true, false] {
		let drag = GizmoDrag::begin(
			left,
			before,
			&app.camera,
			[800, 600],
			[400.0, 300.0],
			GizmoMode::Translate,
			0,
		)
		.unwrap();
		if cancel {
			*app.scene.get_mut(left).unwrap().transform_mut() =
				drag.preview([450.0, 300.0]).unwrap();
		}
		app.tools.drag = Some(drag);
		if cancel {
			app.cancel_drag();
		} else {
			app.finish_drag().unwrap();
		}
		assert!(app.tools.drag.is_none());
		assert_eq!(*app.scene.get(left).unwrap().transform(), before);
	}
	app.finish_drag().unwrap(); // Repeated mouse release is harmless.
	press(&mut app, &[Key::ControlLeft, Key::Y], 0.0);
	assert_eq!(snapshot(&app), renamed);
}

#[test]
fn frame_metrics_average_samples_and_discard_focus_stalls() {
	let mut metrics = crate::editor_tools::FrameMetrics::default();
	let start = Instant::now();
	metrics.record(start, 99.0); // Initial timestamp is not a frame sample.
	for tick in 1..=5 {
		metrics.record(start + Duration::from_millis(tick * 100), 0.004);
	}
	assert!((metrics.frame_ms - 100.0).abs() < 0.001);
	assert!((metrics.cpu_ms - 4.0).abs() < 0.001);
	metrics.record(start + Duration::from_millis(600), 99.0); // Discard with stalled window.
	metrics.record(start + Duration::from_secs(10), 99.0);
	for tick in 1..=5 {
		metrics.record(
			start + Duration::from_secs(10) + Duration::from_millis(tick * 100),
			0.002,
		);
	}
	assert!((metrics.frame_ms - 100.0).abs() < 0.001);
	assert!((metrics.cpu_ms - 2.0).abs() < 0.001);
}

#[test]
fn saving_current_scene_preserves_catalog_ids_and_inactive_scene_behaviors() {
	use runtime::project::{BehaviorSpec, SceneBehavior};

	let directory = TestDirectory::new();
	let (mut app, _) = fixture();
	app.input.reset();
	let mut project = app.editor.project().unwrap().clone();
	let secondary = runtime::project::ProjectData::for_scene(
		"Secondary fixture",
		"scenes/secondary.json",
		&app.scene.to_data(&app.camera).unwrap(),
	)
	.unwrap();
	let secondary_id = secondary.startup_scene_id.clone();
	project
		.scenes
		.insert(secondary.scenes.iter().next().unwrap().clone())
		.unwrap();
	project.behaviors.push(SceneBehavior {
		scene_id: secondary_id,
		behavior: BehaviorSpec::Spin {
			entity: EntityKey::new("only-in-secondary").unwrap(),
			angular_velocity: [0.0, 1.0, 0.0],
		},
	});
	app.project = Some(project.clone());
	app.editor.set_initial_project(project.clone()).unwrap();
	app.scene_path = Some(directory.0.join("robots.json"));
	app.project_path = Some(directory.0.join("project.json"));
	app.save_scene().unwrap();
	let saved = runtime::project::ProjectData::load(app.project_path.as_ref().unwrap()).unwrap();
	assert_eq!(saved, project);
	assert_eq!(app.editor.project(), Some(&project));
	assert!(!app.is_dirty());
}

#[test]
fn development_stop_after_a_scene_transition_restores_authored_topology_and_undo() {
	let (mut app, [left, _]) = fixture();
	let before_delete = snapshot(&app);
	let mut project = app.editor.project().unwrap().clone();
	let finish = SceneId::new("finish").unwrap();
	project
		.scenes
		.insert(
			runtime::scene_catalog::SceneDescriptor::new(finish.clone(), "scenes/finish.json")
				.unwrap(),
		)
		.unwrap();
	app.editor.set_initial_project(project).unwrap();
	app.editor.select(&app.scene, left).unwrap();
	app.editor.delete_selected(&mut app.scene).unwrap();
	let authored = snapshot(&app);
	app.start_play().unwrap();

	let destination = Scene::default();
	let document = Arc::new(destination.to_data(&Camera::default()).unwrap());
	let prepared = app
		.editor
		.prepare_play_scene(finish.clone(), document)
		.unwrap();
	app.input.press(Key::D);
	app.editor
		.commit_play_scene(prepared, &mut app.input, true)
		.unwrap();
	app.scene = destination;
	assert!(!app.input.is_held(Key::D));
	assert_eq!(app.editor.play_scene_id(), Some(&finish));
	assert_ne!(snapshot(&app), authored);

	app.stop_play().unwrap();
	assert_eq!(snapshot(&app), authored);
	assert!(app.editor.play_scene_id().is_none());
	press(&mut app, &[Key::ControlLeft, Key::Z], 0.0);
	assert_eq!(snapshot(&app), before_delete);
}

#[path = "tests_app_scene_io/camera_input.rs"]
mod camera_input;
#[path = "tests_app_scene_io/gestures.rs"]
mod gestures;
#[path = "tests_app_scene_io/persistence.rs"]
mod persistence;
