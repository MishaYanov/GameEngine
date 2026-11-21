use super::{CameraIntent, MAX_LOG_DISTANCE_DELTA};
use std::f64::consts::PI;

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
