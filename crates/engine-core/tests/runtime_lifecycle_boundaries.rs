#[path = "runtime_lifecycle_boundaries/scheduling.rs"]
mod scheduling;
mod support;

use engine_core::{
	Engine, EngineBuilder, SceneData,
	command::{EditorCommandDto as Command, EditorCommandRequest},
	components::{ComponentId, NativeBehaviorComponent, NativeBehaviorSpec},
	editor::PrefabData,
	project::ProjectData,
	runtime_lifecycle::RuntimeEntityHandle,
	scene_catalog::{SceneDescriptor, SceneId},
	scene_data::{ColliderData, ColliderInteraction, EntityData},
	simulation_session::{
		ActionWriter, GameModule, GameModuleFactory, InputFrame, SimulationAction,
		SimulationSession,
	},
	world::RuntimeWorld,
};
use std::{
	collections::BTreeMap,
	sync::{
		Arc, Mutex,
		atomic::{AtomicBool, Ordering},
	},
};
use support::*;

const HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn behavior(id: &str, spec: NativeBehaviorSpec) -> NativeBehaviorComponent {
	NativeBehaviorComponent {
		id: ComponentId::new(id).unwrap(),
		enabled: true,
		behavior: spec,
	}
}
fn emitter(name: &str) -> EntityData {
	let mut value = entity(name, [5.0, 0.0, 0.0]);
	value.native_behaviors.push(behavior(
		"emitter",
		NativeBehaviorSpec::TimedSpawner {
			prefab_path: "prefabs/item.json".into(),
			interval_ticks: 1,
			max_alive: 2,
			lifetime_ticks: 100,
			spawn_offset: [0.0; 3],
			bindings: BTreeMap::new(),
		},
	));
	value
}
fn prefab(entities: Vec<EntityData>) -> PrefabData {
	PrefabData {
		schema_version: 3,
		entities,
		gameplay: None,
		reference_slots: BTreeMap::new(),
	}
}
fn project(data: &SceneData) -> ProjectData {
	let mut project = ProjectData::for_scene_with_id(
		"Boundaries",
		SceneId::new("main").unwrap(),
		"scenes/main.json",
		data,
	)
	.unwrap();
	project.fixed_hz = 10;
	project
}
fn run(engine: &mut Engine, command: Command) {
	engine
		.execute(EditorCommandRequest {
			expected_revision: engine.session().revision(),
			command,
		})
		.unwrap();
}
fn inspection(session: &SimulationSession) -> serde_json::Value {
	serde_json::to_value(session.runtime_inspection("play", "run", 0, 128).unwrap()).unwrap()
}

#[derive(Clone, Default)]
struct Script {
	fail: Arc<AtomicBool>,
	despawn: Arc<Mutex<Option<RuntimeEntityHandle>>>,
	request_scene: Arc<Mutex<Option<SceneId>>>,
}
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
		if let Some(handle) = self.despawn.lock().unwrap().clone() {
			output.push(SimulationAction::Despawn { handle })?;
		}
		if let Some(scene_id) = self.request_scene.lock().unwrap().clone() {
			output.push(SimulationAction::RequestScene { scene_id })?;
		}
		if self.fail.load(Ordering::Relaxed) {
			output.push(SimulationAction::SetVisible {
				entity: key("missing-action-target"),
				visible: false,
			})?;
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
fn native_instance_state_and_trigger_history_rollback_after_later_action_failure() {
	let shape = |sensor| ColliderData::Sphere {
		radius: 0.5,
		offset: [0.0; 3],
		interaction: ColliderInteraction {
			sensor,
			..Default::default()
		},
	};
	let actor = entity("actor", [0.0; 3]).with_collider(Some(shape(false)));
	let mut sensor = entity("key", [0.0; 3]).with_collider(Some(shape(true)));
	sensor.native_behaviors.push(behavior(
		"unlock",
		NativeBehaviorSpec::KeyDoor {
			actor: key("actor"),
			key_trigger: key("key"),
			door: key("door"),
			key_visual: Some(key("key")),
		},
	));
	let door = entity("door", [10.0, 0.0, 0.0]).with_collider(Some(shape(false)));
	let data = Arc::new(scene(vec![actor, sensor, door]));
	let script = Script::default();
	script.fail.store(true, Ordering::Relaxed);
	let mut session =
		SimulationSession::with_factory(project(&data), data.clone(), &script).unwrap();
	let before = inspection(&session);
	assert!(session.tick(0.1).is_err());
	assert_eq!(session.world().data(), data.as_ref());
	assert_eq!(
		inspection(&session),
		before,
		"candidate component state must not leak from a rejected tick"
	);
	script.fail.store(false, Ordering::Relaxed);
	session.set_paused(false);
	let accepted = session.tick(0.1).unwrap();
	assert_eq!(
		accepted.trigger_events.len(),
		1,
		"the rejected contact must still produce its first Enter"
	);
	assert!(!session.world().data().entities[2].collision_enabled);
	assert!(
		inspection(&session)["entities"][1]["behavior_states"][0]["activated"]
			.as_bool()
			.unwrap()
	);
}

#[test]
fn pending_request_limit_accepts_32_and_rejects_33_as_one_atomic_tick() {
	for count in [32, 33] {
		let data = Arc::new(scene(
			(0..count)
				.map(|index| emitter(&format!("spawner-{index}")))
				.collect(),
		));
		let mut session = SimulationSession::new(project(&data), data).unwrap();
		let before = inspection(&session);
		if count == 32 {
			session.tick(0.1).unwrap();
			assert_eq!(session.pending_runtime_spawns().len(), 32);
			let requests = session.pending_runtime_spawns();
			assert_eq!(requests.first().unwrap().id, 1);
			assert_eq!(requests.last().unwrap().id, 32);
		} else {
			assert!(
				session
					.tick(0.1)
					.unwrap_err()
					.contains("pending spawn budget")
			);
			assert!(session.pending_runtime_spawns().is_empty());
			assert_eq!(inspection(&session), before);
		}
	}
}

#[test]
fn active_behavior_admission_limit_is_4096_and_counts_only_enabled_attachments() {
	let mut entities = Vec::new();
	for index in 0..512 {
		let mut value = emitter(&format!("owner-{index}"));
		let template = value.native_behaviors[0].clone();
		value.native_behaviors = (0..8)
			.map(|component| NativeBehaviorComponent {
				id: ComponentId::new(format!("emitter-{component}")).unwrap(),
				..template.clone()
			})
			.collect();
		entities.push(value);
	}
	let mut data = scene(entities);
	data.validate().unwrap();
	let initial = Arc::new(data.clone());
	let session = SimulationSession::new(project(&data), initial).unwrap();
	let one_more = prefab(vec![emitter("overflow")]);
	assert!(
		session
			.prepare_runtime_spawn(&one_more, HASH, [0.0; 3], None)
			.err()
			.unwrap()
			.contains("4096")
	);
	let mut disabled = emitter("disabled");
	disabled.native_behaviors[0].enabled = false;
	data.entities.push(disabled);
	data.validate().unwrap();
	data.entities.last_mut().unwrap().native_behaviors[0].enabled = true;
	assert!(data.validate().unwrap_err().contains("4096"));
}

#[test]
fn accepted_spawn_advances_render_revision_and_rejected_spawn_publishes_nothing() {
	let data = scene(vec![emitter("spawner")]);
	let mut engine = EngineBuilder::new(data.clone(), project(&data))
		.build()
		.unwrap();
	run(&mut engine, Command::StartPlay);
	let item = prefab(vec![entity("item", [0.0; 3])]);
	let prepared = engine
		.prepare_runtime_spawn(&item, HASH, [0.0; 3], None)
		.unwrap();
	let stale = engine
		.prepare_runtime_spawn(&item, HASH, [0.0; 3], None)
		.unwrap();
	let revision = engine.session().scene_revision();
	let authored_revision = engine.session().revision();
	let cursor = engine.change_cursor();
	engine.commit_runtime_spawn(prepared).unwrap();
	assert_eq!(engine.session().scene_revision(), revision + 1);
	assert_eq!(engine.session().revision(), authored_revision);
	assert_eq!(engine.changes_since(cursor).unwrap().events.len(), 1);
	let document = engine.session().render_document();
	let state = serde_json::to_value(engine.runtime_inspection(0, 128).unwrap()).unwrap();
	let cursor = engine.change_cursor();
	assert!(engine.commit_runtime_spawn(stale).is_err());
	assert_eq!(engine.session().scene_revision(), revision + 1);
	assert_eq!(engine.session().render_document(), document);
	assert_eq!(
		serde_json::to_value(engine.runtime_inspection(0, 128).unwrap()).unwrap(),
		state
	);
	assert_eq!(engine.change_cursor(), cursor);
}

#[test]
fn prepared_spawns_cannot_cross_restart_or_scene_swap_and_never_publish_partial_events() {
	let data = scene(vec![emitter("spawner")]);
	let next = scene(vec![entity("destination", [0.0; 3])]);
	let next_id = SceneId::new("next").unwrap();
	let mut project = project(&data);
	project
		.scenes
		.insert(SceneDescriptor::new(next_id.clone(), "scenes/next.json").unwrap())
		.unwrap();
	let mut engine = EngineBuilder::new(data, project)
		.with_scenes(vec![(next_id.clone(), next.clone())])
		.build()
		.unwrap();
	run(&mut engine, Command::StartPlay);
	let item = prefab(vec![entity("item", [0.0; 3])]);
	let token = engine
		.prepare_runtime_spawn(&item, HASH, [0.0; 3], None)
		.unwrap();
	let status = engine.play_status().unwrap();
	let (restart, _) = engine
		.prepare_restart_play(&status.run_id, engine.session().revision())
		.unwrap();
	engine.commit_play_scene(restart).unwrap();
	let cursor = engine.change_cursor();
	let document = engine.session().render_document();
	assert!(engine.commit_runtime_spawn(token).is_err());
	assert_eq!(engine.change_cursor(), cursor);
	assert_eq!(engine.session().render_document(), document);
	let token = engine
		.prepare_runtime_spawn(&item, HASH, [0.0; 3], None)
		.unwrap();
	let transition = engine
		.session()
		.prepare_play_scene(next_id, Arc::new(next))
		.unwrap();
	engine.commit_play_scene(transition).unwrap();
	let cursor = engine.change_cursor();
	let document = engine.session().render_document();
	assert!(engine.commit_runtime_spawn(token).is_err());
	assert_eq!(engine.change_cursor(), cursor);
	assert_eq!(engine.session().render_document(), document);
	assert_eq!(engine.runtime_inspection(0, 128).unwrap().timer_count, 0);
}

#[test]
fn stop_after_spawn_preserves_authored_drafts_component_configuration_and_undo() {
	let data = scene(vec![emitter("spawner")]);
	let mut engine = EngineBuilder::new(data.clone(), project(&data))
		.build()
		.unwrap();
	run(
		&mut engine,
		Command::Rename {
			key: "spawner".into(),
			name: "Unsaved emitter".into(),
		},
	);
	let draft = engine.session().authored_document();
	run(&mut engine, Command::StartPlay);
	engine.tick(0.1).unwrap();
	let request = engine.pending_runtime_spawns().remove(0);
	let item = prefab(vec![entity("item", [0.0; 3])]);
	let token = engine
		.prepare_runtime_spawn(&item, HASH, request.position, Some(request.id))
		.unwrap();
	engine.commit_runtime_spawn(token).unwrap();
	assert_eq!(engine.runtime_inspection(0, 128).unwrap().timer_count, 1);
	run(&mut engine, Command::StopPlay);
	assert_eq!(engine.session().render_document(), draft);
	assert_eq!(engine.session().authored_document(), draft);
	assert!(engine.session().is_dirty());
	assert!(engine.pending_runtime_spawns().is_empty());
	assert!(engine.runtime_inspection(0, 128).is_err());
	run(&mut engine, Command::Undo);
	assert_eq!(engine.session().authored_document().as_ref(), &data);
}

#[test]
fn despawning_request_owner_cancels_prepared_child_and_its_pending_request() {
	let data = Arc::new(scene(vec![entity("authored", [0.0; 3])]));
	let script = Script::default();
	let mut session = SimulationSession::with_factory(project(&data), data, &script).unwrap();
	let token = session
		.prepare_runtime_spawn(&prefab(vec![emitter("owner")]), HASH, [0.0; 3], None)
		.unwrap();
	let owner = token.entities()[0].key.clone();
	session.commit_runtime_spawn(token).unwrap();
	session.tick(0.1).unwrap();
	let request = session.pending_runtime_spawns().remove(0);
	let child = session
		.prepare_runtime_spawn(
			&prefab(vec![entity("child", [0.0; 3])]),
			HASH,
			request.position,
			Some(request.id),
		)
		.unwrap();
	*script.despawn.lock().unwrap() = session.runtime_handle(&owner);
	session.tick(0.1).unwrap();
	assert!(session.pending_runtime_spawns().is_empty());
	assert!(session.commit_runtime_spawn(child).is_err());
	assert_eq!(session.world().data().entities.len(), 1);
	assert_eq!(inspection(&session)["timer_count"], 0);
}

#[test]
fn missing_gpu_capability_rejection_preserves_live_timers_requests_world_and_event_cursor() {
	let data = scene(vec![emitter("spawner")]);
	let mut engine = EngineBuilder::new(data.clone(), project(&data))
		.build()
		.unwrap();
	run(&mut engine, Command::StartPlay);
	engine
		.set_play_visibility_capabilities([key("spawner")].into_iter().collect())
		.unwrap();
	let item = prefab(vec![entity("item", [0.0; 3])]);
	engine.tick(0.1).unwrap();
	let request = engine.pending_runtime_spawns().remove(0);
	let mut first = engine
		.prepare_runtime_spawn(&item, HASH, request.position, Some(request.id))
		.unwrap();
	first
		.set_visibility_capabilities(
			first
				.entities()
				.iter()
				.map(|entity| entity.key.clone())
				.collect(),
		)
		.unwrap();
	engine.commit_runtime_spawn(first).unwrap();
	engine.tick(0.1).unwrap();
	let request = engine.pending_runtime_spawns().remove(0);
	let rejected = engine
		.prepare_runtime_spawn(&item, HASH, request.position, Some(request.id))
		.unwrap();
	let state = serde_json::to_value(engine.runtime_inspection(0, 128).unwrap()).unwrap();
	let document = engine.session().render_document();
	let revision = engine.session().scene_revision();
	let cursor = engine.change_cursor();
	assert!(
		engine
			.commit_runtime_spawn(rejected)
			.unwrap_err()
			.message
			.contains("render resources")
	);
	assert_eq!(engine.change_cursor(), cursor);
	assert_eq!(engine.session().scene_revision(), revision);
	assert_eq!(engine.session().render_document(), document);
	assert_eq!(
		serde_json::to_value(engine.runtime_inspection(0, 128).unwrap()).unwrap(),
		state
	);
	assert_eq!(state["timer_count"], 1);
	assert_eq!(state["pending_spawns"], 1);
}

#[test]
fn invalid_world_collider_candidate_cannot_publish_or_change_lifecycle_ownership() {
	let data = scene(vec![emitter("spawner")]);
	let mut engine = EngineBuilder::new(data.clone(), project(&data))
		.build()
		.unwrap();
	run(&mut engine, Command::StartPlay);
	engine.tick(0.1).unwrap();
	let request = engine.pending_runtime_spawns().remove(0);
	let first = engine
		.prepare_runtime_spawn(
			&prefab(vec![entity("item", [0.0; 3])]),
			HASH,
			request.position,
			Some(request.id),
		)
		.unwrap();
	engine.commit_runtime_spawn(first).unwrap();
	let mut oversized = entity("oversized", [0.0; 3]);
	oversized.transform.scale = [2.0; 3];
	oversized.collider = Some(ColliderData::Sphere {
		radius: f32::MAX,
		offset: [0.0; 3],
		interaction: ColliderInteraction::default(),
	});
	// Local numbers are finite. The failure is in prepared world-space collider construction.
	let candidate = engine
		.prepare_runtime_spawn(&prefab(vec![oversized]), HASH, [0.0; 3], None)
		.unwrap();
	let state = serde_json::to_value(engine.runtime_inspection(0, 128).unwrap()).unwrap();
	let document = engine.session().render_document();
	let revision = engine.session().scene_revision();
	let cursor = engine.change_cursor();
	assert!(engine.commit_runtime_spawn(candidate).is_err());
	assert_eq!(engine.change_cursor(), cursor);
	assert_eq!(engine.session().scene_revision(), revision);
	assert_eq!(engine.session().render_document(), document);
	assert_eq!(
		serde_json::to_value(engine.runtime_inspection(0, 128).unwrap()).unwrap(),
		state
	);
	assert_eq!(state["timer_count"], 1);
}
