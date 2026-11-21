//! The local display options and the draft edited by the Player menu.
use renderer::ui::ScreenOverlayStyle;
use runtime::Key;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum TextSize {
	#[default]
	Standard,
	Large,
	ExtraLarge,
}

impl TextSize {
	fn factor(self) -> f32 {
		match self {
			Self::Standard => 1.0,
			Self::Large => 1.25,
			Self::ExtraLarge => 1.5,
		}
	}

	fn label(self) -> &'static str {
		match self {
			Self::Standard => "Standard",
			Self::Large => "Large",
			Self::ExtraLarge => "Extra large",
		}
	}

	fn adjacent(self, forward: bool) -> Self {
		let sizes = [Self::Standard, Self::Large, Self::ExtraLarge];
		sizes[(self as usize + if forward { 1 } else { 2 }) % sizes.len()]
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DisplaySettings {
	pub(super) text_size: TextSize,
	pub(super) high_contrast: bool,
	pub(crate) cue_captions: bool,
}

impl DisplaySettings {
	pub(crate) fn overlay_style(self) -> ScreenOverlayStyle {
		ScreenOverlayStyle {
			text_scale: self.text_size.factor(),
			high_contrast: self.high_contrast,
		}
	}
}

impl Default for DisplaySettings {
	fn default() -> Self {
		Self {
			text_size: TextSize::Standard,
			high_contrast: false,
			cue_captions: true,
		}
	}
}

#[derive(Clone, Copy)]
enum DisplayRow {
	TextSize,
	Contrast,
	Captions,
	Apply,
	Back,
}

pub(super) struct DisplayMenu {
	focus: DisplayRow,
	draft: DisplaySettings,
}

impl DisplayMenu {
	pub(super) fn new(draft: DisplaySettings) -> Self {
		Self {
			focus: DisplayRow::TextSize,
			draft,
		}
	}

	pub(super) fn rows(&self) -> (String, Vec<String>, usize) {
		let toggle = |value| if value { "On" } else { "Off" };
		(
			"Accessibility".into(),
			vec![
				format!("Text size: {}", self.draft.text_size.label()),
				format!("High contrast: {}", toggle(self.draft.high_contrast)),
				format!("Cue captions: {}", toggle(self.draft.cue_captions)),
				"Apply".into(),
				"Back".into(),
			],
			self.focus as usize,
		)
	}

	pub(super) fn key(&mut self, key: Key) -> DisplayAction {
		match key {
			Key::Escape => return DisplayAction::Back,
			Key::ArrowUp | Key::ArrowDown => {
				use DisplayRow::*;
				let rows = [TextSize, Contrast, Captions, Apply, Back];
				let index = super::advance_row(self.focus as usize, rows.len(), key);
				self.focus = rows[index];
			}
			Key::ArrowLeft | Key::ArrowRight => {
				let forward = key == Key::ArrowRight;
				match self.focus {
					DisplayRow::TextSize => {
						self.draft.text_size = self.draft.text_size.adjacent(forward)
					}
					DisplayRow::Contrast => self.draft.high_contrast = !self.draft.high_contrast,
					DisplayRow::Captions => self.draft.cue_captions = !self.draft.cue_captions,
					DisplayRow::Apply | DisplayRow::Back => {}
				}
			}
			Key::Enter => match self.focus {
				DisplayRow::Apply => return DisplayAction::Apply(self.draft),
				DisplayRow::Back => return DisplayAction::Back,
				_ => {}
			},
			_ => {}
		}
		DisplayAction::Stay
	}
}

pub(super) enum DisplayAction {
	Stay,
	Apply(DisplaySettings),
	Back,
}
