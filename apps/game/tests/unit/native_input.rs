use super::*;

#[test]
fn maps_keyboard_groups_without_aliasing_physical_keys() {
	for (native, key) in [
		(KeyCode::KeyW, Key::W),
		(KeyCode::KeyB, Key::B),
		(KeyCode::KeyX, Key::X),
		(KeyCode::Digit0, Key::Digit0),
		(KeyCode::Numpad0, Key::Numpad0),
		(KeyCode::Enter, Key::Enter),
		(KeyCode::NumpadEnter, Key::NumpadEnter),
		(KeyCode::ShiftRight, Key::ShiftRight),
		(KeyCode::SuperLeft, Key::SuperLeft),
		(KeyCode::IntlYen, Key::IntlYen),
		(KeyCode::Lang5, Key::Lang5),
		(KeyCode::BracketLeft, Key::BracketLeft),
		(KeyCode::Home, Key::Home),
		(KeyCode::PrintScreen, Key::PrintScreen),
		(KeyCode::MediaPlayPause, Key::MediaPlayPause),
		(KeyCode::AudioVolumeMute, Key::AudioVolumeMute),
		(KeyCode::BrowserBack, Key::BrowserBack),
		(KeyCode::F35, Key::F35),
	] {
		assert_eq!(keyboard_key(PhysicalKey::Code(native)), Some(key));
	}
}

#[test]
fn preserves_unidentified_platform_scancodes() {
	for (native, code) in [
		(
			WindowNativeKeyCode::Unidentified,
			NativeKeyCode::Unidentified,
		),
		(
			WindowNativeKeyCode::Android(501),
			NativeKeyCode::Android(501),
		),
		(WindowNativeKeyCode::MacOS(255), NativeKeyCode::MacOS(255)),
		(
			WindowNativeKeyCode::Windows(0xe01d),
			NativeKeyCode::Windows(0xe01d),
		),
		(WindowNativeKeyCode::Xkb(777), NativeKeyCode::Xkb(777)),
	] {
		assert_eq!(
			keyboard_key(PhysicalKey::Unidentified(native)),
			Some(Key::Unidentified(code))
		);
	}
}

#[test]
fn maps_all_mouse_buttons_and_preserves_extra_button_ids() {
	for (native, button) in [
		(WindowMouseButton::Left, MouseButton::Left),
		(WindowMouseButton::Right, MouseButton::Right),
		(WindowMouseButton::Middle, MouseButton::Middle),
		(WindowMouseButton::Back, MouseButton::Back),
		(WindowMouseButton::Forward, MouseButton::Forward),
		(WindowMouseButton::Other(0), MouseButton::Other(0)),
		(
			WindowMouseButton::Other(u16::MAX),
			MouseButton::Other(u16::MAX),
		),
	] {
		assert_eq!(mouse_button(native), button);
	}
}
