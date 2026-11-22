#[path = "gameplay_events_api/authored.rs"]
mod authored;
#[path = "gameplay_events_api/motion.rs"]
mod motion;
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
		ProjectData::from_json(&test_project::manifest("event-lab")).unwrap();
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

fn character_fixture() -> (ProjectData, Arc<SceneData>) {
	let mut data = SceneData::from_json(include_str!(
		"../../../apps/game/assets/scenes/controller-room.json"
	))
	.unwrap();
	let project =
		ProjectData::from_json(&test_project::manifest("controller-room")).unwrap();
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

#[path = "../../../tests/support/project_fixture.rs"]
mod test_project;
