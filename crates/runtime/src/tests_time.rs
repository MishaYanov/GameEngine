use super::*;
use std::time::{Duration, Instant};

#[test]
fn first_tick_establishes_a_baseline_and_returns_zero() {
	let mut clock = FrameClock::default();
	let start = Instant::now();
	assert_eq!(clock.tick(start), Duration::ZERO);
	assert_eq!(
		clock.tick(start + Duration::from_millis(16)),
		Duration::from_millis(16)
	);
}

#[test]
fn consecutive_ticks_measure_each_frame_without_losing_submillisecond_precision() {
	let mut clock = FrameClock::default();
	let start = Instant::now();
	clock.tick(start);
	assert_eq!(
		clock.tick(start + Duration::from_micros(125)),
		Duration::from_micros(125)
	);
	assert_eq!(
		clock.tick(start + Duration::from_micros(375)),
		Duration::from_micros(250)
	);
}

#[test]
fn delta_is_capped_at_one_hundred_milliseconds() {
	let start = Instant::now();
	for (elapsed, expected) in [
		(
			Duration::from_millis(100) - Duration::from_nanos(1),
			Duration::from_millis(100) - Duration::from_nanos(1),
		),
		(Duration::from_millis(100), Duration::from_millis(100)),
		(
			Duration::from_millis(100) + Duration::from_nanos(1),
			Duration::from_millis(100),
		),
		(Duration::from_secs(60), Duration::from_millis(100)),
	] {
		let mut clock = FrameClock::default();
		clock.tick(start);
		assert_eq!(
			clock.tick(start + elapsed),
			expected,
			"elapsed: {elapsed:?}"
		);
	}
}

#[test]
fn a_long_pause_updates_the_baseline_instead_of_accumulating_backlog() {
	let mut clock = FrameClock::default();
	let start = Instant::now();
	let resumed = start + Duration::from_secs(10);
	clock.tick(start);
	assert_eq!(clock.tick(resumed), Duration::from_millis(100));
	assert_eq!(
		clock.tick(resumed + Duration::from_millis(16)),
		Duration::from_millis(16)
	);
}

#[test]
fn equal_or_backwards_timestamps_return_zero_and_rebase_the_next_tick() {
	let mut clock = FrameClock::default();
	let start = Instant::now();
	clock.tick(start + Duration::from_millis(20));
	assert_eq!(
		clock.tick(start + Duration::from_millis(20)),
		Duration::ZERO
	);
	assert_eq!(clock.tick(start), Duration::ZERO);
	assert_eq!(
		clock.tick(start + Duration::from_millis(5)),
		Duration::from_millis(5)
	);
}

#[test]
fn reset_discards_the_previous_frame_including_time_spent_paused() {
	let mut clock = FrameClock::default();
	let start = Instant::now();
	clock.reset();
	clock.tick(start);
	clock.tick(start + Duration::from_millis(16));
	clock.reset();
	clock.reset();
	let resumed = start + Duration::from_secs(60);
	assert_eq!(clock.tick(resumed), Duration::ZERO);
	assert_eq!(
		clock.tick(resumed + Duration::from_millis(8)),
		Duration::from_millis(8)
	);
}
