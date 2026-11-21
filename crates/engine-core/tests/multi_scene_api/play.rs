use super::*;

#[test]
fn behavior_targets_and_play_are_scoped_to_active_scene_even_when_entity_keys_overlap() {
	let (mut engine, first, second) = fixture();
	run(
		&mut engine,
		Command::Select {
			keys: vec!["actor".into()],
		},
	);
	run(&mut engine, Command::DeleteSelection);
	assert_eq!(engine.session().project().behaviors_for(&first).count(), 0);
	assert_eq!(engine.session().project().behaviors_for(&second).count(), 1);
	run(&mut engine, Command::Undo);
	assert_eq!(engine.session().project().behaviors_for(&first).count(), 1);
	switch(&mut engine, &second);
	run(&mut engine, Command::StartPlay);
	rejected_without_mutation(
		&mut engine,
		Command::SwitchScene {
			scene_id: first.clone(),
		},
	);
	run(
		&mut engine,
		Command::SetEntityInput {
			entity: "actor".into(),
			movement: [1.0, 0.0, 0.0],
		},
	);
	engine.tick(0.1).unwrap();
	close(
		engine.session().render_document().entities[0]
			.transform
			.position[0],
		10.5,
	);
	run(&mut engine, Command::StopPlay);
	assert_eq!(
		engine.snapshot().unwrap().document.entities[0]
			.transform
			.position[0],
		10.0
	);
}

#[test]
fn receipts_identify_scenes_affected_by_behavior_and_startup_changes() {
	let (mut engine, first, second) = fixture();
	let mut project = engine.session().project().clone();
	project
		.behaviors
		.iter_mut()
		.find(|entry| entry.scene_id == second)
		.unwrap()
		.behavior = BehaviorSpec::Spin {
		entity: key("actor"),
		angular_velocity: [0.0, 1.0, 0.0],
	};
	let changed = engine
		.execute(EditorCommandRequest {
			expected_revision: engine.session().revision(),
			command: Command::SetProject { project },
		})
		.unwrap();
	assert_eq!(changed.active_scene_id, first);
	assert!(changed.affected_scenes.contains(&second));
	let startup = engine
		.execute(EditorCommandRequest {
			expected_revision: engine.session().revision(),
			command: Command::SetStartupScene {
				scene_id: second.clone(),
			},
		})
		.unwrap();
	assert_eq!(startup.active_scene_id, first);
	assert!(startup.affected_scenes.contains(&first));
	assert!(startup.affected_scenes.contains(&second));
}
