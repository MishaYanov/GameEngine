use super::*;

#[test]
fn short_press_survives_zero_steps_and_hold_never_rejumps() {
	let mut s = session(room(vec![]));
	tick(&mut s);
	s.set_character_input(&key("player"), [0.; 2], true)
		.unwrap();
	s.set_character_input(&key("player"), [0.; 2], false)
		.unwrap();
	assert_eq!(s.tick(0.001).unwrap().steps, 0);
	s.tick(1. / 60. - 0.001).unwrap();
	assert_eq!(s.character_motion()[0].jumped_at_tick, Some(2));
	assert!(position(&s)[1] > 1.);
	s.set_character_input(&key("player"), [0.; 2], true)
		.unwrap();
	for _ in 0..120 {
		tick(&mut s);
	}
	assert_eq!(s.character_motion()[0].jumped_at_tick, Some(2));
	assert!(s.character_motion()[0].landed_at_tick.is_some());
	assert!(s.character_motion()[0].grounded);
}

#[test]
fn press_is_not_replayed_across_catchup_and_pause_discards_it() {
	let data = room(vec![]);
	let mut a = session(data.clone());
	let mut b = session(data);
	tick(&mut a);
	tick(&mut b);
	for s in [&mut a, &mut b] {
		s.set_character_input(&key("player"), [1., 0.], true)
			.unwrap();
	}
	a.tick(4. / 60.).unwrap();
	for _ in 0..4 {
		tick(&mut b);
	}
	assert_eq!(a.world().data(), b.world().data());
	assert_eq!(
		serde_json::to_value(a.character_motion()[0].clone()).unwrap()["jumped_at_tick"],
		serde_json::json!(2)
	);
	let mut paused = session(room(vec![]));
	tick(&mut paused);
	paused
		.set_character_input(&key("player"), [0.; 2], true)
		.unwrap();
	paused.set_paused(true);
	paused.set_paused(false);
	tick(&mut paused);
	assert_eq!(paused.character_motion()[0].jumped_at_tick, None);
}

#[test]
fn native_code_and_player_event_samples_capture_short_taps_identically() {
	use engine_core::{
		InputState,
		command::{EditorCommandDto as C, EditorCommandRequest},
		editor::EditorSession,
		simulation_session::MovementTarget,
	};
	let data = room(vec![]);
	let p = project(&data);
	let mut a = EditorSession::for_project(data.clone(), p.clone()).unwrap();
	let mut b = EditorSession::for_project(data, p.clone()).unwrap();
	for e in [&mut a, &mut b] {
		e.execute_command(EditorCommandRequest {
			expected_revision: e.revision(),
			command: C::StartPlay,
		})
		.unwrap();
		e.tick(1. / 60.).unwrap();
	}
	a.set_game_keyboard_input_from_codes(&["Space".into(), "KeyW".into()].into_iter().collect())
		.unwrap();
	a.set_game_keyboard_input_from_codes(&["KeyW".into()].into_iter().collect())
		.unwrap();
	let mut input = InputState::default();
	input.press(Key::W);
	input.press(Key::Space);
	b.set_game_keyboard_input(
		&MovementTarget::AllControlled,
		p.input_profile.as_ref().unwrap().resolve_keyboard(&input),
	)
	.unwrap();
	input.release(Key::Space);
	b.set_game_keyboard_input(
		&MovementTarget::AllControlled,
		p.input_profile.as_ref().unwrap().resolve_keyboard(&input),
	)
	.unwrap();
	a.tick(4. / 60.).unwrap();
	for _ in 0..4 {
		b.tick(1. / 60.).unwrap();
	}
	assert_eq!(
		a.render_world().unwrap().data(),
		b.render_world().unwrap().data()
	);
	assert_eq!(
		a.runtime_inspection(0, 128).unwrap().character_motion[0].jumped_at_tick,
		Some(2)
	);
	assert!(
		a.render_world().unwrap().data().entities[0]
			.transform
			.position[2]
			< 0.
	);
}

#[test]
fn keyboard_targets_do_not_cross_controller_and_legacy_axes_or_owners() {
	use engine_core::{
		InputState,
		command::{EditorCommandDto as C, EditorCommandRequest},
		editor::EditorSession,
		simulation_session::MovementTarget,
	};
	let mut other = player();
	other.key = key("other");
	other.transform.position[0] = 4.;
	let data = room(vec![other, entity("legacy", [8., 3., 0.])]);
	let mut p = project(&data);
	p.add_behavior(BehaviorSpec::ControlledMove {
		entity: key("legacy"),
		speed: 1.,
		space: BehaviorSpace::World,
	});
	let mut e = EditorSession::for_project(data, p.clone()).unwrap();
	e.execute_command(EditorCommandRequest {
		expected_revision: e.revision(),
		command: C::StartPlay,
	})
	.unwrap();
	e.tick(1. / 60.).unwrap();
	let mut input = InputState::default();
	input.press(Key::W);
	input.press(Key::Space);
	e.set_game_keyboard_input(
		&MovementTarget::Entity(key("player")),
		p.input_profile.as_ref().unwrap().resolve_keyboard(&input),
	)
	.unwrap();
	e.tick(1. / 60.).unwrap();
	let world = e.render_world().unwrap();
	assert!(world.data().entities[0].transform.position[2] < 0.);
	assert_eq!(world.data().entities[2].transform.position[2], 0.);
	assert_eq!(world.data().entities[3].transform.position[1], 3.);
	e.set_game_keyboard_input(
		&MovementTarget::AllControlled,
		p.input_profile.as_ref().unwrap().resolve_keyboard(&input),
	)
	.unwrap();
	e.tick(1. / 60.).unwrap();
	let world = e.render_world().unwrap();
	assert!(world.data().entities[2].transform.position[2] < 0.);
	assert!(world.data().entities[3].transform.position[1] > 3.);
}

#[test]
fn focus_neutralization_discards_held_edges_without_suppressing_a_fresh_action() {
	use engine_core::{
		InputState,
		command::{EditorCommandDto as C, EditorCommandRequest},
		editor::EditorSession,
		simulation_session::MovementTarget,
	};
	for held_at_loss in [false, true] {
		let data = room(vec![]);
		let p = project(&data);
		let mut e = EditorSession::for_project(data, p.clone()).unwrap();
		e.execute_command(EditorCommandRequest {
			expected_revision: e.revision(),
			command: C::StartPlay,
		})
		.unwrap();
		e.tick(1. / 60.).unwrap();
		let mut input = InputState::default();
		if held_at_loss {
			input.press(Key::Space);
		}
		e.set_game_keyboard_input(
			&MovementTarget::AllControlled,
			p.input_profile.as_ref().unwrap().resolve_keyboard(&input),
		)
		.unwrap();
		input.suppress_held();
		e.clear_play_input();
		if held_at_loss {
			input.press(Key::Space); // focus restoration/repeat cannot recreate the held edge
			e.set_game_keyboard_input(
				&MovementTarget::AllControlled,
				p.input_profile.as_ref().unwrap().resolve_keyboard(&input),
			)
			.unwrap();
			e.tick(1. / 60.).unwrap();
			assert_eq!(
				e.runtime_inspection(0, 128).unwrap().character_motion[0].jumped_at_tick,
				None
			);
			input.release(Key::Space); // an observed release (including synthetic OS release) re-arms
		}
		input.press(Key::Space);
		e.set_game_keyboard_input(
			&MovementTarget::AllControlled,
			p.input_profile.as_ref().unwrap().resolve_keyboard(&input),
		)
		.unwrap();
		e.tick(1. / 60.).unwrap();
		assert!(
			e.runtime_inspection(0, 128).unwrap().character_motion[0]
				.jumped_at_tick
				.is_some()
		);
	}
}

#[test]
fn short_jump_survives_spawn_admission_drain_without_being_replayed() {
	use engine_core::{
		components::{NativeBehaviorComponent, NativeBehaviorSpec},
		editor::PrefabData,
	};
	let mut spawner = entity("spawner", [8., 0., 0.]);
	spawner.native_behaviors.push(NativeBehaviorComponent {
		id: ComponentId::new("spawn").unwrap(),
		enabled: true,
		behavior: NativeBehaviorSpec::TimedSpawner {
			prefab_path: "prefabs/item.json".into(),
			interval_ticks: 1,
			max_alive: 1,
			lifetime_ticks: 1000,
			spawn_offset: [0.; 3],
			bindings: Default::default(),
		},
	});
	let mut data = room(vec![spawner]);
	data.entities[0]
		.character_controller
		.as_mut()
		.unwrap()
		.settings
		.jump_buffer_ticks = 120;
	let mut whole = session(data.clone());
	let mut split = session(data);
	let item = PrefabData {
		schema_version: 3,
		composition: Default::default(),
		entities: vec![entity("item", [0.; 3])],
		gameplay: None,
		reference_slots: Default::default(),
	};
	fn frame(s: &mut SimulationSession, item: &PrefabData, mut elapsed: f64) -> u32 {
		let mut steps = 0;
		for _ in 0..8 {
			let report = s.tick(elapsed).unwrap();
			steps += report.steps;
			for request in s.pending_runtime_spawns() {
				let candidate = s
					.prepare_runtime_spawn(
						item,
						&"a".repeat(64),
						request.position,
						Some(request.id),
					)
					.unwrap();
				s.commit_runtime_spawn(candidate).unwrap();
			}
			if report.pending_steps == 0 {
				return steps;
			}
			elapsed = 0.;
		}
		panic!("unbounded drain");
	}
	for s in [&mut whole, &mut split] {
		s.set_character_input(&key("player"), [0.; 2], true)
			.unwrap();
		s.set_character_input(&key("player"), [0.; 2], false)
			.unwrap();
	}
	assert_eq!(frame(&mut whole, &item, 4. / 60.), 4);
	for _ in 0..4 {
		assert_eq!(frame(&mut split, &item, 1. / 60.), 1);
	}
	assert_eq!(whole.world().data(), split.world().data());
	for _ in 0..120 {
		frame(&mut whole, &item, 1. / 60.);
		frame(&mut split, &item, 1. / 60.);
	}
	assert_eq!(whole.world().data(), split.world().data());
	assert_eq!(whole.character_motion()[0].jumped_at_tick, Some(1));
	assert!(whole.character_motion()[0].grounded);
}
