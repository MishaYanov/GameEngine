//! Local feedback audio preferences and the draft shown in Player Controls.

use runtime::Key;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FeedbackMix {
	pub(super) level_percent: u8,
	pub(super) muted: bool,
}

impl Default for FeedbackMix {
	fn default() -> Self {
		Self {
			level_percent: 100,
			muted: false,
		}
	}
}

impl FeedbackMix {
	pub(super) fn validate(self) -> Result<Self, &'static str> {
		if self.level_percent <= 100 {
			Ok(self)
		} else {
			Err("Feedback volume must be from 0 to 100")
		}
	}

	pub(crate) fn effective_percent(self) -> u8 {
		if self.muted { 0 } else { self.level_percent }
	}
}

#[derive(Clone, Copy)]
enum Row {
	Volume,
	Mute,
	Apply,
	Back,
}

pub(super) struct AudioMenu {
	focus: Row,
	draft: FeedbackMix,
}

impl AudioMenu {
	pub(super) fn new(draft: FeedbackMix) -> Self {
		Self {
			focus: Row::Volume,
			draft,
		}
	}

	pub(super) fn rows(&self) -> (String, Vec<String>, usize) {
		let mute = if self.draft.muted { "On" } else { "Off" };
		(
			"Feedback Audio".into(),
			vec![
				format!("Volume: {}%", self.draft.level_percent),
				format!("Mute: {mute}"),
				"Apply".into(),
				"Back".into(),
			],
			self.focus as usize,
		)
	}

	pub(super) fn key(&mut self, key: Key) -> AudioAction {
		match key {
			Key::Escape => return AudioAction::Back,
			Key::ArrowUp | Key::ArrowDown => {
				let rows = [Row::Volume, Row::Mute, Row::Apply, Row::Back];
				self.focus = rows[super::advance_row(self.focus as usize, rows.len(), key)];
			}
			Key::ArrowLeft | Key::ArrowRight => match self.focus {
				Row::Volume => {
					let volume = self.draft.level_percent;
					self.draft.level_percent = if key == Key::ArrowRight {
						volume.saturating_add(10).min(100)
					} else {
						volume.saturating_sub(10)
					};
				}
				Row::Mute => self.draft.muted = !self.draft.muted,
				Row::Apply | Row::Back => {}
			},
			Key::Enter => match self.focus {
				Row::Volume => {}
				Row::Mute => self.draft.muted = !self.draft.muted,
				Row::Apply => return AudioAction::Apply(self.draft),
				Row::Back => return AudioAction::Back,
			},
			_ => {}
		}
		AudioAction::Stay
	}
}

pub(super) enum AudioAction {
	Stay,
	Apply(FeedbackMix),
	Back,
}

#[cfg(test)]
#[path = "../../tests/unit/player_controls_audio.rs"]
mod tests;
