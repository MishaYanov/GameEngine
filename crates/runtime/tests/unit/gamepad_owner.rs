use super::*;

const ACTIVE: GamepadSample = GamepadSample {
	left_x: 18_000,
	left_y: 0,
	jump: true,
	interact: false,
};
const CENTERED: GamepadSample = GamepadSample {
	left_x: 1_000,
	left_y: -2_000,
	jump: false,
	interact: false,
};

#[test]
fn focus_reset_requires_neutral_then_a_new_sample() {
	let mut owner = GamepadOwner::default();
	let now = Instant::now();
	assert!(!owner.poll(now, |_| Some(ACTIVE)));
	assert!(!owner.poll(now + CONNECTED_INTERVAL, |_| Some(CENTERED)));
	assert!(owner.poll(now + CONNECTED_INTERVAL * 2, |_| Some(ACTIVE)));
	assert_eq!(owner.sample(), ACTIVE);
	owner.reset();
	assert!(owner.sample().is_neutral());
	assert!(!owner.poll(now + CONNECTED_INTERVAL * 3, |_| Some(ACTIVE)));
	assert!(!owner.poll(now + CONNECTED_INTERVAL * 4, |_| Some(CENTERED)));
	assert!(owner.poll(now + CONNECTED_INTERVAL * 5, |_| Some(ACTIVE)));
}

#[test]
fn disconnect_clears_the_old_sample_and_scans_on_a_later_deadline() {
	let mut owner = GamepadOwner::default();
	let now = Instant::now();
	assert!(!owner.poll(now, |slot| (slot == 2).then_some(CENTERED)));
	let now = now + CONNECTED_INTERVAL;
	assert!(owner.poll(now, |slot| {
		assert_eq!(slot, 2);
		Some(ACTIVE)
	}));
	let now = now + CONNECTED_INTERVAL;
	assert!(owner.poll(now, |slot| {
		assert_eq!(slot, 2);
		None
	}));
	assert!(owner.sample().is_neutral());
	assert_eq!(owner.deadline(), Some(now + DISCOVERY_INTERVAL));
	assert!(!owner.poll(now + CONNECTED_INTERVAL, |_| panic!("polled too early")));
	assert!(!owner.poll(now + DISCOVERY_INTERVAL, |_| Some(ACTIVE)));
	assert!(owner.sample().is_neutral());
}

#[test]
fn centered_noise_does_not_invalidate_the_current_sample() {
	let mut owner = GamepadOwner::default();
	let now = Instant::now();
	assert!(!owner.poll(now, |_| Some(CENTERED)));
	assert!(!owner.poll(now + CONNECTED_INTERVAL, |_| Some(CENTERED)));
	assert_eq!(owner.sample(), GamepadSample::default());
}
