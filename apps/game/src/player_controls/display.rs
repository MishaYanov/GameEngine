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

use crate::project_sound::CombatCue;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CaptionCategories {
	pub(super) sword: bool,
	pub(super) damage: bool,
	pub(super) outcome: bool,
}

impl Default for CaptionCategories {
	fn default() -> Self {
		Self {
			sword: true,
			damage: true,
			outcome: true,
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DisplaySettings {
	pub(super) text_size: TextSize,
	pub(super) high_contrast: bool,
	pub(crate) cue_captions: bool,
	pub(super) caption_categories: CaptionCategories,
}

impl DisplaySettings {
	pub(crate) fn overlay_style(self) -> ScreenOverlayStyle {
		ScreenOverlayStyle {
			text_scale: self.text_size.factor(),
			high_contrast: self.high_contrast,
		}
	}

	pub(crate) fn captions_enabled_for(self, cue: CombatCue) -> bool {
		self.cue_captions
			&& match cue {
			CombatCue::Swing | CombatCue::Hit => self.caption_categories.sword,
			CombatCue::Hurt => self.caption_categories.damage,
			CombatCue::Victory | CombatCue::Defeat => self.caption_categories.outcome,
		}
	}
}

impl Default for DisplaySettings {
	fn default() -> Self {
		Self {
			text_size: TextSize::Standard,
			high_contrast: false,
			cue_captions: true,
			caption_categories: CaptionCategories::default(),
		}
	}
}

#[derive(Clone, Copy)]
enum DisplayRow {
	TextSize,
	Contrast,
	Captions,
	Categories,
	Preview,
	Apply,
	Back,
}

enum Page {
	Settings,
	Categories(usize),
	Preview { cue: usize, focus: usize },
}

pub(super) struct DisplayMenu {
	focus: usize,
	draft: DisplaySettings,
	page: Page,
}

impl DisplayMenu {
	const ROWS: [DisplayRow; 7] = [
		DisplayRow::TextSize,
		DisplayRow::Contrast,
		DisplayRow::Captions,
		DisplayRow::Apply,
		DisplayRow::Categories,
		DisplayRow::Preview,
		DisplayRow::Back,
	];
	const CATEGORY_ROWS: [&'static str; 3] = ["Sword actions", "Damage taken", "Duel outcome"];

	pub(super) fn new(draft: DisplaySettings) -> Self {
		Self {
			focus: 0,
			draft,
			page: Page::Settings,
		}
	}

	pub(super) fn preview_style(&self) -> Option<ScreenOverlayStyle> {
		matches!(self.page, Page::Preview { .. }).then_some(self.draft.overlay_style())
	}

	pub(super) fn rows(&self) -> (String, Vec<String>, usize) {
		let toggle = |value| if value { "On" } else { "Off" };
		match self.page {
			Page::Settings => (
				"Accessibility".into(),
				Self::ROWS
					.into_iter()
					.map(|row| match row {
						DisplayRow::TextSize => {
							format!("Text size: {}", self.draft.text_size.label())
						}
						DisplayRow::Contrast => {
							format!("High contrast: {}", toggle(self.draft.high_contrast))
						}
						DisplayRow::Captions => {
							format!("Cue captions: {}", toggle(self.draft.cue_captions))
						}
						DisplayRow::Categories => "Caption categories".into(),
						DisplayRow::Preview => "Preview captions".into(),
						DisplayRow::Apply => "Apply".into(),
						DisplayRow::Back => "Back".into(),
					})
					.collect(),
				self.focus,
			),
			Page::Categories(focus) => {
				let categories = self.draft.caption_categories;
				let rows = Self::CATEGORY_ROWS
					.into_iter()
					.zip([categories.sword, categories.damage, categories.outcome])
					.map(|(label, on)| format!("{label}: {}", toggle(on)))
					.chain(["Back".into()])
					.collect();
				("Caption categories".into(), rows, focus)
			}
			Page::Preview { cue, focus } => {
				let cue = CombatCue::ALL[cue];
				let status = if self.draft.captions_enabled_for(cue) {
					"Shown in play"
				} else if self.draft.cue_captions {
					"Category off"
				} else {
					"Cue captions off"
				};
				(
					"Caption preview (visual only)".into(),
					vec![
						cue.caption().into(),
						status.into(),
						"Previous example".into(),
						"Next example".into(),
						"Back".into(),
					],
					focus,
				)
			}
		}
	}

	pub(super) fn key(&mut self, key: Key) -> DisplayAction {
		match &mut self.page {
			Page::Settings => return self.settings_key(key),
			Page::Categories(focus) => match key {
				Key::Escape | Key::Enter
				if key == Key::Escape || *focus == Self::CATEGORY_ROWS.len() =>
					{
						self.page = Page::Settings;
					}
				Key::ArrowUp | Key::ArrowDown => {
					*focus = super::advance_row(*focus, Self::CATEGORY_ROWS.len() + 1, key);
				}
				Key::Enter | Key::ArrowLeft | Key::ArrowRight
				if *focus < Self::CATEGORY_ROWS.len() =>
					{
						let categories = &mut self.draft.caption_categories;
						let category = [
							&mut categories.sword,
							&mut categories.damage,
							&mut categories.outcome,
						];
						*category[*focus] = !*category[*focus];
					}
				_ => {}
			},
			Page::Preview { cue, focus } => match key {
				Key::Escape | Key::Enter if key == Key::Escape || *focus == 4 => {
					self.page = Page::Settings;
				}
				Key::ArrowUp | Key::ArrowDown => *focus = super::advance_row(*focus, 5, key),
				Key::ArrowLeft | Key::Enter if key == Key::ArrowLeft || *focus == 2 => {
					*cue = (*cue + CombatCue::ALL.len() - 1) % CombatCue::ALL.len();
					*focus = 0;
				}
				Key::ArrowRight | Key::Enter if key == Key::ArrowRight || *focus == 3 => {
					*cue = (*cue + 1) % CombatCue::ALL.len();
					*focus = 0;
				}
				_ => {}
			},
		}
		DisplayAction::Stay
	}

	fn settings_key(&mut self, key: Key) -> DisplayAction {
		let row = Self::ROWS[self.focus];
		match key {
			Key::Escape => return DisplayAction::Back,
			Key::ArrowUp | Key::ArrowDown => {
				self.focus = super::advance_row(self.focus, Self::ROWS.len(), key);
			}
			Key::ArrowLeft | Key::ArrowRight => match row {
				DisplayRow::TextSize => {
					self.draft.text_size = self.draft.text_size.adjacent(key == Key::ArrowRight)
				}
				DisplayRow::Contrast => self.draft.high_contrast = !self.draft.high_contrast,
				DisplayRow::Captions => self.draft.cue_captions = !self.draft.cue_captions,
				_ => {}
			},
			Key::Enter => match row {
				DisplayRow::Categories => self.page = Page::Categories(0),
				DisplayRow::Preview => self.page = Page::Preview { cue: 0, focus: 0 },
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
