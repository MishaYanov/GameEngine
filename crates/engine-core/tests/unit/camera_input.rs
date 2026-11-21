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
