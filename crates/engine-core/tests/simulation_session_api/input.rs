use super::*;

#[test]
fn idle_pause_and_input_release_discard_partial_ticks_and_do_not_replay_elapsed_time() {
	let data = Arc::new(scene(vec![entity("actor", [0.0; 3])]));
	let mut session = SimulationSession::new(
		project(&data, vec![controlled("actor", 1.0, BehaviorSpace::World)]),
		data,
	)
	.unwrap();
	session.tick(60.0).unwrap();
	session.set_controlled_input([1.0, 0.0, 0.0]).unwrap();
	assert_eq!(session.tick(1.0 / 120.0).unwrap().steps, 0);
	session.set_paused(true);
	assert!(!session.needs_tick());
	assert_eq!(session.tick(60.0).unwrap().steps, 0);
	session.set_paused(false);
	assert!(!session.needs_tick());
	session.set_controlled_input([1.0, 0.0, 0.0]).unwrap();
	assert_eq!(session.tick(1.0 / 120.0).unwrap().steps, 0);
	assert_eq!(session.tick(1.0 / 120.0).unwrap().steps, 1);
	close(
		session.document().entities[0].transform.position[0],
		1.0 / 60.0,
	);
	session.set_controlled_input([0.0; 3]).unwrap();
	session.tick(60.0).unwrap();
	session.set_controlled_input([1.0, 0.0, 0.0]).unwrap();
	assert_eq!(session.tick(1.0 / 120.0).unwrap().steps, 0);
	let outcome = session.tick(1.0).unwrap();
	assert_eq!(outcome.steps, 8);
	assert!(outcome.dropped_seconds > 0.8);
}

#[test]
fn invalid_input_and_elapsed_are_atomic_and_extreme_finite_input_is_normalized() {
	let data = Arc::new(scene(vec![entity("actor", [0.0; 3])]));
	let mut session = SimulationSession::new(
		project(&data, vec![controlled("actor", 1.0, BehaviorSpace::World)]),
		Arc::clone(&data),
	)
	.unwrap();
	assert!(session.set_input(&key("actor"), [f32::NAN; 3]).is_err());
	assert!(!session.needs_tick());
	for elapsed in [-1.0, f64::NAN, f64::INFINITY] {
		assert!(session.tick(elapsed).is_err());
		assert_eq!(session.document(), data);
		assert_eq!(session.tick_index(), 0);
	}
	session
		.set_input(&key("actor"), [f32::MAX, f32::MAX, 0.0])
		.unwrap();
	session.tick(0.1).unwrap();
	let position = glam::Vec3::from_array(session.document().entities[0].transform.position);
	close(position.length(), 0.1);
}

#[test]
fn broadcast_input_preserves_target_order_and_partial_ticks_for_unchanged_or_held_motion() {
	let data = Arc::new(scene(vec![entity("a", [0.0; 3]), entity("b", [0.0; 3])]));
	let mut session = SimulationSession::new(
		project(
			&data,
			vec![
				controlled("b", 1.0, BehaviorSpace::World),
				controlled("a", 1.0, BehaviorSpace::World),
				controlled("b", 0.0, BehaviorSpace::World),
			],
		),
		data,
	)
	.unwrap();
	assert_eq!(session.controlled_targets(), &[key("b"), key("a")]);
	assert!(!session.set_controlled_input([0.0; 3]).unwrap());
	assert!(session.set_input(&key("b"), [1.0, 0.0, 0.0]).unwrap());
	assert!(session.set_controlled_input([1.0, 0.0, 0.0]).unwrap());
	assert_eq!(session.tick(1.0 / 120.0).unwrap().steps, 0);
	assert!(!session.set_controlled_input([1.0, 0.0, 0.0]).unwrap());
	assert_eq!(session.tick(1.0 / 120.0).unwrap().steps, 1);
	assert_eq!(session.tick(1.0 / 120.0).unwrap().steps, 0);
	assert!(session.set_controlled_input([0.0, 1.0, 0.0]).unwrap());
	assert_eq!(session.tick(1.0 / 120.0).unwrap().steps, 1);
	for entity in &session.document().entities {
		close(entity.transform.position[0], 1.0 / 60.0);
		close(entity.transform.position[1], 1.0 / 60.0);
	}
	assert!(session.set_input(&key("unknown"), [1.0, 0.0, 0.0]).is_err());
	assert!(session.set_controlled_input([0.0; 3]).unwrap());
	assert!(!session.set_controlled_input([0.0; 3]).unwrap());
	assert!(!session.needs_tick());
}

#[test]
fn configured_scheduler_distinguishes_idle_input_disabled_controls_and_nonzero_spins() {
	for (speed, angular_velocity, idle_ticks, active_ticks) in [
		(0.0, 0.0, false, false),
		(1.0, 0.0, false, true),
		(0.0, 1.0, true, true),
	] {
		let data = Arc::new(scene(vec![entity("actor", [0.0; 3])]));
		let mut session = SimulationSession::new(
			project(
				&data,
				vec![
					controlled("actor", speed, BehaviorSpace::World),
					BehaviorSpec::Spin {
						entity: key("actor"),
						angular_velocity: [0.0, angular_velocity, 0.0],
					},
				],
			),
			data,
		)
		.unwrap();
		assert_eq!(session.needs_tick(), idle_ticks);
		session.set_controlled_input([1.0, 0.0, 0.0]).unwrap();
		assert_eq!(session.needs_tick(), active_ticks);
		session.clear_input();
		assert_eq!(session.needs_tick(), idle_ticks);
		session.set_paused(true);
		assert!(!session.needs_tick());
	}
}
