use engine_core::{
	command::{EditorCommandDto as Command, EditorCommandRequest},
	editor::EditorSession,
	project::ProjectData,
	scene_catalog::{SceneDescriptor, SceneId},
};
use std::sync::Arc;

mod support;
use support::*;

fn run(session: &mut EditorSession, command: Command) {
	session
		.execute_command(EditorCommandRequest {
			expected_revision: session.revision(),
			command,
		})
		.unwrap();
}

fn assert_state_matches_snapshot(session: &EditorSession) {
	let state = session.state();
	let snapshot = session.snapshot().unwrap();
	assert_eq!(state.dirty, session.is_dirty());
	assert_eq!(state.dirty, snapshot.dirty);
	assert_eq!(state.project_dirty, snapshot.project_dirty);
	assert_eq!(
		serde_json::to_value(state.scenes).unwrap(),
		serde_json::to_value(snapshot.scenes).unwrap()
	);
}

#[test]
fn dirty_queries_compare_contents_after_undo_and_redo_recreate_saved_snapshots() {
	let mut session = EditorSession::new(scene(vec![entity("actor", [0.0; 3])])).unwrap();
	let saved = session.authored_document();
	let saved_project = session.project_document();
	run(
		&mut session,
		Command::Rename {
			key: "actor".into(),
			name: "changed".into(),
		},
	);
	assert!(session.is_dirty());
	run(&mut session, Command::Undo);
	let restored = session.authored_document();
	assert!(!Arc::ptr_eq(&saved, &restored));
	assert_eq!(saved, restored);
	assert!(!session.is_dirty());
	assert!(!session.active_scene_dirty());
	assert_state_matches_snapshot(&session);
	let revision = session.revision();
	session.mark_saved().unwrap();
	assert_eq!(session.revision(), revision);
	run(&mut session, Command::Redo);
	session.mark_scene_saved().unwrap();
	run(&mut session, Command::Undo);
	assert!(session.is_dirty());
	run(&mut session, Command::Redo);
	assert!(!session.is_dirty());
	assert_state_matches_snapshot(&session);

	let mut project = session.project().clone();
	project.name = "new project name".into();
	run(&mut session, Command::SetProject { project });
	assert!(session.state().project_dirty);
	run(&mut session, Command::Undo);
	assert!(!Arc::ptr_eq(&saved_project, &session.project_document()));
	assert_eq!(saved_project, session.project_document());
	assert!(!session.state().project_dirty);
	assert_state_matches_snapshot(&session);
}

#[test]
fn scene_queries_keep_saved_inactive_and_unloaded_states_independent() {
	let main = SceneId::new("main").unwrap();
	let second = SceneId::new("second").unwrap();
	let unloaded = SceneId::new("unloaded").unwrap();
	let data = scene(vec![entity("actor", [0.0; 3])]);
	let mut project =
		ProjectData::for_scene_with_id("Test", main.clone(), "scenes/main.json", &data).unwrap();
	for id in [&second, &unloaded] {
		project
			.scenes
			.insert(
				SceneDescriptor::new(id.clone(), format!("scenes/{}.json", id.as_str())).unwrap(),
			)
			.unwrap();
	}
	let mut session = EditorSession::for_project(data.clone(), project).unwrap();
	session
		.load_saved_scenes(vec![(second.clone(), data)])
		.unwrap();
	let main_document = session.scene_document(&main).unwrap();
	let second_document = session.scene_document(&second).unwrap();
	run(
		&mut session,
		Command::Select {
			keys: vec!["actor".into()],
		},
	);
	assert!(Arc::ptr_eq(
		&main_document,
		&session.scene_document(&main).unwrap()
	));
	assert!(Arc::ptr_eq(
		&second_document,
		&session.scene_document(&second).unwrap()
	));
	assert!(session.scene_document(&unloaded).is_err());
	let missing = session
		.scene_states()
		.into_iter()
		.find(|state| state.id == unloaded)
		.unwrap();
	assert!(!missing.loaded);
	assert!(!missing.dirty);
	assert!(!session.is_dirty());
	run(
		&mut session,
		Command::Rename {
			key: "actor".into(),
			name: "edited main".into(),
		},
	);
	run(
		&mut session,
		Command::SwitchScene {
			scene_id: second.clone(),
		},
	);
	assert!(!session.active_scene_dirty());
	assert!(session.is_dirty());
	assert_state_matches_snapshot(&session);
	session.mark_scene_saved_by_id(&main).unwrap();
	assert!(!session.is_dirty());
	assert_eq!(session.active_scene_id(), &second);
	let revision = session.revision();
	session.mark_scene_saved_by_id(&main).unwrap();
	assert_eq!(session.revision(), revision);
	run(
		&mut session,
		Command::ChangeScenePath {
			scene_id: main.clone(),
			asset_path: "scenes/renamed.json".into(),
		},
	);
	assert!(session.is_dirty());
	assert!(
		session
			.scene_states()
			.into_iter()
			.find(|state| state.id == main)
			.unwrap()
			.dirty
	);
	run(&mut session, Command::Undo);
	assert!(!session.is_dirty());
	assert_state_matches_snapshot(&session);
}
