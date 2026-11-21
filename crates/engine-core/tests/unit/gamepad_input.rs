use super::GamepadSample;
use crate::input::{GameInputBinding, InputProfile, InputState, Key};
use std::collections::HashSet;

#[test]
fn radial_dead_zone_filters_drift_without_discarding_partial_speed() {
	let drift = GamepadSample {
		left_x: 4000,
		left_y: -4000,
		..GamepadSample::default()
	};
	assert_eq!(drift.movement(), [0.0; 2]);
	assert!(drift.is_neutral());
	assert!(
		!GamepadSample {
			jump: true,
			..drift
		}
			.is_neutral()
	);

	let just_outside = GamepadSample {
		left_x: 6554,
		..GamepadSample::default()
	};
	assert!(just_outside.movement()[0] > 0.0);
	let half = GamepadSample {
		left_x: 16384,
		..GamepadSample::default()
	};
	assert!((half.movement()[0] - 0.375).abs() < 0.0001);
	assert_eq!(half.movement()[1], 0.0);
}

#[test]
fn signed_extents_and_diagonals_share_the_same_maximum_speed() {
	for (axis, direction) in [(i16::MIN, -1.0), (i16::MAX, 1.0)] {
		let horizontal = GamepadSample {
			left_x: axis,
			..GamepadSample::default()
		};
		assert_eq!(horizontal.movement(), [direction, 0.0]);
		let [x, y] = GamepadSample {
			left_y: axis,
			..horizontal
		}
			.movement();
		assert!((x.hypot(y) - 1.0).abs() <= f32::EPSILON);
		assert_eq!(x, y);
		assert_eq!(x.signum(), direction);
	}
}

#[test]
fn keyboard_precedence_and_authored_actions_are_shared_by_both_resolvers() {
	let profile = InputProfile {
		actions: vec![GameInputBinding {
			id: "interact".into(),
			key: Key::E,
			animation_parameter: Some("Interact".into()),
		}],
		..InputProfile::default()
	};
	let controller = GamepadSample {
		left_x: i16::MIN,
		jump: true,
		interact: true,
		..GamepadSample::default()
	};
	let mut keyboard = InputState::default();
	keyboard.press(Key::D);
	keyboard.press(Key::E);
	let input = profile.resolve_controls(&keyboard, controller);
	let code_input = profile
		.resolve_controls_from_codes(&HashSet::from(["KeyD".into(), "KeyE".into()]), controller);
	assert_eq!(input.movement, [1.0, 0.0]);
	assert_eq!(input.movement, code_input.movement);
	assert_eq!(input.actions.ids, HashSet::from(["interact".into()]));
	assert_eq!(input.actions.ids, code_input.actions.ids);
	assert_eq!(
		input.actions.animation_parameters,
		HashSet::from(["Interact".into()])
	);
	assert_eq!(
		input.actions.animation_parameters,
		code_input.actions.animation_parameters
	);

	keyboard.press(Key::A);
	let canceled = profile.resolve_controls(&keyboard, controller);
	assert_eq!(canceled.movement, [-1.0, 0.0]);
	keyboard.reset();
	assert!(profile.resolve_keyboard(&keyboard).actions.ids.is_empty());
}
