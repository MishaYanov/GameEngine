//! Local feedback audio preferences and the draft shown in Player Controls.

use bridge::feedback_audio::FeedbackBus;
use runtime::Key;
use serde::{Deserialize, Serialize};

use super::listening_check::ListeningCheck;
use crate::{
	feedback_audio::{AuditionScope, CueAuditionOutcome, CueClipSource, FeedbackAudio},
	project_sound::{CombatCue, ProjectSound},
};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LocalBusLevels {
	collection: u8,
	mechanisms: u8,
	completion: u8,
}

impl Default for LocalBusLevels {
	fn default() -> Self {
		Self {
			collection: 100,
			mechanisms: 100,
			completion: 100,
		}
	}
}

impl LocalBusLevels {
	fn percent(self, bus: FeedbackBus) -> u8 {
		match bus {
			FeedbackBus::Collection => self.collection,
			FeedbackBus::Mechanisms => self.mechanisms,
			FeedbackBus::Completion => self.completion,
		}
	}

	fn adjust(&mut self, bus: FeedbackBus, step: i16) {
		let level = match bus {
			FeedbackBus::Collection => &mut self.collection,
			FeedbackBus::Mechanisms => &mut self.mechanisms,
			FeedbackBus::Completion => &mut self.completion,
		};
		*level = adjust_percent(*level, step);
	}
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FeedbackMix {
	pub(super) level_percent: u8,
	pub(super) muted: bool,
	pub(super) bus_percent: LocalBusLevels,
}

impl Default for FeedbackMix {
	fn default() -> Self {
		Self {
			level_percent: 100,
			muted: false,
			bus_percent: LocalBusLevels::default(),
		}
	}
}

impl FeedbackMix {
	pub(crate) fn validate(self) -> Result<Self, &'static str> {
		let LocalBusLevels {
			collection,
			mechanisms,
			completion,
		} = self.bus_percent;
		if [self.level_percent, collection, mechanisms, completion]
			.into_iter()
			.all(|level| level <= 100)
		{
			Ok(self)
		} else {
			Err("Feedback audio levels must be from 0 to 100")
		}
	}

	pub(crate) fn effective_percent(self) -> u8 {
		if self.muted { 0 } else { self.level_percent }
	}

	pub(crate) fn category_percent(self, bus: FeedbackBus) -> u8 {
		self.bus_percent.percent(bus)
	}
}

#[derive(Clone, Copy)]
enum Row {
	Volume,
	Mute,
	Bus(FeedbackBus),
	RestoreDefaults,
	Apply,
	Test(FeedbackBus),
	CueDetails,
	ListeningCheck,
	Back,
}

pub(super) struct AudioMenu {
	focus: usize,
	draft: FeedbackMix,
	details: Option<CueDetailsMenu>,
	listening: Option<ListeningCheck>,
	cinder: bool,
	audition: AuditionScope,
}

impl AudioMenu {
	const ROWS: [Row; 12] = [
		Row::Volume,
		Row::Mute,
		Row::Bus(FeedbackBus::Collection),
		Row::Bus(FeedbackBus::Mechanisms),
		Row::Bus(FeedbackBus::Completion),
		Row::RestoreDefaults,
		Row::Apply,
		Row::Test(FeedbackBus::Collection),
		Row::Test(FeedbackBus::Mechanisms),
		Row::Test(FeedbackBus::Completion),
		Row::CueDetails,
		Row::Back,
	];

	const CINDER_ROWS: [Row; 9] = [
		Row::Volume,
		Row::Mute,
		Row::Bus(FeedbackBus::Collection),
		Row::Bus(FeedbackBus::Mechanisms),
		Row::Bus(FeedbackBus::Completion),
		Row::RestoreDefaults,
		Row::Apply,
		Row::ListeningCheck,
		Row::Back,
	];

	pub(super) fn new(draft: FeedbackMix, cinder: bool) -> Self {
		Self {
			focus: 0,
			draft,
			details: None,
			listening: None,
			cinder,
			audition: AuditionScope::default(),
		}
	}

	fn actions(&self) -> &[Row] {
		if self.cinder {
			&Self::CINDER_ROWS
		} else {
			&Self::ROWS
		}
	}

	pub(super) fn rows(
		&self,
		audio: &FeedbackAudio,
		project: &ProjectSound,
		applied: FeedbackMix,
		captions_enabled: bool,
	) -> (String, Vec<String>, usize) {
		if let Some(listening) = &self.listening {
			return listening.rows(project.preview_status(), applied);
		}
		match &self.details {
			Some(details) => details.rows(audio, captions_enabled),
			None => self.settings_rows(),
		}
	}

	fn settings_rows(&self) -> (String, Vec<String>, usize) {
		let mute = if self.draft.muted { "On" } else { "Off" };
		(
			"Feedback Audio".into(),
			self.actions()
				.iter()
				.copied()
				.map(|row| match row {
					Row::Volume => format!("Volume: {}%", self.draft.level_percent),
					Row::Mute => format!("Mute: {mute}"),
					Row::Bus(bus) => {
						format!(
							"{}: {}%",
							self.category_label(bus),
							self.draft.category_percent(bus)
						)
					}
					Row::RestoreDefaults => "Restore Category Defaults".into(),
					Row::Apply => "Apply".into(),
					Row::Test(bus) => format!("Test {}", bus_label(bus)),
					Row::CueDetails => "Cue Details".into(),
					Row::ListeningCheck => "Start Audio Check".into(),
					Row::Back => "Back".into(),
				})
				.collect(),
			self.focus,
		)
	}

	pub(super) fn audition(&self, audio: &FeedbackAudio, bus: FeedbackBus) -> &'static str {
		match audio.audition(bus, &self.audition) {
			#[cfg(windows)]
			CueAuditionOutcome::Submitted => "Cue test submitted",
			CueAuditionOutcome::SilentMix => "Cue gain is zero",
			CueAuditionOutcome::MissingClip => "No clip assigned",
			CueAuditionOutcome::OutputUnavailable => "Queue unavailable",
		}
	}

	pub(super) fn key(&mut self, key: Key, project: &ProjectSound) -> AudioAction {
		if let Some(check) = &mut self.listening {
			let action = check.key(key, project.preview_status());
			if matches!(action, AudioAction::CloseCheck) {
				self.listening = None;
				return AudioAction::ResetPreview;
			}
			return action;
		}
		if let Some(details) = &mut self.details {
			if details.back_requested(key) {
				self.details = None;
			}
			return AudioAction::Stay;
		}
		let row = self.actions()[self.focus];
		match key {
			Key::Escape => return AudioAction::Back,
			Key::ArrowUp | Key::ArrowDown => {
				self.focus = super::advance_row(self.focus, self.actions().len(), key);
			}
			Key::ArrowLeft | Key::ArrowRight => {
				let step = if key == Key::ArrowRight { 10 } else { -10 };
				match row {
					Row::Volume => {
						self.draft.level_percent = adjust_percent(self.draft.level_percent, step);
					}
					Row::Mute => self.draft.muted = !self.draft.muted,
					Row::Bus(bus) => self.draft.bus_percent.adjust(bus, step),
					_ => {}
				}
			}
			Key::Enter => match row {
				Row::Mute => self.draft.muted = !self.draft.muted,
				Row::RestoreDefaults => self.draft.bus_percent = LocalBusLevels::default(),
				Row::Apply => return AudioAction::Apply(self.draft),
				Row::Test(bus) => return AudioAction::Audition(bus),
				Row::CueDetails => self.details = Some(CueDetailsMenu::default()),
				Row::ListeningCheck => {
					self.listening = Some(ListeningCheck::new(project.configured_cues()));
					return AudioAction::ResetPreview;
				}
				Row::Back => return AudioAction::Back,
				_ => {}
			},
			_ => {}
		}
		AudioAction::Stay
	}
	fn category_label(&self, bus: FeedbackBus) -> &'static str {
		if self.cinder {
			match bus {
				FeedbackBus::Collection => "Sword actions",
				FeedbackBus::Mechanisms => "Damage taken",
				FeedbackBus::Completion => "Duel outcome",
			}
		} else {
			bus_label(bus)
		}
	}
}

#[derive(Default)]
struct CueDetailsMenu {
	focus: usize,
	category: usize,
}

impl CueDetailsMenu {
	const CATEGORIES: [FeedbackBus; 3] = [
		FeedbackBus::Collection,
		FeedbackBus::Mechanisms,
		FeedbackBus::Completion,
	];
	const ROW_COUNT: usize = 9;

	fn rows(&self, audio: &FeedbackAudio, captions_enabled: bool) -> (String, Vec<String>, usize) {
		let bus = Self::CATEGORIES[self.category];
		let details = audio.cue_details(bus);
		let clip = match details.clip {
			CueClipSource::Authored => "Authored",
			CueClipSource::Missing => "Missing",
		};
		let captions = if captions_enabled { "On" } else { "Off" };
		let rows: [String; Self::ROW_COUNT] = [
			format!("Category: {}", bus_label(bus)),
			format!("Master: {}%", details.factors.master_percent),
			format!("Local: {}%", details.factors.local_percent),
			format!("Project: {}%", details.factors.project_percent),
			format!("Scene: {}%", details.factors.scene_percent),
			format!("Resolved: {}%", details.resolved_percent),
			format!("Clip: {clip}"),
			format!("Cue captions: {captions}"),
			"Back".into(),
		];
		("Cue Details".into(), rows.into(), self.focus)
	}

	fn back_requested(&mut self, key: Key) -> bool {
		match key {
			Key::Escape => return true,
			Key::Enter if self.focus == Self::ROW_COUNT - 1 => return true,
			Key::ArrowUp | Key::ArrowDown => {
				self.focus = super::advance_row(self.focus, Self::ROW_COUNT, key);
			}
			Key::ArrowLeft | Key::ArrowRight | Key::Enter if self.focus == 0 => {
				let count = Self::CATEGORIES.len();
				let step = if key == Key::ArrowLeft { count - 1 } else { 1 };
				self.category = (self.category + step) % count;
			}
			_ => {}
		}
		false
	}
}

fn adjust_percent(level: u8, step: i16) -> u8 {
	(i16::from(level) + step).clamp(0, 100) as u8
}

fn bus_label(bus: FeedbackBus) -> &'static str {
	match bus {
		FeedbackBus::Collection => "Collection",
		FeedbackBus::Mechanisms => "Mechanisms",
		FeedbackBus::Completion => "Completion",
	}
}

pub(super) enum AudioAction {
	Stay,
	Apply(FeedbackMix),
	Audition(FeedbackBus),
	ProjectAudition(CombatCue),
	ResetPreview,
	CloseCheck,
	Back,
}

#[cfg(test)]
#[path = "../../tests/unit/player_controls_audio.rs"]
mod tests;
