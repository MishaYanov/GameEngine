//! Bounded fixed-step scheduling, independent of rendering frequency.
#[derive(Debug, Clone)]
pub struct FixedStepper {
	step: f64,
	accumulator: f64,
	max_steps: u32,
}
#[derive(Debug, Clone, Copy, Default)]
pub struct StepReport {
	pub steps: u32,
	pub step_seconds: f32,
	pub interpolation: f32,
	pub dropped_seconds: f64,
}
impl Default for FixedStepper {
	fn default() -> Self {
		Self {
			step: 1.0 / 60.0,
			accumulator: 0.0,
			max_steps: 8,
		}
	}
}
impl FixedStepper {
	pub fn new(hz: u32, max_steps: u32) -> Result<Self, String> {
		if hz == 0 || hz > 1000 || max_steps == 0 || max_steps > 8 {
			return Err("Fixed-step rate or catch-up budget is out of range".into());
		}
		Ok(Self {
			step: 1.0 / hz as f64,
			accumulator: 0.0,
			max_steps,
		})
	}
	pub fn reset(&mut self) {
		self.accumulator = 0.0;
	}
	pub fn advance(&mut self, elapsed: f64) -> Result<StepReport, String> {
		if !elapsed.is_finite() || elapsed < 0.0 {
			return Err("Elapsed time must be finite and nonnegative".into());
		}
		let accepted = elapsed.min(0.25);
		self.accumulator += accepted;
		let available = ((self.accumulator + 1e-12) / self.step).floor() as u32;
		let steps = available.min(self.max_steps);
		self.accumulator = (self.accumulator - available as f64 * self.step).max(0.0);
		Ok(StepReport {
			steps,
			step_seconds: self.step as f32,
			// Converting a fraction just below one to f32 can round up to
			// exactly one; keep the documented interpolation interval [0,1).
			interpolation: ((self.accumulator / self.step) as f32)
				.min(f32::from_bits(1.0_f32.to_bits() - 1)),
			dropped_seconds: elapsed - accepted + (available - steps) as f64 * self.step,
		})
	}
}
