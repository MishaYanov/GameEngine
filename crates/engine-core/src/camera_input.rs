//! Normalized, bounded orbit input shared by native hosts and fixed-step simulation.
mod normalize;
pub use normalize::{CameraInputPreferences, WheelSample, normalize_mouse, normalize_wheel};

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

#[cfg(test)]
#[path = "../tests/unit/camera_input.rs"]
mod tests;
