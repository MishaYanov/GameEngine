//! Temporary guided listening results for the current project's committed mix.

use super::{FeedbackMix, advance_row, audio::AudioAction};
use crate::project_sound::{CombatCue, PreviewStatus};
use runtime::Key;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ListeningOutcome {
	Heard,
	Inaudible,
	Skipped,
}

impl ListeningOutcome {
	fn label(self) -> &'static str {
		match self {
			Self::Heard => "Heard",
			Self::Inaudible => "Inaudible",
			Self::Skipped => "Skipped",
		}
	}
}

#[derive(Clone, Copy)]
enum Row {
	Info,
	Play,
	Record(ListeningOutcome),
	Next,
	Back,
}

pub(super) struct ListeningCheck {
	cues: Vec<CombatCue>,
	outcomes: Vec<ListeningOutcome>,
	draft_outcome: Option<ListeningOutcome>,
	focus: usize,
}

impl ListeningCheck {
	const ROWS: [Row; 8] = [
		Row::Info,
		Row::Info,
		Row::Play,
		Row::Record(ListeningOutcome::Heard),
		Row::Record(ListeningOutcome::Inaudible),
		Row::Record(ListeningOutcome::Skipped),
		Row::Next,
		Row::Back,
	];

	pub(super) fn new(cues: Vec<CombatCue>) -> Self {
		let focus = if cues.is_empty() { 0 } else { 2 };
		Self {
			cues,
			outcomes: Vec::new(),
			draft_outcome: None,
			focus,
		}
	}

	pub(super) fn rows(
		&self,
		status: PreviewStatus,
		mix: FeedbackMix,
	) -> (String, Vec<String>, usize) {
		let Some(cue) = self.current() else {
			let mut rows = vec!["Back".into()];
			if self.cues.is_empty() {
				rows.push("No combat cues configured".into());
			} else {
				for outcome in [
					ListeningOutcome::Heard,
					ListeningOutcome::Inaudible,
					ListeningOutcome::Skipped,
				] {
					let count = self
						.outcomes
						.iter()
						.filter(|choice| **choice == outcome)
						.count();
					rows.push(format!("{}: {count}", outcome.label()));
				}
			}
			let missing = CombatCue::ALL.len() - self.cues.len();
			if missing > 0 {
				rows.push(format!("Missing clips: {missing}"));
			}
			return ("Audio Check Results".into(), rows, self.focus);
		};

		let settings = match status {
			PreviewStatus::Ready => "Select Play",
			PreviewStatus::Playing { .. } => "Playing preview",
			PreviewStatus::Finished => "Preview finished",
			PreviewStatus::Interrupted => "Preview interrupted",
			PreviewStatus::SilentMix => "Silent at current gain",
			PreviewStatus::MissingClip => "Clip missing",
			PreviewStatus::OutputUnavailable => "Audio output unavailable",
		};
		let rows = vec![
			gain_label(cue, mix),
			settings.into(),
			if status.busy() {
				"Playing...".into()
			} else if status == PreviewStatus::Ready {
				"Play".into()
			} else {
				"Repeat".into()
			},
			self.outcome_row(ListeningOutcome::Heard, status),
			self.outcome_row(ListeningOutcome::Inaudible, status),
			self.outcome_row(ListeningOutcome::Skipped, status),
			if status.busy() || self.draft_outcome.is_none() {
				"Next (choose result)".into()
			} else {
				"Next".into()
			},
			"Back".into(),
		];
		(
			format!(
				"{} ({}/{})",
				cue.caption(),
				self.outcomes.len() + 1,
				self.cues.len()
			),
			rows,
			self.focus,
		)
	}

	pub(super) fn key(&mut self, key: Key, status: PreviewStatus) -> AudioAction {
		match key {
			Key::Escape => return AudioAction::CloseCheck,
			Key::Enter if self.current().is_none() && self.focus == 0 => {
				return AudioAction::CloseCheck;
			}
			Key::ArrowUp | Key::ArrowDown => {
				let count = if self.current().is_some() {
					Self::ROWS.len()
				} else {
					1 + if self.cues.is_empty() { 1 } else { 3 }
						+ usize::from(self.cues.len() < CombatCue::ALL.len())
				};
				self.focus = advance_row(self.focus, count, key);
			}
			Key::Enter => {
				let Some(cue) = self.current() else {
					return AudioAction::Stay;
				};
				match Self::ROWS[self.focus] {
					Row::Play if !status.busy() => {
						self.draft_outcome = None;
						return AudioAction::ProjectAudition(cue);
					}
					Row::Record(outcome) if can_record(outcome, status) => {
						self.draft_outcome = Some(outcome);
						self.focus = 6;
					}
					Row::Next if !status.busy() => {
						if let Some(outcome) = self.draft_outcome.take() {
							self.outcomes.push(outcome);
							self.focus = if self.current().is_some() { 2 } else { 0 };
							return AudioAction::ResetPreview;
						}
					}
					Row::Back => return AudioAction::CloseCheck,
					_ => {}
				}
			}
			_ => {}
		}
		AudioAction::Stay
	}

	fn current(&self) -> Option<CombatCue> {
		self.cues.get(self.outcomes.len()).copied()
	}

	fn outcome_row(&self, outcome: ListeningOutcome, status: PreviewStatus) -> String {
		if self.draft_outcome == Some(outcome) {
			format!("Selected: {}", outcome.label())
		} else if can_record(outcome, status) {
			outcome.label().into()
		} else {
			format!("{} (wait for play)", outcome.label())
		}
	}
}

fn can_record(outcome: ListeningOutcome, status: PreviewStatus) -> bool {
	match outcome {
		ListeningOutcome::Heard => status == PreviewStatus::Finished,
		ListeningOutcome::Inaudible => !status.busy() && status != PreviewStatus::Ready,
		ListeningOutcome::Skipped => !status.busy(),
	}
}

fn gain_label(cue: CombatCue, mix: FeedbackMix) -> String {
	let gain = cue.gain(mix);
	if gain > 0 {
		return format!("Applied gain: {gain}%");
	}
	let reason = if mix.muted {
		"Muted"
	} else if mix.level_percent == 0 {
		"Volume zero"
	} else if mix.category_percent(cue.bus()) == 0 {
		"Category zero"
	} else {
		"Mix rounds to zero"
	};
	format!("Gain 0% ({reason})")
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::time::Instant;

	#[test]
	fn manual_outcomes_require_current_attempt_and_repeat_or_next_resets_it() {
		let mix = FeedbackMix {
			muted: true,
			..FeedbackMix::default()
		};
		let mut check =
			ListeningCheck::new(vec![CombatCue::Hit, CombatCue::Hurt, CombatCue::Defeat]);
		assert_eq!(
			check.rows(PreviewStatus::Ready, mix).1[0],
			"Gain 0% (Muted)"
		);
		assert!(matches!(
			check.key(Key::Enter, PreviewStatus::Ready),
			AudioAction::ProjectAudition(CombatCue::Hit)
		));
		let now = Instant::now();
		let playing = PreviewStatus::Playing {
			poll_at: now,
			timeout: now,
		};
		assert!(matches!(check.key(Key::Enter, playing), AudioAction::Stay));
		check.focus = 6;
		assert!(matches!(check.key(Key::Enter, playing), AudioAction::Stay));
		assert!(matches!(
			check.key(Key::Enter, PreviewStatus::Finished),
			AudioAction::Stay
		));
		check.focus = 3;
		check.key(Key::Enter, PreviewStatus::Finished);
		assert_eq!(check.draft_outcome, Some(ListeningOutcome::Heard));
		check.focus = 2;
		assert!(matches!(
			check.key(Key::Enter, PreviewStatus::Finished),
			AudioAction::ProjectAudition(CombatCue::Hit)
		));
		assert_eq!(check.draft_outcome, None);
		check.focus = 3;
		check.key(Key::Enter, PreviewStatus::Finished);
		assert!(matches!(
			check.key(Key::Enter, PreviewStatus::Finished),
			AudioAction::ResetPreview
		));

		// A failed/silent attempt can be reported only after playback was tried.
		assert_eq!(check.current(), Some(CombatCue::Hurt));
		check.focus = 4;
		check.key(Key::Enter, PreviewStatus::Ready);
		assert_eq!(check.draft_outcome, None);
		check.key(Key::Enter, PreviewStatus::SilentMix);
		assert_eq!(check.draft_outcome, Some(ListeningOutcome::Inaudible));
		assert!(matches!(
			check.key(Key::Enter, PreviewStatus::SilentMix),
			AudioAction::ResetPreview
		));

		assert_eq!(check.current(), Some(CombatCue::Defeat));
		check.focus = 5;
		check.key(Key::Enter, PreviewStatus::Ready);
		assert!(matches!(
			check.key(Key::Enter, PreviewStatus::Ready),
			AudioAction::ResetPreview
		));
		assert_eq!(
			check.rows(PreviewStatus::Ready, mix).1,
			[
				"Back",
				"Heard: 1",
				"Inaudible: 1",
				"Skipped: 1",
				"Missing clips: 2"
			]
		);
		assert!(matches!(
			check.key(Key::Escape, PreviewStatus::Ready),
			AudioAction::CloseCheck
		));
	}

	#[test]
	fn absent_project_clips_show_an_empty_check() {
		let mut check = ListeningCheck::new(Vec::new());
		assert_eq!(
			check.rows(PreviewStatus::Ready, FeedbackMix::default()).1,
			["Back", "No combat cues configured", "Missing clips: 5"]
		);
		assert!(matches!(
			check.key(Key::Enter, PreviewStatus::Ready),
			AudioAction::CloseCheck
		));
	}
}
