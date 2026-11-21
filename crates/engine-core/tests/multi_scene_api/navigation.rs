use super::*;

#[test]
fn each_scene_remembers_its_selection_without_dirtying_documents_or_adding_history() {
	let (mut engine, first, second) = fixture();
	let first_document = engine.session().scene_document(&first).unwrap();
	let second_document = engine.session().scene_document(&second).unwrap();
	select(&mut engine, &["actor"]);
	switch(&mut engine, &second);
	assert!(engine.session().selection().is_empty());
	select(&mut engine, &["actor"]);
	switch(&mut engine, &first);
	assert_eq!(engine.session().selection(), &[key("actor")]);
	select(&mut engine, &[]);
	switch(&mut engine, &second);
	assert_eq!(engine.session().selection(), &[key("actor")]);
	switch(&mut engine, &first);
	assert!(engine.session().selection().is_empty());

	let snapshot = engine.snapshot().unwrap();
	assert!(!snapshot.dirty);
	assert!(!snapshot.project_dirty);
	assert!(snapshot.scenes.iter().all(|state| !state.dirty));
	assert!(!snapshot.can_undo && !snapshot.can_redo);
	assert_eq!(
		engine.session().scene_document(&first).unwrap(),
		first_document
	);
	assert_eq!(
		engine.session().scene_document(&second).unwrap(),
		second_document
	);
}

#[test]
fn added_loaded_and_reopened_scene_documents_start_with_empty_selections() {
	let (mut engine, first, second) = fixture();
	assert!(engine.session().selection().is_empty());
	select(&mut engine, &["actor"]);
	switch(&mut engine, &second);
	assert!(engine.session().selection().is_empty());
	select(&mut engine, &["actor"]);

	// Reopening constructs a fresh session from authored documents and project data.
	let mut reopened = EngineBuilder::new(
		(*engine.session().scene_document(&first).unwrap()).clone(),
		engine.session().project().clone(),
	)
	.with_scenes(vec![(
		second.clone(),
		(*engine.session().scene_document(&second).unwrap()).clone(),
	)])
	.build()
	.unwrap();
	assert_eq!(reopened.session().active_scene_id(), &first);
	assert!(reopened.session().selection().is_empty());
	switch(&mut reopened, &second);
	assert!(reopened.session().selection().is_empty());

	let added = SceneId::new("new-selection-scene").unwrap();
	run(
		&mut engine,
		Command::AddScene {
			id: added.clone(),
			asset_path: "scenes/new-selection-scene.json".into(),
			document: scene(vec![entity("new-actor", [0.0; 3])]),
		},
	);
	switch(&mut engine, &added);
	assert!(engine.session().selection().is_empty());
	switch(&mut engine, &second);
	assert_eq!(engine.session().selection(), &[key("actor")]);
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
