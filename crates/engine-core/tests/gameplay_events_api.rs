mod support;
use engine_core::{
	SceneData,
	components::{ComponentId, NativeBehaviorComponent, NativeBehaviorSpec},
	editor::PrefabData,
	gameplay_events::{ComponentRef, Signal},
	project::ProjectData,
	scene_catalog::SceneId,
	simulation_session::{
		ActionWriter, GameModule, GameModuleFactory, InputFrame, SimulationAction,
		SimulationSession,
	},
	state_machine::{MachineAction, MachineDef, MachineTrigger, StateDef, TransitionDef},
	world::RuntimeWorld,
};
use std::{
	collections::{BTreeMap, HashMap},
	sync::{
		Arc,
		atomic::{AtomicUsize, Ordering},
	},
};
use support::*;
const HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
fn fixture() -> (ProjectData, Arc<SceneData>) {
	let data = SceneData::from_json(include_str!(
		"../../../apps/game/assets/scenes/event-lab.json"
	))
	.unwrap();
	let project =
		ProjectData::from_json(include_str!("../../../apps/game/event-demo.json")).unwrap();
	(project, Arc::new(data))
}
fn step(session: &mut SimulationSession) {
	session
		.tick(1.0 / f64::from(session.project().fixed_hz))
		.unwrap();
}
fn state(session: &SimulationSession, owner: &str) -> Option<String> {
	session
		.runtime_inspection("play", "run", 0, 128)
		.unwrap()
		.entities
		.into_iter()
		.find(|e| e.handle.key.as_str() == owner)
		.unwrap()
		.behavior_states
		.into_iter()
		.find(|s| s.kind == "state_machine")
		.unwrap()
		.machine_state
}
fn snapshot(session: &SimulationSession) -> serde_json::Value {
	let mut value =
		serde_json::to_value(session.runtime_inspection("play", "run", 0, 128).unwrap()).unwrap();
	for entity in value["entities"].as_array_mut().unwrap() {
		entity["handle"]["world_id"] = serde_json::json!("normalized");
	}
	value["entities"]
		.as_array_mut()
		.unwrap()
		.sort_by_key(|e| e["handle"]["key"].as_str().unwrap().to_owned());
	value
}
fn machine(id: &str, actions: Vec<MachineAction>) -> NativeBehaviorComponent {
	NativeBehaviorComponent {
		id: ComponentId::new(id).unwrap(),
		enabled: true,
		behavior: NativeBehaviorSpec::StateMachine {
			definition: MachineDef {
				schema_version: 1,
				initial: "ready".into(),
				inputs: vec![],
				states: vec![StateDef {
					id: "ready".into(),
					on_enter: actions,
					transitions: vec![],
				}],
			},
		},
	}
}
fn session(data: SceneData) -> SimulationSession {
	let mut p = ProjectData::for_scene("Events", "main.json", &data).unwrap();
	p.fixed_hz = 10;
	SimulationSession::new(p, Arc::new(data)).unwrap()
}

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
fn runtime_state_is_independent_of_entity_order_and_elapsed_partition() {
	let (mut project, data) = fixture();
	project.fixed_hz = 20;
	let mut reverse = (*data).clone();
	reverse.entities.reverse();
	let mut a = SimulationSession::new(project.clone(), data).unwrap();
	let mut b = SimulationSession::new(project, Arc::new(reverse)).unwrap();
	a.tick(0.15).unwrap();
	for _ in 0..3 {
		step(&mut b);
	}
	assert_eq!(snapshot(&a), snapshot(&b));
}

struct FailLate {
	calls: Arc<AtomicUsize>,
	fail_at: usize,
}
impl GameModuleFactory for FailLate {
	fn create(&self, _: &ProjectData, _: &SceneId) -> Result<Box<dyn GameModule>, String> {
		Ok(Box::new(Self {
			calls: self.calls.clone(),
			fail_at: self.fail_at,
		}))
	}
}
impl GameModule for FailLate {
	fn needs_tick(&self, _: &InputFrame) -> bool {
		true
	}
	fn plan(
		&self,
		_: &RuntimeWorld,
		_: &InputFrame,
		_: f32,
		out: &mut ActionWriter<'_>,
	) -> Result<(), String> {
		if self.calls.fetch_add(1, Ordering::Relaxed) + 1 == self.fail_at {
			out.push(SimulationAction::SetVisible {
				entity: key("missing"),
				visible: false,
			})?;
		}
		Ok(())
	}
}
#[test]
fn late_aggregate_failure_rolls_back_initial_effects_inbox_sequence_and_machine_entries() {
	let (mut project, data) = fixture();
	project.fixed_hz = 20;
	let calls = Arc::new(AtomicUsize::new(0));
	let mut simulation = SimulationSession::with_factory(
		project.clone(),
		data.clone(),
		&FailLate {
			calls: calls.clone(),
			fail_at: 3,
		},
	)
	.unwrap();
	let before = snapshot(&simulation);
	let world = simulation.world().data().clone();
	assert!(simulation.tick(0.15).is_err());
	assert!(simulation.paused());
	assert_eq!(snapshot(&simulation), before);
	assert_eq!(simulation.world().data(), &world);
	simulation.set_paused(false);
	simulation.tick(0.15).unwrap();
	let mut clean = SimulationSession::new(project, data).unwrap();
	clean.tick(0.15).unwrap();
	assert_eq!(snapshot(&simulation), snapshot(&clean));
}

#[test]
fn disabled_target_discards_delivery_and_pure_signal_machine_becomes_idle() {
	let mut source = entity("source", [0.; 3]);
	source.native_behaviors.push(machine(
		"logic",
		vec![MachineAction::Emit {
			target: ComponentRef {
				entity: key("target"),
				component: ComponentId::new("logic").unwrap(),
			},
			signal: Signal::Reset,
		}],
	));
	let mut target = entity("target", [2., 0., 0.]);
	let mut disabled = machine("logic", vec![]);
	disabled.enabled = false;
	target.native_behaviors.push(disabled);
	let mut simulation = session(scene(vec![source, target]));
	step(&mut simulation);
	assert!(simulation.needs_tick());
	assert_eq!(snapshot(&simulation)["pending_events"], 1);
	step(&mut simulation);
	assert_eq!(snapshot(&simulation)["stale_events_discarded"], 1);
	assert_eq!(snapshot(&simulation)["pending_events"], 0);
	assert!(!simulation.needs_tick());
	assert_eq!(state(&simulation, "target"), None);
}

#[test]
fn disabled_machine_validates_unreachable_routes_and_action_capabilities() {
	let mut owner = entity("owner", [0.; 3]);
	let mut invalid = machine(
		"logic",
		vec![MachineAction::Emit {
			target: ComponentRef {
				entity: key("owner"),
				component: ComponentId::new("missing").unwrap(),
			},
			signal: Signal::Reset,
		}],
	);
	invalid.enabled = false;
	owner.native_behaviors.push(invalid);
	assert!(
		scene(vec![owner.clone()])
			.validate()
			.unwrap_err()
			.contains("attachment")
	);
	if let NativeBehaviorSpec::StateMachine { definition } = &mut owner.native_behaviors[0].behavior
	{
		definition.states[0].on_enter = vec![MachineAction::SetCollisionEnabled {
			entity: key("owner"),
			enabled: false,
		}];
	}
	assert!(
		scene(vec![owner])
			.validate()
			.unwrap_err()
			.contains("collider")
	);
}

#[test]
fn saved_prefab_remaps_owned_routes_and_external_actor_without_rewriting_state_ids() {
	let prefab = PrefabData::from_json(include_str!(
		"../../../apps/game/assets/prefabs/event-door.json"
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

fn character_fixture() -> (ProjectData, Arc<SceneData>) {
	let mut data = SceneData::from_json(include_str!(
		"../../../apps/game/assets/scenes/controller-room.json"
	))
	.unwrap();
	let project =
		ProjectData::from_json(include_str!("../../../apps/game/controller-demo.json")).unwrap();
	let owner = data
		.entities
		.iter_mut()
		.find(|e| e.key.as_str() == "player")
		.unwrap();
	let transition = |signal, next: &str| TransitionDef {
		when: MachineTrigger::Signal {
			signal,
			source: Some(ComponentRef {
				entity: key("player"),
				component: ComponentId::new("logic").unwrap(),
			}),
		},
		next: next.into(),
	};
	let states = vec![
		StateDef {
			id: "ready".into(),
			on_enter: vec![],
			transitions: vec![transition(Signal::Jumped, "air")],
		},
		StateDef {
			id: "air".into(),
			on_enter: vec![],
			transitions: vec![transition(Signal::Landed, "landed")],
		},
		StateDef {
			id: "landed".into(),
			on_enter: vec![],
			transitions: vec![],
		},
	];
	owner.native_behaviors.push(NativeBehaviorComponent {
		id: ComponentId::new("logic").unwrap(),
		enabled: true,
		behavior: NativeBehaviorSpec::StateMachine {
			definition: MachineDef {
				schema_version: 1,
				initial: "ready".into(),
				inputs: vec![],
				states,
			},
		},
	});
	let mut disabled = machine("disabled", vec![]);
	disabled.enabled = false;
	owner.native_behaviors.push(disabled);
	(project, Arc::new(data))
}

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

#[test]
fn native_replay_distinguishes_hidden_inbox_changes_and_normalizes_random_world_ids() {
	let (project, data) = fixture();
	let mut a = SimulationSession::new(project.clone(), data.clone()).unwrap();
	let mut b = SimulationSession::new(project, data).unwrap();
	step(&mut a);
	step(&mut b);
	assert_eq!(a.native_replay_state(), b.native_replay_state());
	let before = a.native_replay_state();
	let visible = a.world().data().clone();
	let uuid = a.runtime_handle(&key("a-left")).unwrap().world_id;
	assert!(!serde_json::to_string(&before).unwrap().contains(&uuid));
	assert_eq!(
		before["lifecycle"]["events"]["pending"][0]["source"]["same_world"],
		true
	);
	step(&mut a);
	assert_eq!(a.world().data(), &visible);
	assert_ne!(a.native_replay_state(), before);
	assert_eq!(
		a.native_replay_state()["lifecycle"]["events"]["pending"][0]["target"]["entity"],
		"a-door"
	);
}
