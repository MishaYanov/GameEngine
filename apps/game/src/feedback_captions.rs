//! Two-second visual equivalents of the Player's committed feedback cues.
use crate::{player_controls::DisplaySettings, project_sound::CombatCue};
use std::{
	collections::VecDeque,
	time::{Duration, Instant},
};

const LIFETIME: Duration = Duration::from_secs(2);
const LIMIT: usize = 3;

struct Caption {
	cue: CombatCue,
	expires: Instant,
}

#[derive(Default)]
pub(super) struct FeedbackCaptions {
	entries: VecDeque<Caption>,
}

impl FeedbackCaptions {
	pub(super) fn push(&mut self, cue: CombatCue, now: Instant) {
		self.entries
			.retain(|entry| entry.cue != cue && entry.expires > now);
		// Each cue has at most one entry. Apply the screen limit after
		// filtering, so a hidden cue cannot evict a visible caption.
		self.entries.push_back(Caption {
			cue,
			expires: now + LIFETIME,
		});
	}

	pub(super) fn rows(
		&self,
		now: Instant,
		display: DisplaySettings,
	) -> impl Iterator<Item = &'static str> + '_ {
		self.entries
			.iter()
			.rev()
			.filter(move |entry| entry.expires > now && display.captions_enabled_for(entry.cue))
			.take(LIMIT)
			.map(|entry| entry.cue.caption())
	}

	pub(super) fn expire(&mut self, now: Instant) -> bool {
		let count = self.entries.len();
		self.entries.retain(|entry| entry.expires > now);
		self.entries.len() != count
	}

	pub(super) fn next_expiry(&self) -> Option<Instant> {
		self.entries.front().map(|entry| entry.expires)
	}

	pub(super) fn clear(&mut self) {
		self.entries.clear();
	}
}

#[cfg(test)]
#[path = "../tests/unit/feedback_captions.rs"]
mod tests;
