use super::*;

/// Model the adapter protocol without GPU/filesystem variability: admit every
/// complete candidate after its issuing step and drain only retained work.
fn host_frame(session: &mut SimulationSession, item: &PrefabData, elapsed: f64) -> (u32, f64) {
	let mut elapsed = elapsed;
	let mut total_steps = 0;
	let mut dropped = 0.0;
	for _ in 0..8 {
		let outcome = session.tick(elapsed).unwrap();
		total_steps += outcome.steps;
		dropped += outcome.dropped_seconds;
		for request in session.pending_runtime_spawns() {
			let prepared = session
				.prepare_runtime_spawn(item, HASH, request.position, Some(request.id))
				.unwrap();
			session.commit_runtime_spawn(prepared).unwrap();
		}
		if outcome.pending_steps == 0 {
			return (total_steps, dropped);
		}
		assert!(outcome.steps > 0, "a barrier must make fixed-step progress");
		elapsed = 0.0;
	}
	panic!("drain exceeded the bounded eight-step catch-up allowance");
}

fn stable_inspection(session: &SimulationSession) -> serde_json::Value {
	let mut value = inspection(session);
	for entity in value["entities"].as_array_mut().unwrap() {
		entity["handle"].as_object_mut().unwrap().remove("world_id");
	}
	value
}

#[test]
fn host_admission_barrier_makes_spawned_ids_timers_and_state_independent_of_frame_partition() {
	let data = Arc::new(scene(vec![emitter("spawner")]));
	let project = project(&data);
	let mut whole = SimulationSession::new(project.clone(), data.clone()).unwrap();
	let mut split = SimulationSession::new(project, data).unwrap();
	let item = prefab(vec![entity("item", [0.0; 3])]);
	assert_eq!(host_frame(&mut whole, &item, 0.2), (2, 0.0));
	assert_eq!(host_frame(&mut split, &item, 0.1), (1, 0.0));
	assert_eq!(host_frame(&mut split, &item, 0.1), (1, 0.0));
	assert_eq!(whole.world().data(), split.world().data());
	assert_eq!(stable_inspection(&whole), stable_inspection(&split));
	let state = stable_inspection(&whole);
	assert_eq!(state["timer_count"], 2);
	assert_eq!(state["total_entities"], 3);
	assert_eq!(state["entities"][1]["handle"]["key"], "runtime-1-0");
	assert_eq!(state["entities"][2]["handle"]["key"], "runtime-2-0");
}

#[test]
fn spawn_barriers_preserve_fractional_time_and_charge_clamping_and_catchup_drops_once() {
	let data = Arc::new(scene(vec![emitter("spawner")]));
	let mut project = project(&data);
	project.fixed_hz = 100;
	project.max_catchup_steps = 3;
	let mut clamped = SimulationSession::new(project.clone(), data.clone()).unwrap();
	let mut split = SimulationSession::new(project.clone(), data.clone()).unwrap();
	let item = prefab(vec![entity("item", [0.0; 3])]);
	let (steps, dropped) = host_frame(&mut clamped, &item, 1.0);
	assert_eq!(steps, 3);
	assert!((dropped - 0.97).abs() < 1e-9);
	assert_eq!(
		clamped.tick(0.0).unwrap().steps,
		0,
		"accepted and discarded work must not survive a complete drain"
	);
	for _ in 0..3 {
		assert_eq!(host_frame(&mut split, &item, 0.01), (1, 0.0));
	}
	assert_eq!(stable_inspection(&clamped), stable_inspection(&split));
	let mut fractional = SimulationSession::new(project, data).unwrap();
	assert_eq!(host_frame(&mut fractional, &item, 0.025), (2, 0.0));
	assert_eq!(host_frame(&mut fractional, &item, 0.005), (1, 0.0));
	assert_eq!(stable_inspection(&fractional), stable_inspection(&split));
}

#[test]
fn pause_discards_deferred_spawn_steps_and_does_not_resume_them_on_zero_elapsed() {
	let data = Arc::new(scene(vec![emitter("spawner")]));
	let mut session = SimulationSession::new(project(&data), data).unwrap();
	let outcome = session.tick(0.2).unwrap();
	assert_eq!(
		(
			outcome.steps,
			outcome.pending_steps,
			outcome.dropped_seconds
		),
		(1, 1, 0.0)
	);
	session.set_paused(true);
	let paused = session.tick(0.0).unwrap();
	assert_eq!((paused.steps, paused.pending_steps), (0, 0));
	session.set_paused(false);
	let resumed = session.tick(0.0).unwrap();
	assert_eq!((resumed.steps, resumed.pending_steps), (0, 0));
	assert_eq!(session.tick_index(), 1);
	let item = prefab(vec![entity("item", [0.0; 3])]);
	assert_eq!(host_frame(&mut session, &item, 0.1), (1, 0.0));
	assert_eq!(session.tick_index(), 2);
}

#[test]
fn scene_request_precedes_spawn_barrier_and_discards_remaining_scene_steps() {
	let data = Arc::new(scene(vec![emitter("spawner")]));
	let mut project = project(&data);
	let next = SceneId::new("next").unwrap();
	project
		.scenes
		.insert(SceneDescriptor::new(next.clone(), "scenes/next.json").unwrap())
		.unwrap();
	let script = Script::default();
	*script.request_scene.lock().unwrap() = Some(next.clone());
	let mut session = SimulationSession::with_factory(project, data, &script).unwrap();
	let outcome = session.tick(0.2).unwrap();
	assert_eq!(outcome.requested_scene, Some(next));
	assert_eq!((outcome.steps, outcome.pending_steps), (1, 0));
	assert!((outcome.dropped_seconds - 0.1).abs() < 1e-9);
	assert_eq!(session.pending_runtime_spawns().len(), 1);
	assert_eq!(session.tick(0.0).unwrap().steps, 0);
}
