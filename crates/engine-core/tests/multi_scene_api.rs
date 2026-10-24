mod support;
use engine_core::{
	Engine, EngineBuilder,
	command::{EditorCommandDto as Command, EditorCommandRequest},
	project::{BehaviorSpec, ProjectData, SceneBehavior},
	scene_catalog::{SceneDescriptor, SceneId},
};
use support::*;

fn fixture() -> (Engine, SceneId, SceneId) {
	let first = scene(vec![entity("actor", [0.0; 3])]);
	let second = scene(vec![entity("actor", [10.0, 0.0, 0.0])]);
	let mut project = ProjectData::for_scene("Two scenes", "scenes/first.json", &first).unwrap();
	let first_id = project.startup_scene_id.clone();
	let second_id = SceneId::new("second").unwrap();
	project
		.scenes
		.insert(SceneDescriptor::new(second_id.clone(), "scenes/second.json").unwrap())
		.unwrap();
	for (scene_id, speed) in [(&first_id, 1.0), (&second_id, 5.0)] {
		project.behaviors.push(SceneBehavior {
			scene_id: scene_id.clone(),
			behavior: BehaviorSpec::ControlledMove {
				entity: key("actor"),
				speed,
				space: Default::default(),
			},
		});
	}
	let engine = EngineBuilder::new(first, project)
		.with_scenes(vec![(second_id.clone(), second)])
		.build()
		.unwrap();
	(engine, first_id, second_id)
}
fn run(engine: &mut Engine, command: Command) {
	engine
		.execute(EditorCommandRequest {
			expected_revision: engine.session().revision(),
			command,
		})
		.unwrap();
}
fn switch(engine: &mut Engine, id: &SceneId) {
	run(
		engine,
		Command::SwitchScene {
			scene_id: id.clone(),
		},
	);
}
fn rejected_without_mutation(engine: &mut Engine, command: Command) {
	let before = serde_json::to_value(engine.snapshot().unwrap()).unwrap();
	let cursor = engine.change_cursor();
	assert!(
		engine
			.execute(EditorCommandRequest {
				expected_revision: engine.session().revision(),
				command
			})
			.is_err()
	);
	assert_eq!(
		serde_json::to_value(engine.snapshot().unwrap()).unwrap(),
		before
	);
	assert_eq!(engine.change_cursor(), cursor);
}

#[test]
fn switching_retains_drafts_and_saved_baselines_with_one_shared_scene_aware_history() {
	let (mut engine, first, second) = fixture();
	run(
		&mut engine,
		Command::Rename {
			key: "actor".into(),
			name: "First draft".into(),
		},
	);
	switch(&mut engine, &second);
	assert_eq!(engine.snapshot().unwrap().project.startup_scene_id, first);
	run(
		&mut engine,
		Command::Rename {
			key: "actor".into(),
			name: "Second saved".into(),
		},
	);
	engine.mark_scene_saved().unwrap();
	let snapshot = engine.snapshot().unwrap();
	assert!(snapshot.dirty);
	assert!(!snapshot.project_dirty);
	assert!(
		snapshot
			.scenes
			.iter()
			.find(|entry| entry.id == first)
			.unwrap()
			.dirty
	);
	assert!(
		!snapshot
			.scenes
			.iter()
			.find(|entry| entry.id == second)
			.unwrap()
			.dirty
	);
	switch(&mut engine, &first);
	assert_eq!(
		engine.snapshot().unwrap().document.entities[0].name,
		"First draft"
	);
	// Switching added no history entry: this undoes the latest edit in scene two.
	run(&mut engine, Command::Undo);
	switch(&mut engine, &second);
	assert_eq!(
		engine.snapshot().unwrap().document.entities[0].name,
		"actor"
	);
	assert!(
		engine
			.snapshot()
			.unwrap()
			.scenes
			.iter()
			.find(|entry| entry.id == second)
			.unwrap()
			.dirty
	);
	run(&mut engine, Command::Undo);
	switch(&mut engine, &first);
	assert_eq!(
		engine.snapshot().unwrap().document.entities[0].name,
		"actor"
	);
	run(&mut engine, Command::Redo);
	run(&mut engine, Command::Redo);
	switch(&mut engine, &second);
	assert_eq!(
		engine.snapshot().unwrap().document.entities[0].name,
		"Second saved"
	);
	assert!(
		!engine
			.snapshot()
			.unwrap()
			.scenes
			.iter()
			.find(|entry| entry.id == second)
			.unwrap()
			.dirty
	);
}

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
fn startup_and_active_scene_are_independent_and_removal_guards_preserve_state() {
	let (mut engine, first, second) = fixture();
	rejected_without_mutation(
		&mut engine,
		Command::SwitchScene {
			scene_id: SceneId::new("missing").unwrap(),
		},
	);
	rejected_without_mutation(
		&mut engine,
		Command::RemoveScene {
			scene_id: first.clone(),
		},
	);
	run(
		&mut engine,
		Command::SetStartupScene {
			scene_id: second.clone(),
		},
	);
	assert_eq!(engine.snapshot().unwrap().active_scene_id, first);
	assert_eq!(engine.snapshot().unwrap().project.startup_scene_id, second);
	rejected_without_mutation(
		&mut engine,
		Command::RemoveScene {
			scene_id: second.clone(),
		},
	);
	run(&mut engine, Command::Undo);
	switch(&mut engine, &second);
	run(
		&mut engine,
		Command::Rename {
			key: "actor".into(),
			name: "Dirty second".into(),
		},
	);
	switch(&mut engine, &first);
	rejected_without_mutation(
		&mut engine,
		Command::RemoveScene {
			scene_id: second.clone(),
		},
	);
	run(&mut engine, Command::Undo);
	switch(&mut engine, &first);
	run(
		&mut engine,
		Command::RemoveScene {
			scene_id: second.clone(),
		},
	);
	assert!(engine.session().project().scenes.get(&second).is_err());
	run(&mut engine, Command::Undo);
	switch(&mut engine, &second);
	assert_eq!(
		engine.snapshot().unwrap().document.entities[0]
			.transform
			.position[0],
		10.0
	);
	assert_eq!(engine.session().project().behaviors_for(&second).count(), 1);
}

#[test]
fn adding_a_scene_is_undoable_and_failed_catalog_changes_preserve_documents_and_history() {
	let (mut engine, first, second) = fixture();
	let document = scene(vec![entity("new", [1.0, 2.0, 3.0])]);
	for (id, asset_path) in [
		(first.clone(), "scenes/new.json"),
		(SceneId::new("new").unwrap(), "scenes/second.json"),
	] {
		rejected_without_mutation(
			&mut engine,
			Command::AddScene {
				id,
				asset_path: asset_path.into(),
				document: document.clone(),
			},
		);
	}
	let added = SceneId::new("added").unwrap();
	run(
		&mut engine,
		Command::AddScene {
			id: added.clone(),
			asset_path: "scenes/added.json".into(),
			document: document.clone(),
		},
	);
	switch(&mut engine, &added);
	assert!(
		engine
			.snapshot()
			.unwrap()
			.scenes
			.iter()
			.find(|entry| entry.id == added)
			.unwrap()
			.dirty
	);
	assert!(engine.snapshot().unwrap().project_dirty);
	run(&mut engine, Command::Undo);
	assert!(engine.session().project().scenes.get(&added).is_err());
	assert!(engine.session().project().scenes.get(&second).is_ok());
	run(&mut engine, Command::Redo);
	switch(&mut engine, &added);
	assert_eq!(engine.snapshot().unwrap().document, document);
	run(
		&mut engine,
		Command::BeginTransaction {
			label: "Preview".into(),
		},
	);
	rejected_without_mutation(&mut engine, Command::SwitchScene { scene_id: first });
	run(&mut engine, Command::CancelTransaction);
}

#[test]
fn removed_scene_ids_cannot_be_reused_while_their_original_draft_remains_undoable() {
	let (mut engine, _, second) = fixture();
	run(
		&mut engine,
		Command::RemoveScene {
			scene_id: second.clone(),
		},
	);
	rejected_without_mutation(
		&mut engine,
		Command::AddScene {
			id: second.clone(),
			asset_path: "scenes/replacement.json".into(),
			document: scene(vec![entity("replacement", [99.0, 0.0, 0.0])]),
		},
	);
	run(&mut engine, Command::Undo);
	switch(&mut engine, &second);
	assert_eq!(
		engine
			.snapshot()
			.unwrap()
			.project
			.scene_path(&second)
			.unwrap(),
		"scenes/second.json"
	);
	assert_eq!(
		engine.snapshot().unwrap().document.entities[0].key,
		key("actor")
	);
	assert_eq!(
		engine.snapshot().unwrap().document.entities[0]
			.transform
			.position[0],
		10.0
	);
	assert_eq!(engine.session().project().behaviors_for(&second).count(), 1);
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

#[test]
fn legacy_single_document_replacement_rejects_multi_scene_workspaces_without_losing_drafts() {
	let (mut engine, first, second) = fixture();
	switch(&mut engine, &second);
	run(
		&mut engine,
		Command::Rename {
			key: "actor".into(),
			name: "Retain inactive draft".into(),
		},
	);
	switch(&mut engine, &first);
	let before = serde_json::to_value(engine.snapshot().unwrap()).unwrap();
	let cursor = engine.change_cursor();
	assert!(engine.replace_document(scene(vec![])).is_err());
	assert_eq!(
		serde_json::to_value(engine.snapshot().unwrap()).unwrap(),
		before
	);
	assert_eq!(engine.change_cursor(), cursor);
	switch(&mut engine, &second);
	assert_eq!(
		engine.snapshot().unwrap().document.entities[0].name,
		"Retain inactive draft"
	);
}

#[test]
fn loading_saved_scene_batches_rejects_partial_invalid_members_without_retaining_any_documents() {
	use engine_core::editor::EditorSession;
	let startup = scene(vec![]);
	let mut project = ProjectData::for_scene("Batch load", "scenes/main.json", &startup).unwrap();
	let active = project.startup_scene_id.clone();
	let first = SceneId::new("first").unwrap();
	let second = SceneId::new("second").unwrap();
	for id in [&first, &second] {
		project
			.scenes
			.insert(
				SceneDescriptor::new(id.clone(), format!("scenes/{}.json", id.as_str())).unwrap(),
			)
			.unwrap();
	}
	let mut invalid = startup.clone();
	invalid.camera.near_plane = -1.0;
	for tail in [
		(active.clone(), startup.clone()),
		(first.clone(), startup.clone()),
		(SceneId::new("unknown").unwrap(), startup.clone()),
		(second.clone(), invalid),
	] {
		let mut session = EditorSession::for_project(startup.clone(), project.clone()).unwrap();
		let before = serde_json::to_value(session.snapshot().unwrap()).unwrap();
		assert!(
			session
				.load_saved_scenes(vec![(first.clone(), startup.clone()), tail])
				.is_err()
		);
		assert_eq!(
			serde_json::to_value(session.snapshot().unwrap()).unwrap(),
			before
		);
		assert!(session.scene_document(&first).is_err());
		assert!(session.scene_document(&second).is_err());
		session
			.load_saved_scenes(vec![
				(first.clone(), startup.clone()),
				(second.clone(), startup.clone()),
			])
			.unwrap();
		assert!(
			session
				.scene_states()
				.iter()
				.all(|state| state.loaded && !state.dirty)
		);
		assert!(!session.is_dirty());
	}
}

#[test]
fn saving_an_inactive_scene_updates_only_its_baseline_without_navigation_or_history_loss() {
	let (mut engine, first, second) = fixture();
	switch(&mut engine, &second);
	run(
		&mut engine,
		Command::Rename {
			key: "actor".into(),
			name: "Second draft".into(),
		},
	);
	switch(&mut engine, &first);
	run(
		&mut engine,
		Command::Rename {
			key: "actor".into(),
			name: "First draft".into(),
		},
	);
	let before = engine.snapshot().unwrap();
	let receipt = engine.mark_scene_saved_by_id(&second).unwrap();
	assert_eq!(receipt.active_scene_id, first);
	assert_eq!(receipt.affected_scenes, vec![second.clone()]);
	assert!(receipt.changes.is_empty());
	let after = engine.snapshot().unwrap();
	assert_eq!(after.document, before.document);
	assert!(after.revision > before.revision);
	assert!(
		after
			.scenes
			.iter()
			.find(|state| state.id == first)
			.unwrap()
			.dirty
	);
	assert!(
		!after
			.scenes
			.iter()
			.find(|state| state.id == second)
			.unwrap()
			.dirty
	);
	assert!(after.can_undo);
	let cursor = engine.change_cursor();
	assert!(
		!engine
			.mark_scene_saved_by_id(&second)
			.unwrap()
			.state_changed
	);
	assert_eq!(engine.change_cursor(), cursor);
	let snapshot = serde_json::to_value(engine.snapshot().unwrap()).unwrap();
	assert!(
		engine
			.mark_scene_saved_by_id(&SceneId::new("missing").unwrap())
			.is_err()
	);
	assert_eq!(
		serde_json::to_value(engine.snapshot().unwrap()).unwrap(),
		snapshot
	);
	assert_eq!(engine.change_cursor(), cursor);
	run(&mut engine, Command::Undo);
	assert_eq!(
		engine.snapshot().unwrap().document.entities[0].name,
		"actor"
	);
	run(&mut engine, Command::Undo);
	assert_eq!(engine.session().active_scene_id(), &second);
	assert!(
		engine
			.snapshot()
			.unwrap()
			.scenes
			.iter()
			.find(|state| state.id == second)
			.unwrap()
			.dirty
	);
}

#[test]
fn same_scene_navigation_is_a_noop_and_grouped_edits_block_navigation_until_cancelled() {
	let (mut engine, first, second) = fixture();
	run(
		&mut engine,
		Command::Select {
			keys: vec!["actor".into()],
		},
	);
	let before = serde_json::to_value(engine.snapshot().unwrap()).unwrap();
	let cursor = engine.change_cursor();
	switch(&mut engine, &first);
	assert_eq!(
		serde_json::to_value(engine.snapshot().unwrap()).unwrap(),
		before
	);
	assert_eq!(engine.change_cursor(), cursor);
	run(
		&mut engine,
		Command::BeginTransaction {
			label: "Scene draft".into(),
		},
	);
	rejected_without_mutation(
		&mut engine,
		Command::SwitchScene {
			scene_id: second.clone(),
		},
	);
	run(&mut engine, Command::CancelTransaction);
	switch(&mut engine, &second);
	assert!(engine.snapshot().unwrap().selection.is_empty());
}
