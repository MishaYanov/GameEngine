//! Normalized, bounded orbit input shared by native hosts and fixed-step simulation.
use std::f64::consts::{LN_2, PI, TAU};

const MAX_LOG_DISTANCE_DELTA: f64 = 2.0 * LN_2; // ln(4): at most fourfold zoom.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CameraInputOwner {
	pub run_id: String,
	pub scene_generation: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraIntent {
	pub yaw_delta: f64,
	pub pitch_delta: f64,
	pub log_distance_delta: f64,
}

impl CameraIntent {
	pub fn validate(self) -> Result<(), String> {
		self.validate_bounds(PI)
	}

	/// Several individually valid samples can add up to one full turn in a step.
	pub(crate) fn validate_step(self) -> Result<(), String> {
		self.validate_bounds(TAU)
	}

	fn validate_bounds(self, max_angle: f64) -> Result<(), String> {
		let values = [self.yaw_delta, self.pitch_delta, self.log_distance_delta];
		if !values.into_iter().all(f64::is_finite) {
			return Err("Camera input must contain only finite values".into());
		}
		if self.yaw_delta.abs() > max_angle || self.pitch_delta.abs() > max_angle {
			return Err(format!(
				"Camera input cannot exceed {max_angle} radians per axis"
			));
		}
		if self.log_distance_delta.abs() > MAX_LOG_DISTANCE_DELTA {
			return Err("Camera input cannot change distance by over 4x".into());
		}
		Ok(())
	}

	pub fn is_empty(self) -> bool {
		self == Self::default()
	}
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CameraInputLatch {
	pending: CameraIntent,
}

impl CameraInputLatch {
	pub fn submit(&mut self, input: CameraIntent) -> Result<(), String> {
		input.validate()?; // Validate the whole sample before any mutation.
		self.pending = CameraIntent {
			yaw_delta: (self.pending.yaw_delta + input.yaw_delta).clamp(-TAU, TAU),
			pitch_delta: (self.pending.pitch_delta + input.pitch_delta).clamp(-TAU, TAU),
			log_distance_delta: (self.pending.log_distance_delta + input.log_distance_delta)
				.clamp(-MAX_LOG_DISTANCE_DELTA, MAX_LOG_DISTANCE_DELTA),
		};
		Ok(())
	}

	pub fn take_for_step(&mut self) -> CameraIntent {
		std::mem::take(&mut self.pending)
	}

	pub fn has_pending(&self) -> bool {
		!self.pending.is_empty()
	}

	pub fn snapshot(&self) -> CameraIntent {
		self.pending
	}

	pub fn clear(&mut self) {
		self.pending = CameraIntent::default();
	}
}

#[derive(Debug, Clone, Copy)]
pub struct CameraInputPreferences {
	pub radians_per_count: f64,
	pub invert_y: bool,
}

impl Default for CameraInputPreferences {
	fn default() -> Self {
		Self {
			radians_per_count: 0.002,
			invert_y: false,
		}
	}
}

pub fn normalize_mouse(
	dx: f64,
	dy: f64,
	preferences: CameraInputPreferences,
) -> Result<CameraIntent, String> {
	let sensitivity = preferences.radians_per_count;
	if !dx.is_finite()
		|| !dy.is_finite()
		|| !sensitivity.is_finite()
		|| !(0.0..=0.1).contains(&sensitivity)
	{
		return Err("Invalid native camera mouse sample or sensitivity".into());
	}
	// Bound BEFORE multiplying; an arbitrary finite f64 can still overflow a product.
	let native_limit = 1_000_000.0;
	let yaw_delta = (dx.clamp(-native_limit, native_limit) * sensitivity).clamp(-PI, PI);
	let pitch_sign = if preferences.invert_y { 1.0 } else { -1.0 };
	let pitch_delta =
		(dy.clamp(-native_limit, native_limit) * sensitivity * pitch_sign).clamp(-PI, PI);
	let intent = CameraIntent {
		yaw_delta,
		pitch_delta,
		log_distance_delta: 0.0,
	};
	intent.validate()?;
	Ok(intent)
}

#[derive(Debug, Clone, Copy)]
pub enum WheelSample {
	Lines(f64),
	Pixels(f64),
}

pub fn normalize_wheel(sample: WheelSample) -> Result<CameraIntent, String> {
	let lines = match sample {
		WheelSample::Lines(y) => y,
		// Product policy, not an OS promise that forty pixels always means one line.
		WheelSample::Pixels(y) => y / 40.0,
	};
	if !lines.is_finite() {
		return Err("Invalid native camera wheel sample".into());
	}
	// Positive wheel brings the camera closer; each line changes scale by 1.1x.
	let log_distance_delta = (-lines.clamp(-100.0, 100.0) * 1.1_f64.ln())
		.clamp(-MAX_LOG_DISTANCE_DELTA, MAX_LOG_DISTANCE_DELTA);
	let intent = CameraIntent {
		log_distance_delta,
		..CameraIntent::default()
	};
	intent.validate()?;
	Ok(intent)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn yaw(value: f64) -> CameraIntent {
		CameraIntent {
			yaw_delta: value,
			..CameraIntent::default()
		}
	}

	#[test]
	fn accumulates_until_a_step_and_consumes_exactly_once() {
		let mut latch = CameraInputLatch::default();
		latch.submit(yaw(0.02)).unwrap();
		assert!(latch.has_pending()); // A redraw with no step does nothing here.
		latch.submit(yaw(0.03)).unwrap();
		assert!((latch.take_for_step().yaw_delta - 0.05).abs() < 1e-12);
		assert_eq!(latch.take_for_step(), CameraIntent::default());
		assert!(!latch.has_pending());
	}

	#[test]
	fn invalid_sample_does_not_partially_mutate() {
		let mut latch = CameraInputLatch::default();
		latch.submit(yaw(0.25)).unwrap();
		let before = latch.clone();
		for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
			let input = CameraIntent {
				yaw_delta: 0.5,
				pitch_delta: 0.1,
				log_distance_delta: invalid,
			};
			assert!(latch.submit(input).is_err());
			assert_eq!(latch, before);
		}
		assert!(latch.submit(yaw(PI + 0.01)).is_err());
		assert_eq!(latch, before);
	}

	#[test]
	fn aggregate_has_a_separate_finite_bound() {
		let mut latch = CameraInputLatch::default();
		for _ in 0..100 {
			latch.submit(yaw(PI)).unwrap();
		}
		assert_eq!(latch.snapshot().yaw_delta, TAU);
		latch.submit(yaw(-PI)).unwrap();
		assert_eq!(latch.take_for_step().yaw_delta, PI);
	}

	#[test]
	fn transaction_candidate_does_not_consume_live_input() {
		let mut live = CameraInputLatch::default();
		live.submit(yaw(0.5)).unwrap();
		let mut candidate = live.clone();
		assert_eq!(candidate.take_for_step().yaw_delta, 0.5);
		assert_eq!(live.snapshot().yaw_delta, 0.5);
		// On aggregate rejection the session explicitly pauses and clears live.
		live.clear();
		assert!(!live.has_pending());
	}
	#[test]
	fn mouse_uses_displacement_and_pitch_convention() {
		let prefs = CameraInputPreferences::default();
		let normal = normalize_mouse(10.0, 20.0, prefs).unwrap();
		assert!((normal.yaw_delta - 0.02).abs() < 1e-12);
		assert!((normal.pitch_delta + 0.04).abs() < 1e-12);
		let inverted = normalize_mouse(
			10.0,
			20.0,
			CameraInputPreferences {
				invert_y: true,
				..prefs
			},
		)
			.unwrap();
		assert_eq!(inverted.pitch_delta, -normal.pitch_delta);
		assert!(normalize_mouse(f64::NAN, 0.0, prefs).is_err());
	}

	#[test]
	fn wheel_units_have_the_documented_ratio() {
		let line = normalize_wheel(WheelSample::Lines(1.0)).unwrap();
		let pixel = normalize_wheel(WheelSample::Pixels(40.0)).unwrap();
		assert_eq!(line, pixel);
		assert!((line.log_distance_delta.exp() - 1.0 / 1.1).abs() < 1e-12);
		assert!(normalize_wheel(WheelSample::Lines(f64::INFINITY)).is_err());
	}
}
