use super::*;
use crate::{Camera, InputState, Key};
use std::f32::consts::{FRAC_PI_2, PI, TAU};

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

#[test]
fn defaults_preserve_existing_control_settings() {
	let controller = CameraController::default();
	assert_eq!(controller.move_speed, 2.0);
	assert_eq!(controller.mouse_sensitivity, 0.002);
}

#[test]
fn no_held_arrows_leave_the_camera_unchanged() {
	let controller = CameraController::default();
	let mut released_arrow = held_input(&[Key::ArrowUp]);
	released_arrow.release(Key::ArrowUp);

	for input in [
		InputState::default(),
		held_input(&[Key::W, Key::A, Key::S, Key::D, Key::Space, Key::Escape]),
		released_arrow,
	] {
		let mut camera = Camera {
			position: [4.0, -2.0, 7.0],
			yaw: 0.3,
			pitch: -0.2,
			vertical_fov: 0.8,
			near: 0.2,
			far: 250.0,
		};
		controller.update(&mut camera, &input, 0.5);
		assert_vector(camera.position, [4.0, -2.0, 7.0]);
		assert_eq!((camera.yaw, camera.pitch), (0.3, -0.2));
		assert_eq!(
			(camera.vertical_fov, camera.near, camera.far),
			(0.8, 0.2, 250.0)
		);
	}
}

#[test]
fn each_arrow_moves_in_its_expected_direction_without_rotating() {
	let controller = CameraController::default();
	for (key, expected) in [
		(Key::ArrowUp, [0.0, 0.0, 2.0]),
		(Key::ArrowDown, [0.0, 0.0, 4.0]),
		(Key::ArrowLeft, [-1.0, 0.0, 3.0]),
		(Key::ArrowRight, [1.0, 0.0, 3.0]),
	] {
		let mut camera = Camera::default();
		let projection = (camera.vertical_fov, camera.near, camera.far);
		controller.update(&mut camera, &held_input(&[key]), 0.5);
		assert_vector(camera.position, expected);
		assert_eq!((camera.yaw, camera.pitch), (0.0, 0.0));
		assert_eq!((camera.vertical_fov, camera.near, camera.far), projection);
	}
}

#[test]
fn opposite_arrows_cancel_independently_on_each_axis() {
	let controller = CameraController::default();
	for (keys, expected) in [
		(vec![Key::ArrowUp, Key::ArrowDown], [0.0, 0.0, 3.0]),
		(vec![Key::ArrowLeft, Key::ArrowRight], [0.0, 0.0, 3.0]),
		(
			vec![
				Key::ArrowUp,
				Key::ArrowDown,
				Key::ArrowLeft,
				Key::ArrowRight,
			],
			[0.0, 0.0, 3.0],
		),
		(
			vec![Key::ArrowUp, Key::ArrowDown, Key::ArrowRight],
			[1.0, 0.0, 3.0],
		),
	] {
		let mut camera = Camera::default();
		controller.update(&mut camera, &held_input(&keys), 0.5);
		assert_vector(camera.position, expected);
	}
}

#[test]
fn diagonals_have_the_same_total_speed_as_single_arrows() {
	let controller = CameraController::default();
	let component = 1.0 / 2.0_f32.sqrt();
	for (horizontal, x) in [(Key::ArrowLeft, -component), (Key::ArrowRight, component)] {
		for (vertical, z) in [(Key::ArrowUp, -component), (Key::ArrowDown, component)] {
			let mut camera = Camera::default();
			controller.update(&mut camera, &held_input(&[horizontal, vertical]), 0.5);
			assert_vector(camera.position, [x, 0.0, 3.0 + z]);
			let distance = (camera.position[0].powi(2)
				+ camera.position[1].powi(2)
				+ (camera.position[2] - 3.0).powi(2))
			.sqrt();
			assert_close(distance, 1.0);
		}
	}
}

#[test]
fn movement_follows_yaw_and_pitch_while_strafing_stays_horizontal() {
	let controller = CameraController::default();
	for (key, expected) in [
		(Key::ArrowUp, [3.0_f32.sqrt(), 1.0, 0.0]),
		(Key::ArrowDown, [-3.0_f32.sqrt(), -1.0, 0.0]),
		(Key::ArrowRight, [0.0, 0.0, 2.0]),
		(Key::ArrowLeft, [0.0, 0.0, -2.0]),
	] {
		let mut camera = Camera {
			position: [0.0; 3],
			yaw: FRAC_PI_2,
			pitch: PI / 6.0,
			..Camera::default()
		};
		controller.update(&mut camera, &held_input(&[key]), 1.0);
		assert_vector(camera.position, expected);
		assert_eq!((camera.yaw, camera.pitch), (FRAC_PI_2, PI / 6.0));
	}
}

#[test]
fn held_movement_scales_with_elapsed_time_across_frames() {
	let controller = CameraController::default();
	let mut input = held_input(&[Key::ArrowUp, Key::ArrowRight]);
	let mut whole = Camera::default();
	let mut partitioned = Camera::default();

	controller.update(&mut whole, &input, 0.75);
	for delta_seconds in [0.1, 0.15, 0.5] {
		controller.update(&mut partitioned, &input, delta_seconds);
		input.end_frame();
	}
	assert_vector(partitioned.position, whole.position);
	assert!(input.is_held(Key::ArrowUp));
	assert!(input.is_held(Key::ArrowRight));

	let before = partitioned.position;
	controller.update(&mut partitioned, &input, 0.0);
	assert_vector(partitioned.position, before);
}

#[test]
fn configured_speed_controls_distance_and_can_disable_movement() {
	for (move_speed, expected) in [(6.0, [4.0, 2.0, 3.0]), (0.0, [1.0, 2.0, 3.0])] {
		let controller = CameraController {
			move_speed,
			..CameraController::default()
		};
		let mut camera = Camera {
			position: [1.0, 2.0, 3.0],
			..Camera::default()
		};
		controller.update(&mut camera, &held_input(&[Key::ArrowRight]), 0.5);
		assert_vector(camera.position, expected);
	}
}

#[test]
fn mouse_motion_uses_configured_sensitivity_and_inverts_vertical_delta() {
	let controller = CameraController {
		mouse_sensitivity: 0.01,
		..CameraController::default()
	};
	for (dx, dy, expected_yaw, expected_pitch) in
		[(20.0, 10.0, 0.2, -0.1), (-20.0, -10.0, TAU - 0.2, 0.1)]
	{
		let mut camera = Camera::default();
		controller.mouse_motion(&mut camera, dx, dy);
		assert_close(camera.yaw, expected_yaw);
		assert_close(camera.pitch, expected_pitch);
	}
}

#[test]
fn mouse_events_accumulate_without_changing_position_or_projection() {
	let controller = CameraController::default();
	let mut camera = Camera {
		position: [4.0, 5.0, 6.0],
		vertical_fov: 0.8,
		near: 0.2,
		far: 250.0,
		..Camera::default()
	};
	controller.mouse_motion(&mut camera, 100.0, -50.0);
	controller.mouse_motion(&mut camera, 50.0, 25.0);
	controller.mouse_motion(&mut camera, 0.0, 0.0);
	assert_close(camera.yaw, 0.3);
	assert_close(camera.pitch, 0.05);
	assert_vector(camera.position, [4.0, 5.0, 6.0]);
	assert_eq!(
		(camera.vertical_fov, camera.near, camera.far),
		(0.8, 0.2, 250.0)
	);
}

#[test]
fn mouse_motion_preserves_camera_yaw_wrapping_and_pitch_limits() {
	let controller = CameraController {
		mouse_sensitivity: 1.0,
		..CameraController::default()
	};
	let limit = 89.0_f32.to_radians();
	for sign in [-1.0_f32, 1.0] {
		let mut camera = Camera::default();
		controller.mouse_motion(
			&mut camera,
			(sign * (2.0 * TAU + 0.25)) as f64,
			(-sign * PI) as f64,
		);
		assert_close(camera.yaw, if sign > 0.0 { 0.25 } else { TAU - 0.25 });
		assert!((0.0..TAU).contains(&camera.yaw));
		assert_close(camera.pitch, sign * limit);

		controller.mouse_motion(&mut camera, 0.0, (sign * 0.25) as f64);
		assert_close(camera.pitch, sign * (limit - 0.25));
	}
}
