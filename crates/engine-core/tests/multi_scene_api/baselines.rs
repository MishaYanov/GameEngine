use super::*;

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
