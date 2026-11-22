use super::*;

#[test]
fn floor_rest_is_stable_and_walk_uses_xz() {
	let mut s = session(room(vec![]));
	for _ in 0..600 {
		tick(&mut s);
	}
	let resting = position(&s);
	assert!((resting[1] - 0.9001).abs() < 0.0002);
	assert!(s.character_motion()[0].grounded);
	s.set_character_input(&key("player"), [0., -1.], false)
		.unwrap();
	for _ in 0..60 {
		tick(&mut s);
	}
	assert!(position(&s)[2] < -3.5);
	assert!((position(&s)[1] - resting[1]).abs() < 0.0002);
}

#[test]
fn small_box_step_succeeds_but_tall_step_and_low_ceiling_block() {
	for (height, ceiling, should_pass) in
		[(0.2, false, true), (0.5, false, false), (0.2, true, false)]
	{
		let mut extra = vec![box_at(
			"step",
			[1.2, height / 2., 0.],
			[0.4, height / 2., 1.],
		)];
		if ceiling {
			extra.push(box_at("ceiling", [1.2, 2.05, 0.], [0.6, 0.05, 1.]));
		}
		let mut s = session(room(extra));
		s.set_character_input(&key("player"), [1., 0.], false)
			.unwrap();
		let mut highest = 0.0_f32;
		for _ in 0..60 {
			tick(&mut s);
			highest = highest.max(position(&s)[1]);
		}
		if should_pass {
			assert!(position(&s)[0] > 2., "{:?}", position(&s));
			assert!(highest > 1.09);
		} else {
			assert!(position(&s)[0] < 0.501, "{:?}", position(&s));
			assert!(highest < 0.91);
		}
	}
}

#[test]
fn sensor_floor_never_grounds_and_fall_speed_is_bounded() {
	let mut data = room(vec![]);
	if let Some(ColliderData::Box { interaction, .. }) = &mut data.entities[1].collider {
		interaction.sensor = true;
	}
	let mut s = session(data);
	for _ in 0..180 {
		tick(&mut s);
	}
	let state = &s.character_motion()[0];
	assert!(!state.grounded);
	assert_eq!(state.vertical_speed, -30.);
	assert!(position(&s)[1] < -40.);
}

#[test]
fn coyote_jump_and_buffered_landing_use_simulation_ticks() {
	let mut data = room(vec![]);
	data.entities[1] = box_at("floor", [-2., -0.5, 0.], [2.5, 0.5, 2.]);
	let mut s = session(data);
	s.set_character_input(&key("player"), [1., 0.], false)
		.unwrap();
	for _ in 0..60 {
		tick(&mut s);
		if !s.character_motion()[0].grounded {
			break;
		}
	}
	assert!(!s.character_motion()[0].grounded);
	s.set_character_input(&key("player"), [1., 0.], true)
		.unwrap();
	tick(&mut s);
	assert!(s.character_motion()[0].vertical_speed > 0.);
	assert_eq!(s.character_motion()[0].jumped_at_tick, Some(s.tick_index()));

	let mut data = room(vec![]);
	data.entities[0].transform.position[1] = 3.;
	let mut s = session(data);
	for _ in 0..120 {
		tick(&mut s);
		if position(&s)[1] < 1.2 {
			break;
		}
	}
	assert!(!s.character_motion()[0].grounded);
	s.set_character_input(&key("player"), [0.; 2], true)
		.unwrap();
	s.set_character_input(&key("player"), [0.; 2], false)
		.unwrap();
	for _ in 0..8 {
		tick(&mut s);
		if s.character_motion()[0].jumped_at_tick.is_some() {
			break;
		}
	}
	let motion = &s.character_motion()[0];
	assert_eq!(
		motion.jumped_at_tick,
		motion.landed_at_tick.map(|tick| tick + 1)
	);
	assert!(motion.jumped_at_tick.is_some());
}

#[test]
fn removed_or_disabled_support_is_cleared_before_publishing_motion() {
	for remove in [false, true] {
		let data = scene(vec![player()]);
		let script = BoundaryScript::default();
		let mut s =
			SimulationSession::with_factory(project(&data), Arc::new(data), &script).unwrap();
		let item = engine_core::editor::PrefabData {
			schema_version: 3,
			composition: Default::default(),
			entities: vec![box_at("floor", [0., -0.5, 0.], [20., 0.5, 20.])],
			gameplay: None,
			reference_slots: Default::default(),
		};
		let candidate = s
			.prepare_runtime_spawn(&item, &"a".repeat(64), [0.; 3], None)
			.unwrap();
		s.commit_runtime_spawn(candidate).unwrap();
		let support = s
			.world()
			.data()
			.entities
			.iter()
			.find(|e| e.key != key("player"))
			.unwrap()
			.key
			.clone();
		tick(&mut s);
		assert!(s.character_motion()[0].grounded);
		*script.0.lock().unwrap() = Some(if remove {
			BoundaryAction::Remove(s.runtime_handle(&support).unwrap())
		} else {
			BoundaryAction::Collider(support, false)
		});
		tick(&mut s);
		assert!(!s.character_motion()[0].grounded);
		*script.0.lock().unwrap() = None;
		tick(&mut s);
		assert!(s.character_motion()[0].vertical_speed < 0.);
	}
}

#[test]
fn coyote_deadline_is_inclusive_and_expired_buffer_cannot_jump_on_landing() {
	for (wait_ticks, allowed) in [(4, true), (5, false)] {
		let mut data = room(vec![]);
		data.entities[1] = box_at("floor", [-2., -0.5, 0.], [2.5, 0.5, 2.]);
		let mut s = session(data);
		s.set_character_input(&key("player"), [1., 0.], false)
			.unwrap();
		for _ in 0..60 {
			tick(&mut s);
			if !s.character_motion()[0].grounded {
				break;
			}
		}
		assert!(!s.character_motion()[0].grounded);
		for _ in 0..wait_ticks {
			tick(&mut s);
		}
		s.set_character_input(&key("player"), [1., 0.], true)
			.unwrap();
		tick(&mut s);
		assert_eq!(s.character_motion()[0].jumped_at_tick.is_some(), allowed);
	}
	let mut data = room(vec![]);
	data.entities[0].transform.position[1] = 3.;
	data.entities[0]
		.character_controller
		.as_mut()
		.unwrap()
		.settings
		.jump_buffer_ticks = 0;
	let mut s = session(data);
	s.set_character_input(&key("player"), [0.; 2], true)
		.unwrap();
	s.set_character_input(&key("player"), [0.; 2], false)
		.unwrap();
	for _ in 0..120 {
		tick(&mut s);
	}
	assert!(s.character_motion()[0].grounded);
	assert_eq!(s.character_motion()[0].jumped_at_tick, None);
}

#[test]
fn ceiling_contact_clips_upward_velocity_and_lands_once() {
	let mut s = session(room(vec![box_at("ceiling", [0., 2., 0.], [2., 0.1, 2.])]));
	tick(&mut s);
	s.set_character_input(&key("player"), [0.; 2], true)
		.unwrap();
	tick(&mut s);
	assert!(position(&s)[1] <= 1.0002);
	assert_eq!(s.character_motion()[0].vertical_speed, 0.);
	for _ in 0..60 {
		tick(&mut s);
		assert!(position(&s)[1] <= 1.0002);
	}
	let landed = s.character_motion()[0].landed_at_tick;
	assert!(landed.is_some());
	assert!(s.character_motion()[0].grounded);
	for _ in 0..60 {
		tick(&mut s);
	}
	assert_eq!(s.character_motion()[0].landed_at_tick, landed);
	assert_eq!(s.character_motion()[0].jumped_at_tick, Some(2));
}
