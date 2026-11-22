//! Keyboard remap draft, selection, and single press capture.

use runtime::{Key, input::InputProfile};

use super::{
	PromptDevice,
	action_keys::{ActionKeyOverride, can_remap_key},
	input_labels::compact_key_name,
};

pub(super) struct KeyboardMenu {
	focus: usize,
	authored: InputProfile,
	draft: Vec<ActionKeyOverride>,
	capturing: Option<String>,
	editable: bool,
}

impl KeyboardMenu {
	pub(super) fn new(
		authored: Option<&InputProfile>,
		draft: &[ActionKeyOverride],
		editable: bool,
	) -> Self {
		Self {
			focus: 0,
			authored: authored.cloned().unwrap_or_default(),
			draft: draft.to_vec(),
			capturing: None,
			editable,
		}
	}

	pub(super) fn captures_key(&self) -> bool {
		self.capturing.is_some()
	}

	pub(super) fn cancel_capture(&mut self) -> bool {
		self.capturing.take().is_some()
	}

	pub(super) fn draft(&self) -> &[ActionKeyOverride] {
		&self.draft
	}

	pub(super) fn rows(&self, columns: usize) -> (String, Vec<String>, usize) {
		let mut rows = Vec::with_capacity(self.authored.actions.len() + 3);
		for action in &self.authored.actions {
			if self.capturing.as_deref() == Some(action.id.as_str()) {
				rows.push(if columns >= 13 { "Press a key" } else { "Key?" }.into());
				continue;
			}
			let key = self
				.draft
				.iter()
				.find(|row| row.action_id == action.id)
				.map_or(action.key, |row| row.key);
			let name = compact_key_name(key);
			let width = columns.saturating_sub(2);
			let label_width = width.saturating_sub(name.len() + 2);
			rows.push(if label_width < action.id.len().min(4) {
				name.into_owned()
			} else {
				let end = action.id.len().min(label_width);
				let label = action.id.get(..end).unwrap_or(&action.id);
				format!("{label}: {name}")
			});
		}
		rows.push(
			if self.editable {
				"Apply"
			} else {
				"Apply disabled"
			}
				.into(),
		);
		rows.push("Restore defaults".into());
		rows.push("Back".into());
		("Keyboard".into(), rows, self.focus)
	}

	pub(super) fn key(&mut self, key: Key, device: PromptDevice) -> KeyboardAction {
		if self.captures_key() {
			if key == Key::Escape {
				self.cancel_capture();
				return KeyboardAction::Cancelled;
			}
			if device != PromptDevice::Keyboard || !can_remap_key(key) {
				return KeyboardAction::Stay;
			}
			let Some(action_id) = self.capturing.take() else {
				return KeyboardAction::Stay;
			};
			self.draft.retain(|row| row.action_id != action_id);
			if self
				.authored
				.actions
				.iter()
				.any(|binding| binding.id == action_id && binding.key != key)
			{
				self.draft.push(ActionKeyOverride { action_id, key });
			}
			return KeyboardAction::Edited;
		}
		match key {
			Key::Escape => return KeyboardAction::Back,
			Key::ArrowUp | Key::ArrowDown => {
				self.focus = super::advance_row(self.focus, self.authored.actions.len() + 3, key);
			}
			Key::Enter => {
				let actions = self.authored.actions.len();
				if self.focus == actions + 2 {
					return KeyboardAction::Back;
				}
				if !self.editable {
					return KeyboardAction::Locked;
				}
				if let Some(binding) = self.authored.actions.get(self.focus) {
					self.capturing = Some(binding.id.clone());
					return KeyboardAction::Started;
				}
				if self.focus == actions {
					return KeyboardAction::Apply;
				}
				self.draft.clear();
				return KeyboardAction::Edited;
			}
			_ => {}
		}
		KeyboardAction::Stay
	}
}

pub(super) enum KeyboardAction {
	Stay,
	Started,
	Cancelled,
	Edited,
	Apply,
	Back,
	Locked,
}
