//! Consistent names for physical bindings in Controls and the Player HUD.

use std::borrow::Cow;

use runtime::Key;

pub(crate) fn compact_key_name(key: Key) -> Cow<'static, str> {
	match key {
		Key::Escape => "Esc".into(),
		Key::Space => "Space".into(),
		Key::Enter => "Enter".into(),
		Key::ArrowUp => "Up".into(),
		Key::ArrowDown => "Down".into(),
		Key::ArrowLeft => "Left".into(),
		Key::ArrowRight => "Right".into(),
		Key::ControlLeft => "Left Ctrl".into(),
		Key::ControlRight => "Right Ctrl".into(),
		Key::ShiftLeft => "Left Shift".into(),
		Key::ShiftRight => "Right Shift".into(),
		Key::AltLeft => "Left Alt".into(),
		Key::AltRight => "Right Alt".into(),
		named => {
			let code = named.physical_code();
			let short = code
				.strip_prefix("Key")
				.or_else(|| code.strip_prefix("Digit"))
				.filter(|text| text.len() == 1);
			match short {
				Some(text) => Cow::Owned(text.to_owned()),
				None => Cow::Owned(code),
			}
		}
	}
}
