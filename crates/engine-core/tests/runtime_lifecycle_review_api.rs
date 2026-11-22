mod support;

use engine_core::{
	EntityKey, SceneData,
	components::{ComponentId, NativeBehaviorComponent, NativeBehaviorSpec},
	editor::PrefabData,
	project::ProjectData,
	runtime_lifecycle::RuntimeEntityHandle,
	scene_catalog::SceneId,
	scene_data::EntityData,
	simulation_session::{
		ActionWriter, GameModule, GameModuleFactory, InputFrame, SimulationAction,
		SimulationSession,
	},
	world::RuntimeWorld,
};
use std::{
	collections::BTreeMap,
	sync::{Arc, Mutex},
};
use support::*;

const HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn spawner(id: &str, interval: u32, lifetime: u32) -> NativeBehaviorComponent {
	NativeBehaviorComponent {
		id: ComponentId::new(id).unwrap(),
		enabled: true,
		behavior: NativeBehaviorSpec::TimedSpawner {
			prefab_path: "prefabs/item.json".into(),
			interval_ticks: interval,
			max_alive: 1,
			lifetime_ticks: lifetime,
			spawn_offset: [0.; 3],
			bindings: BTreeMap::new(),
		},
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
	let mut project = ProjectData::for_scene("Lifecycle review", "main.json", data).unwrap();
	project.fixed_hz = 10;
	project
}

fn fulfill(session: &mut SimulationSession, prefab: &PrefabData, component: &str) -> EntityKey {
	let requests = session.pending_runtime_spawns();
	assert_eq!(
		requests.len(),
		1,
		"only the due attachment may request a spawn"
	);
	assert_eq!(requests[0].component_id.as_str(), component);
	let prepared = session
		.prepare_runtime_spawn(prefab, HASH, requests[0].position, Some(requests[0].id))
		.unwrap();
	let key = prepared.entities()[0].key.clone();
	session.commit_runtime_spawn(prepared).unwrap();
	key
}

#[test]
fn attachments_on_one_owner_keep_independent_deadlines_and_alive_limits() {
	let mut owner = entity("owner", [0.; 3]);
	owner.native_behaviors = vec![spawner("fast", 1, 2), spawner("slow", 2, 6)];
	let authored = scene(vec![owner]);
	let saved = authored.to_json().unwrap();
	let reopened = Arc::new(SceneData::from_json(&saved).unwrap());
	let mut session = SimulationSession::new(project(&reopened), reopened.clone()).unwrap();
	let item = prefab(vec![entity("item", [0.; 3])]);

	session.tick(0.1).unwrap();
	let first_fast = fulfill(&mut session, &item, "fast");
	session.tick(0.1).unwrap();
	let slow = fulfill(&mut session, &item, "slow");
	assert_eq!(
		session
			.runtime_inspection("play", "run", 0, 128)
			.unwrap()
			.timer_count,
		2
	);

	session.tick(0.1).unwrap();
	assert!(session.runtime_handle(&first_fast).is_none());
	assert!(session.runtime_handle(&slow).is_some());
	assert!(session.pending_runtime_spawns().is_empty());

	session.tick(0.1).unwrap();
	let second_fast = fulfill(&mut session, &item, "fast");
	assert_ne!(
		first_fast, second_fast,
		"retired identities must not be reused"
	);
	assert!(session.runtime_handle(&slow).is_some());
	session.tick(0.1).unwrap();
	assert!(
		session.pending_runtime_spawns().is_empty(),
		"each attachment is at its own alive limit"
	);
	session.tick(0.1).unwrap();
	assert!(session.runtime_handle(&second_fast).is_none());
	assert!(session.runtime_handle(&slow).is_some());

	session.tick(0.1).unwrap();
	let third_fast = fulfill(&mut session, &item, "fast");
	session.tick(0.1).unwrap();
	assert!(session.runtime_handle(&slow).is_none());
	assert!(session.runtime_handle(&third_fast).is_some());
	session.tick(0.1).unwrap();
	let second_slow = fulfill(&mut session, &item, "slow");
	assert!(session.runtime_handle(&third_fast).is_none());
	assert!(session.runtime_handle(&second_slow).is_some());
	let inspection = session.runtime_inspection("play", "run", 0, 128).unwrap();
	assert_eq!(inspection.total_entities, 2);
	assert_eq!(inspection.timer_count, 1);
	assert!(inspection.last_error.is_none());
	assert_eq!(
		reopened.to_json().unwrap(),
		saved,
		"runtime ownership cannot alter authoring data"
	);
}

#[derive(Clone, Default)]
struct DuplicateRemovals(Arc<Mutex<Vec<RuntimeEntityHandle>>>);

impl GameModule for DuplicateRemovals {
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
		for handle in self.0.lock().unwrap().iter() {
			output.push(SimulationAction::Despawn {
				handle: handle.clone(),
			})?;
		}
		Ok(())
	}
}

impl GameModuleFactory for DuplicateRemovals {
	fn create(&self, _: &ProjectData, _: &SceneId) -> Result<Box<dyn GameModule>, String> {
		Ok(Box::new(self.clone()))
	}
}

#[test]
fn simultaneous_parent_child_expiry_and_duplicate_actions_retire_ownership_once() {
	let mut owner = entity("authored-owner", [0.; 3]);
	owner
		.native_behaviors
		.push(spawner("parent-emitter", 100, 3));
	let authored = Arc::new(scene(vec![owner]));
	let script = DuplicateRemovals::default();
	let mut session =
		SimulationSession::with_factory(project(&authored), authored.clone(), &script).unwrap();
	for _ in 0..100 {
		session.tick(0.1).unwrap();
	}
	let mut nested = entity("nested-owner", [0.; 3]);
	nested.native_behaviors.push(spawner("child-emitter", 1, 2));
	let parent = fulfill(&mut session, &prefab(vec![nested]), "parent-emitter");
	session.tick(0.1).unwrap();
	let child = fulfill(
		&mut session,
		&prefab(vec![entity("child", [0.; 3])]),
		"child-emitter",
	);
	let parent_handle = session.runtime_handle(&parent).unwrap();
	let child_handle = session.runtime_handle(&child).unwrap();
	session.tick(0.1).unwrap();
	assert_eq!(session.tick_index(), 102);
	assert_eq!(
		session
			.runtime_inspection("play", "run", 0, 128)
			.unwrap()
			.timer_count,
		2
	);
	*script.0.lock().unwrap() = vec![parent_handle.clone(), child_handle, parent_handle];

	let expired = session.tick(0.1).unwrap();
	assert!(expired.changes.topology_changed);
	assert!(expired.changes.full_resync);
	assert!(session.runtime_handle(&parent).is_none());
	assert!(session.runtime_handle(&child).is_none());
	assert_eq!(session.world().data(), authored.as_ref());
	let inspection = session.runtime_inspection("play", "run", 0, 128).unwrap();
	assert_eq!(inspection.tick, 103);
	assert_eq!(
		inspection.topology_revision, 3,
		"two admissions and one cascading removal"
	);
	assert_eq!(inspection.timer_count, 0);
	assert_eq!(inspection.pending_spawns, 0);
	assert_eq!(inspection.total_entities, 1);
	assert!(inspection.last_error.is_none());
	script.0.lock().unwrap().clear();
	let next = session.tick(0.1).unwrap();
	assert!(next.changes.removed_entities.is_empty());
	assert_eq!(
		session
			.runtime_inspection("play", "run", 0, 128)
			.unwrap()
			.topology_revision,
		3
	);
}
