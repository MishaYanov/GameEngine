use super::*;

#[test]
fn default_simulation_requires_explicit_factory_for_a_named_module() {
	let data = Arc::new(scene(vec![entity("actor", [0.0; 3])]));
	let mut project = project(&data, vec![]);
	project.gameplay_module = Some("custom_module".into());

	assert!(matches!(
		SimulationSession::new(project.clone(), Arc::clone(&data)),
		Err(message) if message.contains("custom_module")
	));
	assert!(SimulationSession::with_factory(project, data, &ConfiguredBehaviorFactory).is_ok());
}

#[test]
fn nonstartup_simulation_scopes_behavior_and_both_input_routes_to_its_scene() {
	let (project, secondary_id, startup, secondary) = scene_context_fixture();
	let mut session = SimulationSession::with_factory_for_scene(
		project.clone(),
		secondary_id.clone(),
		Arc::clone(&secondary),
		&ConfiguredBehaviorFactory,
	)
	.unwrap();
	assert_eq!(session.scene_id(), &secondary_id);
	assert_eq!(session.project(), &project);
	assert!(!session.needs_tick());
	assert!(
		session
			.set_input(&key("startup-only"), [1.0, 0.0, 0.0])
			.is_err()
	);
	assert!(!session.needs_tick());
	session
		.set_input(&key("secondary-only"), [1.0, 0.0, 0.0])
		.unwrap();
	session.tick(0.1).unwrap();
	close(session.document().entities[0].transform.position[0], 10.0);
	close(session.document().entities[1].transform.position[0], 20.2);
	session.clear_input();
	session.set_controlled_input([1.0, 0.0, 0.0]).unwrap();
	session.tick(0.1).unwrap();
	close(session.document().entities[0].transform.position[0], 10.5);
	close(session.document().entities[1].transform.position[0], 20.4);
	assert_eq!(session.project(), &project);
	assert_eq!(startup.entities[0].transform.position, [0.0; 3]);
	assert_eq!(secondary.entities[0].transform.position, [10.0, 0.0, 0.0]);
}

#[test]
fn factories_receive_the_original_project_and_wrappers_keep_the_startup_default() {
	let (project, secondary_id, startup, secondary) = scene_context_fixture();
	let factory = RecordingFactory::default();
	let chosen = SimulationSession::with_factory_for_scene(
		project.clone(),
		secondary_id.clone(),
		secondary,
		&factory,
	)
	.unwrap();
	let default =
		SimulationSession::with_factory(project.clone(), Arc::clone(&startup), &factory).unwrap();
	let normal = SimulationSession::new(project.clone(), startup).unwrap();
	assert_eq!(chosen.scene_id(), &secondary_id);
	assert_eq!(default.scene_id(), &project.startup_scene_id);
	assert_eq!(normal.scene_id(), &project.startup_scene_id);
	for session in [&chosen, &default, &normal] {
		assert_eq!(session.project(), &project);
	}
	assert_eq!(
		factory.calls.lock().unwrap().as_slice(),
		&[
			(project.clone(), secondary_id),
			(project.clone(), project.startup_scene_id.clone()),
		]
	);
}

#[test]
fn unknown_scene_and_missing_chosen_behavior_targets_fail_before_factory_creation() {
	let (project, secondary_id, startup, secondary) = scene_context_fixture();
	let factory = RecordingFactory::default();
	let missing = SceneId::new("missing").unwrap();
	assert!(
		ConfiguredBehaviorFactory
			.create(&project, &missing)
			.is_err()
	);
	// SceneData has no embedded scene identity. The second case rejects because
	// this document lacks the chosen scene's configured secondary-only target.
	for (scene_id, document) in [(missing, secondary), (secondary_id, startup)] {
		let result = SimulationSession::with_factory_for_scene(
			project.clone(),
			scene_id,
			document,
			&factory,
		);
		assert!(result.is_err());
		assert!(factory.calls.lock().unwrap().is_empty());
	}
}

#[test]
fn editor_play_passes_active_scene_without_rewriting_project_startup_or_authored_state() {
	let (project, secondary_id, startup, secondary) = scene_context_fixture();
	let factory = Arc::new(RecordingFactory::default());
	let mut engine = EngineBuilder::new((*startup).clone(), project.clone())
		.with_scenes(vec![(secondary_id.clone(), (*secondary).clone())])
		.module_factory(factory.clone())
		.build()
		.unwrap();
	for command in [
		Command::SwitchScene {
			scene_id: secondary_id.clone(),
		},
		Command::StartPlay,
		Command::StopPlay,
	] {
		engine
			.execute(EditorCommandRequest {
				expected_revision: engine.session().revision(),
				command,
			})
			.unwrap();
	}
	assert_eq!(
		factory.calls.lock().unwrap().as_slice(),
		&[(project.clone(), secondary_id.clone())]
	);
	assert_eq!(engine.session().active_scene_id(), &secondary_id);
	assert_eq!(engine.session().project(), &project);
	assert_eq!(
		engine
			.session()
			.scene_document(&project.startup_scene_id)
			.unwrap(),
		startup
	);
	assert_eq!(
		engine.session().scene_document(&secondary_id).unwrap(),
		secondary
	);
	let snapshot = engine.snapshot().unwrap();
	assert!(!snapshot.dirty && !snapshot.project_dirty);
	assert!(!snapshot.can_undo && !snapshot.can_redo);
}

#[test]
fn prepared_factory_hook_runs_only_after_project_scene_and_target_validation() {
	let (project, scene_id, wrong_scene, mut chosen_scene) = scene_context_fixture();
	Arc::make_mut(&mut chosen_scene)
		.bindings
		.insert("player".into(), key("secondary-only"));
	let factory = PreparedFactory {
		project: project.clone(),
		scene_id: scene_id.clone(),
		document: Arc::clone(&chosen_scene),
		calls: Mutex::new(0),
	};
	let mut invalid_camera = chosen_scene.as_ref().clone();
	invalid_camera.camera.near_plane = -1.0;
	let mut invalid_binding = chosen_scene.as_ref().clone();
	invalid_binding
		.bindings
		.insert("player".into(), key("startup-only"));
	for document in [
		wrong_scene,
		Arc::new(invalid_camera),
		Arc::new(invalid_binding),
	] {
		assert!(
			SimulationSession::with_factory_for_scene(
				project.clone(),
				scene_id.clone(),
				document,
				&factory
			)
			.is_err()
		);
	}
	let mut invalid_project = project.clone();
	invalid_project.fixed_hz = 0;
	assert!(
		SimulationSession::with_factory_for_scene(
			invalid_project,
			scene_id.clone(),
			Arc::clone(&chosen_scene),
			&factory
		)
		.is_err()
	);
	assert_eq!(*factory.calls.lock().unwrap(), 0);
	let mut session =
		SimulationSession::with_factory_for_scene(project, scene_id, chosen_scene, &factory)
			.unwrap();
	assert_eq!(*factory.calls.lock().unwrap(), 1);
	session.set_controlled_input([1.0, 0.0, 0.0]).unwrap();
	assert_eq!(session.tick(1.0 / 60.0).unwrap().steps, 1);
}
