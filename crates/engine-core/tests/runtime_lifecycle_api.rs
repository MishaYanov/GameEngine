mod support;
use engine_core::{
	EntityKey, SceneData,
	components::{ComponentId, NativeBehaviorComponent, NativeBehaviorSpec},
	editor::PrefabData,
	project::{BehaviorSpace, BehaviorSpec, ProjectData},
	runtime_lifecycle::RuntimeEntityHandle,
	scene_catalog::SceneId,
	scene_data::{ColliderData, ColliderInteraction, EntityData},
	simulation_session::{
		ActionWriter, GameModule, GameModuleFactory, InputFrame, SimulationAction,
		SimulationSession,
	},
	triggers::TriggerPhase,
	world::RuntimeWorld,
};
use std::{
	collections::BTreeMap,
	sync::{Arc, Mutex},
};
use support::*;
const HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
fn collider(sensor: bool) -> ColliderData {
	ColliderData::Sphere {
		radius: 0.4,
		offset: [0.; 3],
		interaction: ColliderInteraction {
			sensor,
			..Default::default()
		},
	}
}
fn component(behavior: NativeBehaviorSpec) -> NativeBehaviorComponent {
	NativeBehaviorComponent {
		id: ComponentId::new("behavior").unwrap(),
		enabled: true,
		behavior,
	}
}
fn prefab(entities: Vec<EntityData>) -> PrefabData {
	PrefabData {
		schema_version: 3,
		composition: Default::default(),
		entities,
		gameplay: None,
		reference_slots: BTreeMap::new(),
	}
}
fn project(data: &SceneData) -> ProjectData {
	let mut p = ProjectData::for_scene_with_id(
		"Lifecycle",
		SceneId::new("main").unwrap(),
		"scenes/main.json",
		data,
	)
	.unwrap();
	p.fixed_hz = 10;
	p
}
fn simple() -> (ProjectData, Arc<SceneData>) {
	let data = Arc::new(scene(vec![
		entity("actor", [0.; 3]).with_collider(Some(collider(false))),
	]));
	(project(&data), data)
}
fn spawner(interval: u32, lifetime: u32) -> EntityData {
	let mut e = entity("spawner", [5., 0., 0.]);
	e.native_behaviors
		.push(component(NativeBehaviorSpec::TimedSpawner {
			prefab_path: "prefabs/item.json".into(),
			interval_ticks: interval,
			max_alive: 1,
			lifetime_ticks: lifetime,
			spawn_offset: [0.; 3],
			bindings: BTreeMap::new(),
		}));
	e
}
fn fulfill(session: &mut SimulationSession, prefab: &PrefabData) -> EntityKey {
	let request = session.pending_runtime_spawns().into_iter().next().unwrap();
	let prepared = session
		.prepare_runtime_spawn(prefab, HASH, request.position, Some(request.id))
		.unwrap();
	let key = prepared.entities()[0].key.clone();
	session.commit_runtime_spawn(prepared).unwrap();
	key
}

#[test]
fn independent_key_door_components_commit_separate_states_and_round_trip() {
	let actor = entity("actor", [0.; 3]).with_collider(Some(collider(false)));
	let mut first = entity("key-a", [0.; 3]).with_collider(Some(collider(true)));
	let mut second = entity("key-b", [4., 0., 0.]).with_collider(Some(collider(true)));
	first
		.native_behaviors
		.push(component(NativeBehaviorSpec::KeyDoor {
			actor: key("actor"),
			key_trigger: key("key-a"),
			door: key("door-a"),
			key_visual: Some(key("key-a")),
		}));
	second
		.native_behaviors
		.push(component(NativeBehaviorSpec::KeyDoor {
			actor: key("actor"),
			key_trigger: key("key-b"),
			door: key("door-b"),
			key_visual: Some(key("key-b")),
		}));
	let data = scene(vec![
		actor,
		first,
		second,
		entity("door-a", [10., 0., 0.]).with_collider(Some(collider(false))),
		entity("door-b", [20., 0., 0.]).with_collider(Some(collider(false))),
	]);
	let saved = data.to_json().unwrap();
	let reopened = Arc::new(SceneData::from_json(&saved).unwrap());
	let mut p = project(&reopened);
	p.add_behavior(BehaviorSpec::ControlledMove {
		entity: key("actor"),
		speed: 10.,
		space: BehaviorSpace::World,
	});
	let mut session = SimulationSession::new(p.clone(), reopened.clone()).unwrap();
	session.tick(0.1).unwrap();
	assert!(!session.world().data().entities[3].collision_enabled);
	assert!(session.world().data().entities[4].collision_enabled);
	session.set_controlled_input([1., 0., 0.]).unwrap();
	for _ in 0..4 {
		session.tick(0.1).unwrap();
	}
	session.clear_input();
	session.tick(0.1).unwrap();
	assert!(!session.world().data().entities[4].collision_enabled);
	let inspection = session.runtime_inspection("play", "run", 0, 128).unwrap();
	assert_eq!(
		inspection
			.entities
			.iter()
			.flat_map(|entity| &entity.behavior_states)
			.filter(|state| state.activated)
			.count(),
		2
	);
	let fresh = SimulationSession::new(p, reopened).unwrap();
	assert!(fresh.world().data().entities[3].collision_enabled);
	assert_eq!(saved, data.to_json().unwrap());
}

#[test]
fn prepared_tokens_reject_foreign_world_and_stale_topology_without_mutation() {
	let (p, data) = simple();
	let mut a = SimulationSession::new(p.clone(), data.clone()).unwrap();
	let mut b = SimulationSession::new(p, data).unwrap();
	let prefab = prefab(vec![entity("item", [0.; 3])]);
	let foreign = a
		.prepare_runtime_spawn(&prefab, HASH, [0.; 3], None)
		.unwrap();
	assert!(
		b.commit_runtime_spawn(foreign)
			.unwrap_err()
			.contains("stale")
	);
	let first = a
		.prepare_runtime_spawn(&prefab, HASH, [0.; 3], None)
		.unwrap();
	let stale = a
		.prepare_runtime_spawn(&prefab, HASH, [0.; 3], None)
		.unwrap();
	a.commit_runtime_spawn(first).unwrap();
	assert!(a.commit_runtime_spawn(stale).is_err());
	assert_eq!(a.world().data().entities.len(), 2);
	assert_eq!(b.world().data().entities.len(), 1);
}

#[test]
fn spawn_merges_current_transforms_and_native_admission_requires_prepared_meshes() {
	let (mut p, data) = simple();
	p.add_behavior(BehaviorSpec::ControlledMove {
		entity: key("actor"),
		speed: 10.,
		space: BehaviorSpace::World,
	});
	let mut session = SimulationSession::new(p, data).unwrap();
	let prefab = prefab(vec![entity("item", [0.; 3])]);
	session
		.set_visibility_capabilities([key("actor")].into_iter().collect())
		.unwrap();
	let missing = session
		.prepare_runtime_spawn(&prefab, HASH, [8., 0., 0.], None)
		.unwrap();
	assert!(session.commit_runtime_spawn(missing).is_err());
	let mut ready = session
		.prepare_runtime_spawn(&prefab, HASH, [8., 0., 0.], None)
		.unwrap();
	let spawned = ready.entities()[0].key.clone();
	ready
		.set_visibility_capabilities([spawned.clone()].into_iter().collect())
		.unwrap();
	session.set_controlled_input([1., 0., 0.]).unwrap();
	session.tick(0.1).unwrap();
	session.commit_runtime_spawn(ready).unwrap();
	close(session.world().data().entities[0].transform.position[0], 1.);
	close(session.world().data().entities[1].transform.position[0], 8.);
}

#[test]
fn timers_pause_expire_once_and_release_all_instance_ownership() {
	let data = Arc::new(scene(vec![spawner(100, 2)]));
	let mut session = SimulationSession::new(project(&data), data).unwrap();
	for _ in 0..100 {
		session.tick(0.1).unwrap();
	}
	let prefab = prefab(vec![
		entity("item", [0.; 3]),
		entity("child", [1., 0., 0.]).with_parent(Some(key("item"))),
	]);
	let spawned = fulfill(&mut session, &prefab);
	let handle = session.runtime_handle(&spawned).unwrap();
	assert_eq!(
		session
			.runtime_inspection("p", "r", 0, 128)
			.unwrap()
			.timer_count,
		1
	);
	session.set_paused(true);
	session.tick(60.).unwrap();
	assert!(session.runtime_handle(&spawned).is_some());
	session.set_paused(false);
	session.tick(0.1).unwrap();
	assert!(session.runtime_handle(&spawned).is_some());
	let expired = session.tick(0.1).unwrap();
	assert!(expired.changes.topology_changed);
	assert_eq!(session.world().data().entities.len(), 1);
	assert_eq!(
		session
			.runtime_inspection("p", "r", 0, 128)
			.unwrap()
			.timer_count,
		0
	);
	assert!(session.runtime_handle(&handle.key).is_none());
}

#[test]
fn collectible_reference_slot_despawns_on_contact_then_emits_exit() {
	let actor = entity("actor", [0.; 3]).with_collider(Some(collider(false)));
	let mut emitter = spawner(1, 30);
	emitter.transform.position = [0.; 3];
	if let NativeBehaviorSpec::TimedSpawner { bindings, .. } =
		&mut emitter.native_behaviors[0].behavior
	{
		bindings.insert("actor".into(), key("actor"));
	}
	let data = Arc::new(scene(vec![actor, emitter]));
	let mut session = SimulationSession::new(project(&data), data).unwrap();
	let mut item = entity("item", [0.; 3]).with_collider(Some(collider(true)));
	item.native_behaviors
		.push(component(NativeBehaviorSpec::Collectible {
			actor: key("slot-actor"),
			trigger: key("item"),
			visual: Some(key("item")),
			despawn_owner: true,
		}));
	let mut prefab = prefab(vec![item]);
	prefab
		.reference_slots
		.insert("actor".into(), key("slot-actor"));
	session.tick(0.1).unwrap();
	let spawned = fulfill(&mut session, &prefab);
	let collect = session.tick(0.1).unwrap();
	assert!(
		collect
			.trigger_events
			.iter()
			.any(|event| event.phase == TriggerPhase::Enter && event.sensor == spawned)
	);
	assert!(session.runtime_handle(&spawned).is_none());
	let next = session.tick(0.1).unwrap();
	assert!(
		next.trigger_events
			.iter()
			.any(|event| event.phase == TriggerPhase::Exit && event.sensor == spawned)
	);
	assert_eq!(
		session
			.runtime_inspection("p", "r", 0, 128)
			.unwrap()
			.timer_count,
		0
	);
}

#[test]
fn failed_load_is_bounded_retry_diagnostic_and_inspection_pages_are_bounded() {
	let data = Arc::new(scene(vec![spawner(2, 30)]));
	let mut session = SimulationSession::new(project(&data), data).unwrap();
	session.tick(0.2).unwrap();
	let request = session.pending_runtime_spawns().remove(0);
	session.reject_runtime_spawn(request.id, &"x".repeat(1000));
	let view = session.runtime_inspection("p", "r", 0, 1).unwrap();
	assert_eq!(view.last_error.unwrap().len(), 512);
	assert_eq!(view.pending_spawns, 0);
	assert!(session.runtime_inspection("p", "r", 0, 129).is_err());
	assert!(session.runtime_inspection("p", "r", 0, 0).is_err());
	session.tick(0.1).unwrap();
	assert!(session.pending_runtime_spawns().is_empty());
	session.tick(0.1).unwrap();
	assert_eq!(session.pending_runtime_spawns().len(), 1);
}

#[derive(Clone)]
struct Script(Arc<Mutex<Vec<SimulationAction>>>);
impl GameModule for Script {
	fn needs_tick(&self, _: &InputFrame) -> bool {
		true
	}
	fn plan(
		&self,
		_: &RuntimeWorld,
		_: &InputFrame,
		_: f32,
		output: &mut ActionWriter<'_>,
	) -> Result<(), String> {
		for action in self.0.lock().unwrap().iter().map(|action| match action {
			SimulationAction::Despawn { handle } => SimulationAction::Despawn {
				handle: handle.clone(),
			},
			SimulationAction::SetVisible { entity, visible } => SimulationAction::SetVisible {
				entity: entity.clone(),
				visible: *visible,
			},
			_ => unreachable!(),
		}) {
			output.push(action)?;
		}
		Ok(())
	}
}
impl GameModuleFactory for Script {
	fn create(&self, _: &ProjectData, _: &SceneId) -> Result<Box<dyn GameModule>, String> {
		Ok(Box::new(self.clone()))
	}
}

#[test]
fn rejected_structural_batch_restores_world_handles_timers_and_component_state() {
	let (p, data) = simple();
	let actions = Script(Arc::new(Mutex::new(vec![])));
	let mut session = SimulationSession::with_factory(p, data, &actions).unwrap();
	let prefab = prefab(vec![entity("item", [0.; 3])]);
	let ready = session
		.prepare_runtime_spawn(&prefab, HASH, [0.; 3], None)
		.unwrap();
	let spawned = ready.entities()[0].key.clone();
	session.commit_runtime_spawn(ready).unwrap();
	let handle = session.runtime_handle(&spawned).unwrap();
	let before = session.world().data().clone();
	*actions.0.lock().unwrap() = vec![
		SimulationAction::SetVisible {
			entity: key("actor"),
			visible: false,
		},
		SimulationAction::Despawn {
			handle: handle.clone(),
		},
		SimulationAction::Despawn {
			handle: RuntimeEntityHandle {
				world_id: "foreign".into(),
				..handle.clone()
			},
		},
	];
	assert!(session.tick(0.1).unwrap_err().contains("stale"));
	assert_eq!(session.world().data(), &before);
	assert_eq!(session.runtime_handle(&spawned), Some(handle));
	assert_eq!(session.tick_index(), 0);
}

#[test]
fn stale_handles_cannot_remove_a_later_instance_and_keys_are_deterministic() {
	let (p, data) = simple();
	let actions = Script(Arc::new(Mutex::new(vec![])));
	let mut a = SimulationSession::with_factory(p.clone(), data.clone(), &actions).unwrap();
	let b = SimulationSession::new(p, data).unwrap();
	let prefab = prefab(vec![entity("item", [0.; 3])]);
	let first = a
		.prepare_runtime_spawn(&prefab, HASH, [0.; 3], None)
		.unwrap();
	let other = b
		.prepare_runtime_spawn(&prefab, HASH, [0.; 3], None)
		.unwrap();
	assert_eq!(first.entities()[0].key, other.entities()[0].key);
	let spawned = first.entities()[0].key.clone();
	a.commit_runtime_spawn(first).unwrap();
	let stale = a.runtime_handle(&spawned).unwrap();
	*actions.0.lock().unwrap() = vec![SimulationAction::Despawn {
		handle: stale.clone(),
	}];
	a.tick(0.1).unwrap();
	let next = a
		.prepare_runtime_spawn(&prefab, HASH, [0.; 3], None)
		.unwrap();
	let next_key = next.entities()[0].key.clone();
	assert_ne!(spawned, next_key);
	a.commit_runtime_spawn(next).unwrap();
	assert!(a.tick(0.1).is_err());
	assert!(a.runtime_handle(&next_key).is_some());
}

#[test]
fn removing_spawner_owner_cascades_to_child_instances_and_pending_requests() {
	let data = Arc::new(scene(vec![spawner(100, 4)]));
	let mut session = SimulationSession::new(project(&data), data).unwrap();
	for _ in 0..100 {
		session.tick(0.1).unwrap();
	}
	let mut child_spawner = spawner(1, 100);
	child_spawner.key = key("child-spawner");
	let parent = fulfill(&mut session, &prefab(vec![child_spawner]));
	session.tick(0.1).unwrap();
	let child = fulfill(&mut session, &prefab(vec![entity("child", [0.; 3])]));
	assert_eq!(
		session
			.runtime_inspection("p", "r", 0, 128)
			.unwrap()
			.timer_count,
		2
	);
	for _ in 0..3 {
		session.tick(0.1).unwrap();
	}
	assert!(session.runtime_handle(&parent).is_none());
	assert!(session.runtime_handle(&child).is_none());
	let inspection = session.runtime_inspection("p", "r", 0, 128).unwrap();
	assert_eq!(inspection.total_entities, 1);
	assert_eq!(inspection.timer_count, 0);
	assert_eq!(inspection.pending_spawns, 0);
}

#[test]
fn fixed_step_partition_preserves_timer_deadlines_and_paused_admission_rejects() {
	let data = Arc::new(scene(vec![spawner(1, 3)]));
	let p = project(&data);
	let mut a = SimulationSession::new(p.clone(), data.clone()).unwrap();
	let mut b = SimulationSession::new(p, data).unwrap();
	let item = prefab(vec![entity("item", [0.; 3])]);
	a.tick(0.1).unwrap();
	b.tick(0.1).unwrap();
	let first = fulfill(&mut a, &item);
	let second = fulfill(&mut b, &item);
	a.tick(0.2).unwrap();
	a.tick(0.1).unwrap();
	for _ in 0..3 {
		b.tick(0.1).unwrap();
	}
	assert!(a.runtime_handle(&first).is_none());
	assert!(b.runtime_handle(&second).is_none());
	assert_eq!(a.world().data(), b.world().data());
	let prepared = a.prepare_runtime_spawn(&item, HASH, [0.; 3], None).unwrap();
	a.set_paused(true);
	assert!(
		a.commit_runtime_spawn(prepared)
			.unwrap_err()
			.contains("Resume")
	);
}
