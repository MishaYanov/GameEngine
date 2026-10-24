use super::*;
use runtime::{EntityKey, Transform, assets::Model, model_instance::ModelInstance};
use std::sync::Arc;

fn assert_close(actual: f32, expected: f32) {
	assert!(
		(actual - expected).abs() < 1e-5,
		"expected {expected}, got {actual}"
	);
}

fn assert_vector(actual: [f32; 3], expected: [f32; 3]) {
	for (actual, expected) in actual.into_iter().zip(expected) {
		assert_close(actual, expected);
	}
}

fn held_input(keys: &[Key]) -> InputState {
	let mut input = InputState::default();
	for key in keys {
		input.press(*key);
	}
	input
}

fn scene_with_robots() -> (Scene, RobotGame) {
	let model = Arc::new(Model::new(Vec::new()));
	let mut scene = Scene::default();
	let left_robot = scene
		.spawn_keyed_model(
			EntityKey::new("left").unwrap(),
			"Left Robot",
			"props/robot.glb",
			ModelInstance::new(
				Arc::clone(&model),
				Transform {
					position: [-0.8, 0.0, 0.0],
					scale: [0.7; 3],
					..Transform::default()
				},
			),
		)
		.unwrap();
	let right_robot = scene
		.spawn_keyed_model(
			EntityKey::new("right").unwrap(),
			"Right Robot",
			"props/robot.glb",
			ModelInstance::new(
				model,
				Transform {
					position: [0.8, 0.0, 0.0],
					scale: [0.7; 3],
					..Transform::default()
				},
			),
		)
		.unwrap();
	scene.set_binding("robot_a", left_robot).unwrap();
	scene.set_binding("robot_b", right_robot).unwrap();
	let game = RobotGame::from_scene(&scene).unwrap();
	(scene, game)
}

fn transforms(scene: &Scene) -> Vec<([f32; 3], [f32; 3], [f32; 3])> {
	scene
		.instances()
		.map(|instance| {
			let transform = instance.transform();
			(transform.position, transform.rotation, transform.scale)
		})
		.collect()
}

fn advance_half_second(scene: &mut Scene, game: &mut RobotGame, mut input: InputState) {
	// Keep each frame within the shared fixed-step catch-up budget.
	for _ in 0..5 {
		game.update(scene, &input, 0.1).unwrap();
		input.end_frame();
	}
}

#[test]
fn game_app_updates_existing_robots_without_gpu_resources_or_respawning() {
	let (scene, game) = scene_with_robots();
	let (left_id, right_id) = (game.left_robot, game.right_robot);
	let mut app = crate::GameApp::default();
	app.scene = scene;
	app.editor = runtime::SceneEditor::from_scene(&app.scene);
	let data = app.scene.to_data(&app.camera).unwrap();
	let mut project =
		runtime::project::ProjectData::for_scene("Configured QA game", "scenes/robots.json", &data)
			.unwrap();
	for behavior in [
		runtime::project::BehaviorSpec::ControlledMove {
			entity: app.scene.key(left_id).unwrap().clone(),
			speed: 1.0,
			space: Default::default(),
		},
		runtime::project::BehaviorSpec::Spin {
			entity: app.scene.key(left_id).unwrap().clone(),
			angular_velocity: [0.6, 0.9, 0.0],
		},
		runtime::project::BehaviorSpec::Spin {
			entity: app.scene.key(right_id).unwrap().clone(),
			angular_velocity: [-0.5, 0.75, 0.2],
		},
	] {
		project.add_behavior(behavior);
	}
	app.editor.set_initial_project(project).unwrap();
	app.start_play().unwrap();
	app.input = held_input(&[Key::W, Key::ArrowRight]);
	assert!(app.renderer.is_none());
	assert!(app.asset_manager.is_none());
	assert!(app.window.is_none());

	for delta_seconds in [0.1; 6] {
		app.update(delta_seconds);
		app.input.end_frame();
		assert_eq!(app.scene.instances().count(), 2);
		assert!(app.editor.is_playing());
		assert!(app.scene.get_mut(left_id).is_some());
		assert!(app.scene.get_mut(right_id).is_some());
	}

	let left = app.scene.get_mut(left_id).unwrap().transform();
	assert_vector(left.position, [-0.8, 0.6, 0.0]);
	assert_vector(left.rotation, [0.36, 0.54, 0.0]);
	assert_vector(left.scale, [0.7; 3]);
	let right = app.scene.get_mut(right_id).unwrap().transform();
	assert_vector(right.position, [0.8, 0.0, 0.0]);
	assert_vector(right.rotation, [std::f32::consts::TAU - 0.3, 0.45, 0.12]);
	assert_vector(right.scale, [0.7; 3]);
	assert_vector(app.camera.position, [1.2, 0.0, 3.0]);
}

#[test]
fn wasd_moves_only_the_left_robot_on_world_xy() {
	for (key, expected) in [
		(Key::W, [-0.8, 0.5, 0.0]),
		(Key::S, [-0.8, -0.5, 0.0]),
		(Key::A, [-1.3, 0.0, 0.0]),
		(Key::D, [-0.3, 0.0, 0.0]),
	] {
		let (mut scene, mut game) = scene_with_robots();
		advance_half_second(&mut scene, &mut game, held_input(&[key]));
		assert_vector(
			scene.get_mut(game.left_robot).unwrap().transform().position,
			expected,
		);
		assert_vector(
			scene
				.get_mut(game.right_robot)
				.unwrap()
				.transform()
				.position,
			[0.8, 0.0, 0.0],
		);
	}
}

#[test]
fn opposite_wasd_keys_cancel_independently() {
	for (keys, expected) in [
		(vec![Key::W, Key::S], [-0.8, 0.0, 0.0]),
		(vec![Key::A, Key::D], [-0.8, 0.0, 0.0]),
		(vec![Key::W, Key::S, Key::A, Key::D], [-0.8, 0.0, 0.0]),
		(vec![Key::W, Key::S, Key::D], [-0.3, 0.0, 0.0]),
	] {
		let (mut scene, mut game) = scene_with_robots();
		advance_half_second(&mut scene, &mut game, held_input(&keys));
		assert_vector(
			scene.get_mut(game.left_robot).unwrap().transform().position,
			expected,
		);
	}
}

#[test]
fn diagonal_wasd_movement_preserves_one_unit_per_second() {
	let component = 0.5 / 2.0_f32.sqrt();
	for (horizontal, x) in [(Key::A, -component), (Key::D, component)] {
		for (vertical, y) in [(Key::S, -component), (Key::W, component)] {
			let (mut scene, mut game) = scene_with_robots();
			advance_half_second(&mut scene, &mut game, held_input(&[horizontal, vertical]));
			let position = scene.get_mut(game.left_robot).unwrap().transform().position;
			assert_vector(position, [-0.8 + x, y, 0.0]);
			assert_close(
				((position[0] + 0.8).powi(2) + position[1].powi(2)).sqrt(),
				0.5,
			);
		}
	}
}

#[test]
fn updates_add_existing_rotation_and_partition_elapsed_time_consistently() {
	let (mut whole_scene, mut whole_game) = scene_with_robots();
	let (mut split_scene, mut split_game) = scene_with_robots();
	for (scene, game) in [
		(&mut whole_scene, &whole_game),
		(&mut split_scene, &split_game),
	] {
		scene
			.get_mut(game.left_robot)
			.unwrap()
			.transform_mut()
			.rotation = [0.2, 0.5, 0.3];
		scene
			.get_mut(game.right_robot)
			.unwrap()
			.transform_mut()
			.rotation = [-0.2, 0.3, -0.4];
	}
	whole_game = RobotGame::from_scene(&whole_scene).unwrap();
	split_game = RobotGame::from_scene(&split_scene).unwrap();
	let mut input = held_input(&[Key::W, Key::D]);
	whole_game.update(&mut whole_scene, &input, 0.1).unwrap();
	for delta_seconds in [0.025; 4] {
		split_game
			.update(&mut split_scene, &input, delta_seconds)
			.unwrap();
		input.end_frame();
	}

	let left = whole_scene
		.get_mut(whole_game.left_robot)
		.unwrap()
		.transform();
	assert_vector(left.rotation, [0.26, 0.59, 0.3]);
	let right = whole_scene
		.get_mut(whole_game.right_robot)
		.unwrap()
		.transform();
	assert_vector(
		right.rotation,
		[-0.25_f32, 0.375, -0.38].map(|angle| angle.rem_euclid(std::f32::consts::TAU)),
	);
	for (whole, split) in transforms(&whole_scene)
		.into_iter()
		.zip(transforms(&split_scene))
	{
		assert_vector(split.0, whole.0);
		assert_vector(split.1, whole.1);
		assert_vector(split.2, whole.2);
	}

	let before = transforms(&split_scene);
	split_game.update(&mut split_scene, &input, 0.0).unwrap();
	assert_eq!(transforms(&split_scene), before);
}

#[test]
fn camera_keys_unrelated_keys_and_released_wasd_do_not_move_robots() {
	let mut released_w = held_input(&[Key::W]);
	released_w.release(Key::W);
	for input in [
		InputState::default(),
		held_input(&[
			Key::ArrowUp,
			Key::ArrowDown,
			Key::ArrowLeft,
			Key::ArrowRight,
			Key::Space,
			Key::Escape,
		]),
		released_w,
	] {
		let (mut scene, mut game) = scene_with_robots();
		game.update(&mut scene, &input, 0.5).unwrap();
		assert_vector(
			scene.get_mut(game.left_robot).unwrap().transform().position,
			[-0.8, 0.0, 0.0],
		);
		assert_vector(
			scene
				.get_mut(game.right_robot)
				.unwrap()
				.transform()
				.position,
			[0.8, 0.0, 0.0],
		);
	}
}

#[test]
fn legacy_adapter_rejects_external_topology_changes_without_resurrecting_entities() {
	for (delete_left, delete_right) in [(true, false), (false, true), (true, true)] {
		let (mut scene, mut game) = scene_with_robots();
		if delete_left {
			scene.remove(game.left_robot).unwrap();
		}
		if delete_right {
			scene.remove(game.right_robot).unwrap();
		}
		let before = transforms(&scene);
		assert!(
			game.update(&mut scene, &held_input(&[Key::W]), 0.1)
				.is_err()
		);
		assert_eq!(transforms(&scene), before);
		assert_eq!(
			scene.instances().count(),
			usize::from(!delete_left) + usize::from(!delete_right)
		);
		if !delete_left {
			let left = scene.get_mut(game.left_robot).unwrap().transform();
			assert_vector(left.position, [-0.8, 0.0, 0.0]);
			assert_vector(left.rotation, [0.0; 3]);
		} else {
			assert!(scene.get_mut(game.left_robot).is_none());
		}
		if !delete_right {
			let right = scene.get_mut(game.right_robot).unwrap().transform();
			assert_vector(right.position, [0.8, 0.0, 0.0]);
			assert_vector(right.rotation, [0.0; 3]);
		} else {
			assert!(scene.get_mut(game.right_robot).is_none());
		}
	}
}

#[test]
fn project_display_name_does_not_disable_development_controls() {
	let (scene, _) = scene_with_robots();
	let mut app = crate::GameApp::default();
	app.scene = scene;
	app.editor = runtime::SceneEditor::from_scene(&app.scene);
	app.input = held_input(&[Key::W, Key::ArrowRight]);
	app.project_name = Some("test project".to_owned());
	app.update(0.5);
	assert_vector(
		app.scene.instances().next().unwrap().transform().position,
		[-0.8, 0.5, 0.0],
	);
	assert_vector(app.camera.position, [1.0, 0.0, 3.0]);
}

#[test]
fn binding_requires_two_distinct_entities_and_uses_named_roles() {
	let (mut scene, original) = scene_with_robots();
	assert!(RobotGame::from_scene(&Scene::default()).is_err());
	scene.set_binding("robot_b", original.left_robot).unwrap();
	assert!(RobotGame::from_scene(&scene).is_err());
	scene.set_binding("robot_a", original.right_robot).unwrap();
	let extra = scene
		.spawn_persistent_model(
			"Other robot",
			"props/robot.glb",
			ModelInstance::new(Arc::new(Model::new(Vec::new())), Transform::default()),
		)
		.unwrap();
	let mut game = RobotGame::from_scene(&scene).unwrap();
	assert_eq!(game.left_robot, original.right_robot);
	assert_eq!(game.right_robot, original.left_robot);
	advance_half_second(&mut scene, &mut game, held_input(&[Key::W]));
	assert_vector(
		scene
			.get_mut(original.right_robot)
			.unwrap()
			.transform()
			.position,
		[0.8, 0.5, 0.0],
	);
	assert_vector(
		scene
			.get_mut(original.left_robot)
			.unwrap()
			.transform()
			.position,
		[-0.8, 0.0, 0.0],
	);
	assert_vector(scene.get_mut(extra).unwrap().transform().rotation, [0.0; 3]);
}

#[test]
fn space_pauses_simulation_and_movement_until_a_fresh_press_resumes() {
	let (mut scene, mut game) = scene_with_robots();
	let before = transforms(&scene);
	let mut input = held_input(&[Key::Space, Key::W]);
	game.update(&mut scene, &input, 0.5).unwrap();
	assert!(game.animation_paused());
	assert_eq!(transforms(&scene), before);
	input.end_frame();
	input.press(Key::Space);
	game.update(&mut scene, &input, 1.0).unwrap();
	assert_eq!(transforms(&scene), before);
	input.release(Key::Space);
	input.end_frame();
	input.press(Key::Space);
	advance_half_second(&mut scene, &mut game, input);
	assert!(!game.animation_paused());
	assert_vector(
		scene.get(game.left_robot).unwrap().transform().position,
		[-0.8, 0.5, 0.0],
	);
	assert_vector(
		scene.get(game.right_robot).unwrap().transform().rotation,
		[std::f32::consts::TAU - 0.25, 0.375, 0.1],
	);
}

#[test]
fn edit_only_shortcuts_do_not_edit_transform_or_create_entities_during_simulation() {
	let (mut scene, mut game) = scene_with_robots();
	let input = held_input(&[
		Key::Q,
		Key::E,
		Key::R,
		Key::F,
		Key::Tab,
		Key::N,
		Key::C,
		Key::Delete,
		Key::ControlLeft,
		Key::Z,
		Key::Y,
	]);
	advance_half_second(&mut scene, &mut game, input);
	assert_eq!(scene.instances().count(), 2);
	assert_vector(
		scene.get(game.left_robot).unwrap().transform().position,
		[-0.8, 0.0, 0.0],
	);
	assert_vector(
		scene.get(game.left_robot).unwrap().transform().rotation,
		[0.3, 0.45, 0.0],
	);
	assert_vector(
		scene.get(game.left_robot).unwrap().transform().scale,
		[0.7; 3],
	);
}

#[test]
fn nonfinite_simulation_delta_is_rejected_before_mutating_a_robot() {
	let (mut scene, mut game) = scene_with_robots();
	let before = transforms(&scene);
	assert!(
		game.update(&mut scene, &held_input(&[Key::W]), f32::INFINITY)
			.is_err()
	);
	assert_eq!(transforms(&scene), before);
}
