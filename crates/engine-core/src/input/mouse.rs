use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MouseButton {
	Left,
	Right,
	Middle,
	Back,
	Forward,
	Other(u16),
}

impl MouseButton {
	/// Returns the physical code used by native gameplay input and replay.
	pub fn input_code(self) -> Option<&'static str> {
		match self {
			Self::Left => Some("MouseLeft"),
			Self::Right => Some("MouseRight"),
			Self::Middle => Some("MouseMiddle"),
			Self::Back => Some("MouseBack"),
			Self::Forward => Some("MouseForward"),
			Self::Other(_) => None,
		}
	}
}
