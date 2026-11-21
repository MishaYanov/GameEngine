use super::PhysicalGamepadSample;
use std::time::{Duration, Instant};

const CONNECTED_INTERVAL: Duration = Duration::from_millis(16);
const DISCOVERY_INTERVAL: Duration = Duration::from_millis(250);

/// Owns one attached controller and waits for neutrality after ownership changes.
pub struct GamepadOwner {
	slot: Option<u32>,
	next_poll: Option<Instant>,
	sample: PhysicalGamepadSample,
	waiting_for_neutral: bool,
}

impl Default for GamepadOwner {
	fn default() -> Self {
		Self {
			slot: None,
			next_poll: None,
			sample: PhysicalGamepadSample::default(),
			waiting_for_neutral: true,
		}
	}
}

impl GamepadOwner {
	pub fn sample(&self) -> PhysicalGamepadSample {
		self.sample
	}

	pub fn deadline(&self) -> Option<Instant> {
		self.next_poll
	}

	pub fn connected(&self) -> bool {
		self.slot.is_some()
	}

	/// Clear input when focus, play ownership, or scene identity changes.
	pub fn reset(&mut self) {
		self.next_poll = None;
		self.sample = PhysicalGamepadSample::default();
		self.waiting_for_neutral = true;
	}

	/// Returns true if the admitted physical sample changed.
	pub fn poll(
		&mut self,
		now: Instant,
		mut read: impl FnMut(u32) -> Option<PhysicalGamepadSample>,
	) -> bool {
		if self.next_poll.is_some_and(|deadline| now < deadline) {
			return false;
		}
		let previous = self.sample;
		let state = match self.slot {
			Some(slot) => read(slot),
			None => (0..4).find_map(|slot| {
				let sample = read(slot)?;
				self.slot = Some(slot);
				Some(sample)
			}),
		};
		if let Some(mut sample) = state {
			self.next_poll = Some(now + CONNECTED_INTERVAL);
			if self.waiting_for_neutral {
				self.waiting_for_neutral = !sample.is_neutral();
			} else {
				if sample.movement() == [0.0; 2] {
					sample.left_x = 0;
					sample.left_y = 0;
				}
				self.sample = sample;
			}
		} else {
			self.reset();
			self.slot = None;
			self.next_poll = Some(now + DISCOVERY_INTERVAL);
		}
		previous != self.sample
	}
}

#[cfg(test)]
#[path = "../../tests/unit/gamepad_owner.rs"]
mod tests;
