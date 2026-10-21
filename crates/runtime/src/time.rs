use std::time::{Duration, Instant};

#[derive(Default)]
pub struct FrameClock {
    previous: Option<Instant>,
}

impl FrameClock {
    pub fn tick(&mut self, now: Instant) -> Duration {
        let old_previous = self.previous;

        self.previous = Some(now);

        let Some(prev_time) = old_previous else {
            return Duration::ZERO;
        };

        let duration = now.saturating_duration_since(prev_time);

        duration.min(Duration::from_millis(100))
    }

    pub fn reset(&mut self) {
        self.previous = None;
    }
}

#[cfg(test)]
#[path = "tests_time.rs"]
mod tests;
