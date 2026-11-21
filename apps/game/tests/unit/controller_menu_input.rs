use super::*;
use runtime::input::MenuButtons;

#[test]
fn a_held_menu_button_requires_release_and_a_fresh_press() {
	let mut input = ControllerMenuInput::default();
	let released = PhysicalGamepadSample::default();
	let menu = PhysicalGamepadSample {
		menu: MenuButtons {
			open: true,
			..MenuButtons::default()
		},
		..released
	};
	assert_eq!(input.update(menu, false), None);
	assert_eq!(input.update(released, false), None);
	assert_eq!(input.update(menu, false), Some(MenuIntent::Open));
	assert_eq!(input.update(menu, false), None);
	assert_eq!(input.update(menu, true), None);
}

#[test]
fn opening_with_a_held_confirm_and_reset_while_disconnected_are_safe() {
	let mut input = ControllerMenuInput::default();
	let released = PhysicalGamepadSample::default();
	let pressed_a = PhysicalGamepadSample {
		face_buttons: [true, false, false, false],
		..released
	};
	let open_with_a = PhysicalGamepadSample {
		menu: MenuButtons {
			open: true,
			..MenuButtons::default()
		},
		..pressed_a
	};
	input.update(released, false);
	assert_eq!(input.update(open_with_a, false), Some(MenuIntent::Open));
	assert_eq!(input.update(pressed_a, true), None);
	input.reset();
	assert_eq!(input.update(pressed_a, true), None);
	assert_eq!(input.update(released, true), None);
	assert_eq!(input.update(pressed_a, true), Some(MenuIntent::Confirm));
}

#[test]
fn back_takes_precedence_and_opposite_directions_do_not_navigate() {
	let mut input = ControllerMenuInput::default();
	let released = PhysicalGamepadSample::default();
	let opposite = PhysicalGamepadSample {
		menu: MenuButtons {
			up: true,
			down: true,
			left: true,
			right: true,
			..MenuButtons::default()
		},
		..released
	};
	input.update(released, true);
	assert_eq!(input.update(opposite, true), None);
	let simultaneous = PhysicalGamepadSample {
		face_buttons: [true, true, false, false],
		..released
	};
	assert_eq!(input.update(simultaneous, true), Some(MenuIntent::Back));
	assert_eq!(input.update(simultaneous, true), None);
	assert_eq!(MenuIntent::Back.key(), Key::Escape);
}
