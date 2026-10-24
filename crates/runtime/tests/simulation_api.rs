use runtime::simulation::FixedStepper;

#[test]
fn fixed_ticks_are_independent_of_frame_partition_and_preserve_fractional_time() {
	let mut whole = FixedStepper::new(60, 8).unwrap();
	let mut split = whole.clone();
	let expected = whole.advance(0.1).unwrap();
	let steps = (0..12)
		.map(|_| split.advance(1.0 / 120.0).unwrap().steps)
		.sum::<u32>();
	assert_eq!(expected.steps, 6);
	assert_eq!(steps, 6);
	assert_eq!(split.advance(0.0).unwrap().interpolation, 0.0);
	let partial = split.advance(1.0 / 120.0).unwrap();
	assert_eq!(partial.steps, 0);
	assert!((partial.interpolation - 0.5).abs() < 1e-6);
	assert_eq!(split.advance(1.0 / 120.0).unwrap().steps, 1);
}
#[test]
fn catchup_budget_reports_discarded_time_without_accumulating_a_backlog() {
	let mut clock = FixedStepper::new(100, 3).unwrap();
	let report = clock.advance(1.0).unwrap();
	assert_eq!(report.steps, 3);
	assert!((report.dropped_seconds - 0.97).abs() < 1e-9);
	assert_eq!(clock.advance(0.0).unwrap().steps, 0);
	assert_eq!(clock.advance(0.01).unwrap().steps, 1);
}
#[test]
fn invalid_elapsed_is_atomic_and_reset_discards_partial_ticks() {
	let mut clock = FixedStepper::new(10, 8).unwrap();
	clock.advance(0.05).unwrap();
	for invalid in [-0.1, f64::NAN, f64::INFINITY] {
		assert!(clock.advance(invalid).is_err());
	}
	assert_eq!(clock.advance(0.05).unwrap().steps, 1);
	clock.advance(0.05).unwrap();
	clock.reset();
	assert_eq!(clock.advance(0.05).unwrap().steps, 0);
}
#[test]
fn fixed_step_configuration_is_bounded() {
	for (hz, budget) in [(0, 8), (1001, 8), (60, 0), (60, 9)] {
		assert!(FixedStepper::new(hz, budget).is_err());
	}
	for (hz, budget) in [(1, 1), (1000, 8)] {
		assert!(FixedStepper::new(hz, budget).is_ok());
	}
}

#[test]
fn interpolation_fraction_never_rounds_up_to_one() {
	let mut clock = FixedStepper::new(1000, 8).unwrap();
	let report = clock.advance(0.001 * (1.0 - 1e-8)).unwrap();
	assert_eq!(report.steps, 0);
	assert!((0.0..1.0).contains(&report.interpolation));
}
