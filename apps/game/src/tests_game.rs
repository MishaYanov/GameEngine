use super::*;
use runtime::{Transform, assets::Model, model_instance::ModelInstance};
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
    let left_robot = scene.spawn_model(
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
    );
    let right_robot = scene.spawn_model(
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
    );
    (
        scene,
        RobotGame::from_entities(&[left_robot, right_robot]).unwrap(),
    )
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

#[test]
fn game_app_updates_existing_robots_without_gpu_resources_or_respawning() {
    let (scene, game) = scene_with_robots();
    let (left_id, right_id) = (game.left_robot, game.right_robot);
    let mut app = crate::GameApp::default();
    app.scene = scene;
    app.game = Some(game);
    app.input = held_input(&[Key::W, Key::ArrowRight]);
    assert!(app.renderer.is_none());
    assert!(app.asset_manager.is_none());
    assert!(app.window.is_none());

    for delta_seconds in [0.1, 0.2, 0.3] {
        app.update(delta_seconds);
        app.input.end_frame();
        assert_eq!(app.scene.instances().count(), 2);
        let game = app.game.as_ref().expect("existing game must be retained");
        assert_eq!((game.left_robot, game.right_robot), (left_id, right_id));
        assert!(app.scene.get_mut(left_id).is_some());
        assert!(app.scene.get_mut(right_id).is_some());
    }

    let left = app.scene.get_mut(left_id).unwrap().transform();
    assert_vector(left.position, [-0.8, 0.6, 0.0]);
    assert_vector(left.rotation, [0.36, 0.54, 0.0]);
    assert_vector(left.scale, [0.7; 3]);
    let right = app.scene.get_mut(right_id).unwrap().transform();
    assert_vector(right.position, [0.8, 0.0, 0.0]);
    assert_vector(right.rotation, [-0.3, 0.45, 0.12]);
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
        game.update(&mut scene, &held_input(&[key]), 0.5);
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
        game.update(&mut scene, &held_input(&keys), 0.5);
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
            game.update(&mut scene, &held_input(&[horizontal, vertical]), 0.5);
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
    let mut input = held_input(&[Key::W, Key::D]);
    whole_game.update(&mut whole_scene, &input, 0.5);
    for delta_seconds in [0.1, 0.15, 0.25] {
        split_game.update(&mut split_scene, &input, delta_seconds);
        input.end_frame();
    }

    let left = whole_scene
        .get_mut(whole_game.left_robot)
        .unwrap()
        .transform();
    assert_vector(left.rotation, [0.5, 0.95, 0.3]);
    let right = whole_scene
        .get_mut(whole_game.right_robot)
        .unwrap()
        .transform();
    assert_vector(right.rotation, [-0.45, 0.675, -0.3]);
    for (whole, split) in transforms(&whole_scene)
        .into_iter()
        .zip(transforms(&split_scene))
    {
        assert_vector(split.0, whole.0);
        assert_vector(split.1, whole.1);
        assert_vector(split.2, whole.2);
    }

    let before = transforms(&split_scene);
    split_game.update(&mut split_scene, &input, 0.0);
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
        game.update(&mut scene, &input, 0.5);
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
fn deleted_robot_handles_do_not_stop_remaining_robot_updates() {
    for (delete_left, delete_right) in [(true, false), (false, true), (true, true)] {
        let (mut scene, mut game) = scene_with_robots();
        if delete_left {
            scene.remove(game.left_robot).unwrap();
        }
        if delete_right {
            scene.remove(game.right_robot).unwrap();
        }
        for _ in 0..2 {
            game.update(&mut scene, &held_input(&[Key::W]), 0.25);
        }
        assert_eq!(
            scene.instances().count(),
            usize::from(!delete_left) + usize::from(!delete_right)
        );
        if !delete_left {
            let left = scene.get_mut(game.left_robot).unwrap().transform();
            assert_vector(left.position, [-0.8, 0.5, 0.0]);
            assert_vector(left.rotation, [0.3, 0.45, 0.0]);
        } else {
            assert!(scene.get_mut(game.left_robot).is_none());
        }
        if !delete_right {
            let right = scene.get_mut(game.right_robot).unwrap().transform();
            assert_vector(right.position, [0.8, 0.0, 0.0]);
            assert_vector(right.rotation, [-0.25, 0.375, 0.1]);
        } else {
            assert!(scene.get_mut(game.right_robot).is_none());
        }
    }
}

#[test]
fn project_name_guard_keeps_robot_and_camera_state_unchanged() {
    let (scene, game) = scene_with_robots();
    let mut app = crate::GameApp::default();
    app.scene = scene;
    app.game = Some(game);
    app.input = held_input(&[Key::W, Key::ArrowRight]);
    app.project_name = Some("test project".to_owned());
    let before = transforms(&app.scene);
    app.update(0.5);
    assert_eq!(transforms(&app.scene), before);
    assert_vector(app.camera.position, [0.0, 0.0, 3.0]);
}

#[test]
fn binding_requires_two_distinct_entities_and_uses_source_order() {
    let (mut scene, original) = scene_with_robots();
    assert!(RobotGame::from_entities(&[]).is_err());
    assert!(RobotGame::from_entities(&[original.left_robot]).is_err());
    assert!(RobotGame::from_entities(&[original.left_robot, original.left_robot]).is_err());
    let extra = scene.spawn_named(
        "Other robot",
        ModelInstance::new(Arc::new(Model::new(Vec::new())), Transform::default()),
    );
    let mut game =
        RobotGame::from_entities(&[original.right_robot, original.left_robot, extra]).unwrap();
    assert_eq!(game.left_robot, original.right_robot);
    assert_eq!(game.right_robot, original.left_robot);
    assert_eq!(game.selected_robot, original.right_robot);
    game.update(&mut scene, &held_input(&[Key::W]), 0.5);
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
fn tab_switches_selected_movement_once_per_press_without_changing_names_or_spin() {
    let (mut scene, mut game) = scene_with_robots();
    let mut input = held_input(&[Key::W, Key::Tab]);
    game.update(&mut scene, &input, 0.1);
    assert_eq!(game.selected_robot, game.right_robot);
    input.end_frame();
    input.press(Key::Tab); // OS repeat while held must not create a new press.
    game.update(&mut scene, &input, 0.2);
    assert_eq!(game.selected_robot, game.right_robot);
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
        [0.8, 0.3, 0.0],
    );

    input.release(Key::Tab);
    input.end_frame();
    input.press(Key::Tab);
    game.update(&mut scene, &input, 0.3);
    assert_eq!(game.selected_robot, game.left_robot);
    assert_vector(
        scene.get_mut(game.left_robot).unwrap().transform().position,
        [-0.8, 0.3, 0.0],
    );
    assert_vector(
        scene
            .get_mut(game.right_robot)
            .unwrap()
            .transform()
            .position,
        [0.8, 0.3, 0.0],
    );
    assert_vector(
        scene.get_mut(game.left_robot).unwrap().transform().rotation,
        [0.36, 0.54, 0.0],
    );
    assert_vector(
        scene
            .get_mut(game.right_robot)
            .unwrap()
            .transform()
            .rotation,
        [-0.3, 0.45, 0.12],
    );
    assert_eq!(scene.name(game.left_robot), Some("Left Robot"));
    assert_eq!(scene.name(game.right_robot), Some("Right Robot"));
}

#[test]
fn tab_can_select_a_survivor_after_the_selected_robot_is_deleted() {
    let (mut scene, mut game) = scene_with_robots();
    scene.remove(game.left_robot).unwrap();
    let mut input = held_input(&[Key::W]);
    game.update(&mut scene, &input, 0.1);
    assert_vector(
        scene
            .get_mut(game.right_robot)
            .unwrap()
            .transform()
            .position,
        [0.8, 0.0, 0.0],
    );
    input.press(Key::Tab);
    game.update(&mut scene, &input, 0.1);
    assert_eq!(game.selected_robot, game.right_robot);
    assert_vector(
        scene
            .get_mut(game.right_robot)
            .unwrap()
            .transform()
            .position,
        [0.8, 0.1, 0.0],
    );
    assert_eq!(scene.name(game.left_robot), None);
}
