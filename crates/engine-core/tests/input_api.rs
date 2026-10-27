use engine_core::input::MovementBindings;
use engine_core::{InputButton, InputState, Key, MouseButton, NativeKeyCode};
use std::collections::HashSet;

#[test]
fn existing_key_names_remain_compatible_with_saved_bindings() {
	for (key, name) in [
		(Key::W, "w"),
		(Key::A, "a"),
		(Key::S, "s"),
		(Key::D, "d"),
		(Key::Q, "q"),
		(Key::E, "e"),
		(Key::R, "r"),
		(Key::F, "f"),
		(Key::N, "n"),
		(Key::C, "c"),
		(Key::Z, "z"),
		(Key::Y, "y"),
		(Key::ControlLeft, "control_left"),
		(Key::ControlRight, "control_right"),
		(Key::Delete, "delete"),
		(Key::Space, "space"),
		(Key::Escape, "escape"),
		(Key::ArrowUp, "arrow_up"),
		(Key::ArrowDown, "arrow_down"),
		(Key::ArrowLeft, "arrow_left"),
		(Key::ArrowRight, "arrow_right"),
		(Key::Tab, "tab"),
		(Key::F5, "f5"),
		(Key::F6, "f6"),
		(Key::F9, "f9"),
	] {
		let json = serde_json::json!(name);
		assert_eq!(serde_json::to_value(key).unwrap(), json);
		assert_eq!(serde_json::from_value::<Key>(json).unwrap(), key);
	}
}

#[test]
fn named_key_catalogue_is_unique_and_serializable() {
	let keys = Key::ALL.iter().copied().collect::<HashSet<_>>();
	assert_eq!(keys.len(), 194);
	assert_eq!(keys.len(), Key::ALL.len());
	for key in Key::ALL {
		let json = serde_json::to_value(key).unwrap();
		assert!(json.is_string(), "named keys must retain string encoding");
		assert_eq!(serde_json::from_value::<Key>(json).unwrap(), *key);
	}
	for (key, name) in [
		(Key::B, "b"),
		(Key::Digit0, "digit0"),
		(Key::Backquote, "backquote"),
		(Key::ShiftRight, "shift_right"),
		(Key::IntlYen, "intl_yen"),
		(Key::Lang5, "lang5"),
		(Key::NumpadEnter, "numpad_enter"),
		(Key::BrowserBack, "browser_back"),
		(Key::MediaPlayPause, "media_play_pause"),
		(Key::Power, "power"),
		(Key::F35, "f35"),
	] {
		assert!(keys.contains(&key));
		assert_eq!(serde_json::to_value(key).unwrap(), serde_json::json!(name));
	}
}

#[test]
fn native_scan_codes_round_trip_without_losing_platform_or_width() {
	let codes = [
		NativeKeyCode::Unidentified,
		NativeKeyCode::Android(u32::MAX),
		NativeKeyCode::MacOS(u16::MAX),
		NativeKeyCode::Windows(u16::MAX),
		NativeKeyCode::Xkb(u32::MAX),
	];
	let mut input = InputState::default();
	for code in codes {
		let key = Key::Unidentified(code);
		let json = serde_json::to_value(key).unwrap();
		assert_eq!(serde_json::from_value::<Key>(json).unwrap(), key);
		let button = InputButton::from(key);
		let json = serde_json::to_value(button).unwrap();
		assert_eq!(serde_json::from_value::<InputButton>(json).unwrap(), button);
		input.press(key);
	}
	input.release(Key::Unidentified(NativeKeyCode::Windows(u16::MAX)));
	assert!(!input.is_held(Key::Unidentified(NativeKeyCode::Windows(u16::MAX))));
	assert!(input.is_held(Key::Unidentified(NativeKeyCode::MacOS(u16::MAX))));
	assert!(input.is_held(Key::Unidentified(NativeKeyCode::Android(u32::MAX))));
	assert!(input.is_held(Key::Unidentified(NativeKeyCode::Xkb(u32::MAX))));
	assert_eq!(
		serde_json::to_value(NativeKeyCode::MacOS(7)).unwrap(),
		serde_json::json!({"mac_os": 7})
	);
}

#[test]
fn mouse_buttons_round_trip_and_convert_to_distinct_input_buttons() {
	for button in [
		MouseButton::Left,
		MouseButton::Right,
		MouseButton::Middle,
		MouseButton::Back,
		MouseButton::Forward,
		MouseButton::Other(u16::MAX),
	] {
		let json = serde_json::to_value(button).unwrap();
		assert_eq!(serde_json::from_value::<MouseButton>(json).unwrap(), button);
		let input_button = InputButton::from(button);
		assert_eq!(input_button, InputButton::Mouse(button));
		let json = serde_json::to_value(input_button).unwrap();
		assert_eq!(
			serde_json::from_value::<InputButton>(json).unwrap(),
			input_button
		);
	}
	assert_ne!(InputButton::from(Key::ArrowLeft), MouseButton::Left.into());
	assert_eq!(
		serde_json::to_value(InputButton::from(MouseButton::Left)).unwrap(),
		serde_json::json!({"mouse": "left"})
	);
}

#[test]
fn key_and_mouse_edges_are_independent_and_repeats_do_not_retrigger() {
	let mut input = InputState::default();
	input.press(Key::ArrowLeft);
	input.press(MouseButton::Left);
	assert!(input.is_held(Key::ArrowLeft));
	assert!(input.is_held(MouseButton::Left));
	assert!(input.just_pressed(Key::ArrowLeft));
	assert!(input.just_pressed(MouseButton::Left));
	input.end_frame();
	input.press(Key::ArrowLeft);
	input.press(MouseButton::Left);
	assert!(!input.just_pressed(Key::ArrowLeft));
	assert!(!input.just_pressed(MouseButton::Left));

	input.release(MouseButton::Left);
	assert!(input.just_released(MouseButton::Left));
	assert!(!input.is_held(MouseButton::Left));
	assert!(input.is_held(Key::ArrowLeft));
	assert!(!input.just_released(Key::ArrowLeft));
	input.end_frame();
	input.release(MouseButton::Left);
	assert!(!input.just_released(MouseButton::Left));
	input.release(Key::ArrowLeft);
	assert!(input.just_released(Key::ArrowLeft));

	input.press(MouseButton::Right);
	input.release(MouseButton::Right);
	assert!(input.just_pressed(MouseButton::Right));
	assert!(input.just_released(MouseButton::Right));
	assert!(!input.is_held(MouseButton::Right));
	input.end_frame();
	assert!(!input.just_pressed(MouseButton::Right));
	assert!(!input.just_released(MouseButton::Right));
}

#[test]
fn suppression_requires_each_held_button_to_be_released_before_rearming() {
	let mut input = InputState::default();
	let buttons = [InputButton::from(Key::W), MouseButton::Left.into()];
	for button in buttons {
		input.press(button);
	}
	input.suppress_held();
	input.end_frame();
	input.suppress_held();
	for button in buttons {
		input.press(button);
		assert!(!input.is_held(button));
		assert!(!input.just_pressed(button));
		assert!(!input.just_released(button));
	}
	input.release(Key::W);
	assert!(!input.just_released(Key::W));
	input.press(Key::W);
	assert!(input.just_pressed(Key::W));
	input.press(MouseButton::Left);
	assert!(!input.is_held(MouseButton::Left));
	input.release(MouseButton::Left);
	assert!(!input.just_released(MouseButton::Left));
	input.press(MouseButton::Left);
	assert!(input.just_pressed(MouseButton::Left));
}

#[test]
fn reset_clears_edges_held_buttons_and_suppression_for_both_devices() {
	let mut input = InputState::default();
	let buttons = [InputButton::from(Key::W), MouseButton::Left.into()];
	for button in buttons {
		input.press(button);
	}
	input.suppress_held();
	input.press(Key::D);
	input.press(MouseButton::Right);
	input.release(MouseButton::Right);
	input.reset();
	for button in buttons
		.into_iter()
		.chain([Key::D.into(), MouseButton::Right.into()])
	{
		assert!(!input.is_held(button));
		assert!(!input.just_pressed(button));
		assert!(!input.just_released(button));
		input.press(button);
		assert!(input.is_held(button));
		assert!(input.just_pressed(button));
	}
}

#[test]
fn keyboard_movement_bindings_ignore_mouse_buttons() {
	let mut input = InputState::default();
	input.press(MouseButton::Left);
	assert!(!MovementBindings::WASD.any_held(&input));
	assert_eq!(MovementBindings::WASD.direction(&input), [0.0, 0.0]);
	input.press(Key::W);
	assert!(MovementBindings::WASD.any_held(&input));
	assert_eq!(MovementBindings::WASD.direction(&input), [0.0, 1.0]);
}
