use super::*;

#[test]
fn character_jump_and_landing_arrive_once_on_next_step_with_self_machine_identity() {
	let (project, data) = character_fixture();
	let mut simulation = SimulationSession::new(project, data).unwrap();
	step(&mut simulation);
	assert_eq!(snapshot(&simulation)["event_sequence"], 0);
	simulation
		.set_character_input(&key("player"), [0.; 2], true)
		.unwrap();
	step(&mut simulation);
	assert_eq!(state(&simulation, "player").as_deref(), Some("ready"));
	let pending = simulation.native_replay_state()["lifecycle"]["events"]["pending"].clone();
	assert_eq!(pending.as_array().unwrap().len(), 1);
	assert_eq!(pending[0]["signal"], "jumped");
	assert_eq!(pending[0]["due_tick"], 3);
	assert_eq!(pending[0]["source"], pending[0]["target"]);
	assert_eq!(pending[0]["source"]["entity"], "player");
	assert_eq!(pending[0]["source"]["component"], "logic");
	step(&mut simulation);
	assert_eq!(state(&simulation, "player").as_deref(), Some("air"));
	let mut landed = None;
	for _ in 0..180 {
		step(&mut simulation);
		if let Some(tick) = simulation.character_motion()[0].landed_at_tick {
			landed = Some(tick);
			break;
		}
	}
	let landed = landed.expect("jump must land");
	assert_eq!(simulation.tick_index(), landed);
	assert_eq!(state(&simulation, "player").as_deref(), Some("air"));
	step(&mut simulation);
	assert_eq!(state(&simulation, "player").as_deref(), Some("landed"));
	for _ in 0..30 {
		step(&mut simulation);
	}
	assert_eq!(snapshot(&simulation)["event_sequence"], 2);
	assert_eq!(simulation.character_motion()[0].jumped_at_tick, Some(2));
	assert_eq!(
		simulation.character_motion()[0].landed_at_tick,
		Some(landed)
	);
}

#[test]
fn late_failure_rolls_back_motion_signal_production_delivery_and_motion_timestamps() {
	let (project, data) = character_fixture();
	let mut simulation = SimulationSession::with_factory(
		project.clone(),
		data.clone(),
		&FailLate {
			calls: Arc::new(AtomicUsize::new(0)),
			fail_at: 4,
		},
	)
	.unwrap();
	step(&mut simulation);
	let before = simulation.native_replay_state();
	simulation
		.set_character_input(&key("player"), [0.; 2], true)
		.unwrap();
	assert!(simulation.tick(3. / 60.).is_err());
	let rejected = simulation.native_replay_state();
	assert_eq!(rejected["lifecycle"], before["lifecycle"]);
	assert_eq!(
		rejected["characters"]["states"],
		before["characters"]["states"]
	);
	assert_eq!(simulation.tick_index(), 1);
	assert_eq!(simulation.character_motion()[0].jumped_at_tick, None);
	simulation.set_paused(false);
	simulation
		.set_character_input(&key("player"), [0.; 2], true)
		.unwrap();
	simulation.tick(3. / 60.).unwrap();
	assert_eq!(state(&simulation, "player").as_deref(), Some("air"));
	assert_eq!(snapshot(&simulation)["event_sequence"], 1);
	let restarted = SimulationSession::new(project, data).unwrap();
	assert_eq!(
		restarted.native_replay_state()["lifecycle"]["events"]["sequence"],
		0
	);
	assert_eq!(restarted.character_motion()[0].jumped_at_tick, None);
	assert_eq!(state(&restarted, "player"), None);
}

#[test]
fn controller_adapter_and_machine_effects_share_the_same_step_emission_cap() {
	let (project, data) = character_fixture();
	let mut data = (*data).clone();
	for index in 0..64 {
		let name = format!("emitter-{index}");
		let mut owner = entity(&name, [50., 0., 0.]);
		owner.native_behaviors.push(machine(
			"logic",
			vec![
				MachineAction::Emit {
					target: ComponentRef {
						entity: key(&name),
						component: ComponentId::new("logic").unwrap()
					},
					signal: Signal::Activated
				};
				8
			],
		));
		data.entities.push(owner);
	}
	let mut simulation = SimulationSession::new(project, Arc::new(data)).unwrap();
	let before = simulation.native_replay_state();
	simulation
		.set_character_input(&key("player"), [0.; 2], true)
		.unwrap();
	assert!(
		simulation
			.tick(1. / 60.)
			.unwrap_err()
			.contains("event budget")
	);
	assert_eq!(
		simulation.native_replay_state()["lifecycle"],
		before["lifecycle"]
	);
	assert_eq!(simulation.tick_index(), 0);
	assert_eq!(simulation.character_motion()[0].jumped_at_tick, None);
}
