//! Two-second visual equivalents of the Player's committed feedback cues.
use bridge::feedback_audio::FeedbackCue;
use std::{
	collections::VecDeque,
	time::{Duration, Instant},
};

const LIFETIME: Duration = Duration::from_secs(2);
const LIMIT: usize = 3;

struct Caption {
	cue: FeedbackCue,
	expires: Instant,
}

#[derive(Default)]
pub(super) struct FeedbackCaptions {
	entries: VecDeque<Caption>,
}

impl FeedbackCaptions {
	pub(super) fn push(&mut self, cue: FeedbackCue, now: Instant) {
		self.entries
			.retain(|entry| entry.cue != cue && entry.expires > now);
		if self.entries.len() == LIMIT {
			self.entries.pop_front();
		}
		self.entries.push_back(Caption {
			cue,
			expires: now + LIFETIME,
		});
	}

	pub(super) fn rows(&self, now: Instant) -> impl Iterator<Item=&'static str> + '_ {
		self.entries
			.iter()
			.rev()
			.filter(move |entry| entry.expires > now)
			.map(|entry| label(entry.cue))
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

fn label(cue: FeedbackCue) -> &'static str {
	match cue {
		FeedbackCue::KeyCollected => "Key collected",
		FeedbackCue::GateOpened => "Gate opened",
		FeedbackCue::RunCompleted => "Run completed",
	}
}

#[cfg(test)]
#[path = "../tests/unit/feedback_captions.rs"]
mod tests;
