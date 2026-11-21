use std::time::Instant;

#[derive(Default)]
pub(crate) struct FrameMetrics {
	last: Option<Instant>,
	samples: u32,
	elapsed: f32,
	update: f32,
	pub frame_ms: f32,
	pub cpu_ms: f32,
}

impl FrameMetrics {
	pub fn record(&mut self, now: Instant, update_seconds: f32) {
		if let Some(last) = self.last.replace(now) {
			let delta = now.duration_since(last).as_secs_f32();
			// Focus/minimization stalls are not normal frame samples.
			if delta > 0.25 {
				self.samples = 0;
				self.elapsed = 0.0;
				self.update = 0.0;
				return;
			}
			self.samples += 1;
			self.elapsed += delta;
			self.update += update_seconds;
			if self.elapsed >= 0.5 {
				self.frame_ms = self.elapsed * 1000.0 / self.samples as f32;
				self.cpu_ms = self.update * 1000.0 / self.samples as f32;
				self.samples = 0;
				self.elapsed = 0.0;
				self.update = 0.0;
			}
		}
	}
}
