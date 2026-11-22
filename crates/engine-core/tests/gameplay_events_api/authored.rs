use super::*;

#[test]
fn actual_initial_overlap_latches_after_one_boundary_and_reaches_door_after_two() {
	let (project, data) = fixture();
	let saved = data.to_json().unwrap();
	let mut simulation = SimulationSession::new(project.clone(), data.clone()).unwrap();
	assert_eq!(state(&simulation, "a-left"), None);
	step(&mut simulation);
	assert_eq!(state(&simulation, "a-left").as_deref(), Some("ready"));
	assert_eq!(snapshot(&simulation)["pending_events"], 1);
	step(&mut simulation);
	assert_eq!(state(&simulation, "a-left").as_deref(), Some("latched"));
	assert_eq!(state(&simulation, "a-door").as_deref(), Some("closed"));
	step(&mut simulation);
	assert_eq!(state(&simulation, "a-door").as_deref(), Some("left_only"));
	assert_eq!(state(&simulation, "a-left").as_deref(), Some("latched"));
	assert_eq!(state(&simulation, "b-door").as_deref(), Some("closed"));
	assert_eq!(snapshot(&simulation)["pending_events"], 0);
	assert_eq!(snapshot(&simulation)["event_sequence"], 2);
	assert_eq!(saved, data.to_json().unwrap());
	let restarted = SimulationSession::new(project, data).unwrap();
	assert_eq!(snapshot(&restarted)["event_sequence"], 0);
	assert_eq!(state(&restarted, "a-left"), None);
}

#[test]
fn playable_route_collects_once_and_door_resets_at_the_exact_deadline() {
	let (project, data) = fixture();
	let mut simulation = SimulationSession::new(project, data).unwrap();
	simulation.set_controlled_input([0., 0., -1.]).unwrap();
	let mut opened_at = None;
	for _ in 0..180 {
		step(&mut simulation);
		if state(&simulation, "a-door").as_deref() == Some("open") && opened_at.is_none() {
			opened_at = Some(simulation.tick_index());
		}
	}
	simulation.clear_input();
	let opened_at = opened_at.expect("both real sensors must activate the door");
	assert_eq!(state(&simulation, "a-pickup").as_deref(), Some("spent"));
	assert_eq!(state(&simulation, "a-complete").as_deref(), Some("done"));
	assert_eq!(state(&simulation, "b-door").as_deref(), Some("closed"));
	let pickup = &simulation.world().data().entities
		[simulation.world().entity_index(&key("a-pickup")).unwrap()];
	assert!(!pickup.renderable && !pickup.collision_enabled);
	while simulation.tick_index() < opened_at + 179 {
		step(&mut simulation);
	}
	assert_eq!(state(&simulation, "a-door").as_deref(), Some("open"));
	step(&mut simulation);
	assert_eq!(state(&simulation, "a-door").as_deref(), Some("resetting"));
	assert_eq!(snapshot(&simulation)["pending_events"], 2);
	step(&mut simulation);
	assert_eq!(state(&simulation, "a-door").as_deref(), Some("closed"));
	assert_eq!(state(&simulation, "a-left").as_deref(), Some("ready"));
	assert_eq!(state(&simulation, "a-right").as_deref(), Some("ready"));
	assert_eq!(snapshot(&simulation)["machine_deadlines"], 0);
}

#[test]
fn saved_prefab_remaps_owned_routes_and_external_actor_without_rewriting_state_ids() {
	let prefab = PrefabData::from_json(include_str!(
		"../../../../apps/game/assets/prefabs/event-door.json"
	))
	.unwrap();
	let owned = prefab
		.entities
		.iter()
		.map(|e| (e.key.clone(), key(&format!("copy-{}", e.key.as_str()))))
		.collect::<HashMap<_, _>>();
	let copied = prefab
		.remap_entities(
			&owned,
			&BTreeMap::from([("actor".into(), key("event-player"))]),
		)
		.unwrap();
	for entity in &copied {
		for component in &entity.native_behaviors {
			let NativeBehaviorSpec::StateMachine { definition } = &component.behavior else {
				panic!()
			};
			for reference in definition.entity_references() {
				assert!(
					reference.as_str().starts_with("copy-") || reference.as_str() == "event-player"
				);
			}
			assert!(!definition.initial.starts_with("copy-"));
		}
	}
	let (_, data) = fixture();
	let mut combined = (*data).clone();
	combined.entities.extend(copied);
	combined.validate().unwrap();
	assert_eq!(
		SceneData::from_json(&combined.to_json().unwrap()).unwrap(),
		combined
	);
}

#[test]
fn authored_retirement_rejects_atomically_but_spawned_machine_retires_its_instance() {
	let mut retiring = entity("retiring", [0.; 3]);
	retiring
		.native_behaviors
		.push(machine("logic", vec![MachineAction::DespawnOwner]));
	let mut authored = session(scene(vec![retiring.clone()]));
	let before = snapshot(&authored);
	assert!(authored.tick(0.1).unwrap_err().contains("spawned"));
	assert_eq!(snapshot(&authored), before);
	let mut runtime = session(scene(vec![entity("original", [0.; 3])]));
	let prefab = PrefabData {
		schema_version: 3,
		composition: Default::default(),
		entities: vec![retiring],
		gameplay: None,
		reference_slots: BTreeMap::new(),
	};
	let prepared = runtime
		.prepare_runtime_spawn(&prefab, HASH, [0.; 3], None)
		.unwrap();
	let handle_key = prepared.entities()[0].key.clone();
	runtime.commit_runtime_spawn(prepared).unwrap();
	step(&mut runtime);
	assert!(runtime.runtime_handle(&handle_key).is_none());
	assert_eq!(snapshot(&runtime)["total_entities"], 1);
	assert_eq!(snapshot(&runtime)["pending_events"], 0);
}
