use super::{Key, MouseButton};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputButton {
	Key(Key),
	Mouse(MouseButton),
}

impl From<Key> for InputButton {
	fn from(key: Key) -> Self {
		Self::Key(key)
	}
}

impl From<MouseButton> for InputButton {
	fn from(button: MouseButton) -> Self {
		Self::Mouse(button)
	}
}
