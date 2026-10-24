mod support;
use engine_core::{
	command::{EditorCommandDto as Command, EditorCommandRequest},
	editor::EditorSession,
	project::{BehaviorSpace, BehaviorSpec, ProjectData},
	simulation_session::SimulationSession,
};
use std::sync::Arc;
use support::*;
fn run(session: &mut EditorSession, command: Command) {
	session
		.execute_command(EditorCommandRequest {
			expected_revision: session.revision(),
			command,
		})
		.unwrap();
}
fn configured(data: &engine_core::SceneData) -> ProjectData {
	let mut project = ProjectData::for_scene("Authored", "scenes/main.json", data).unwrap();
	project.behaviors = vec![BehaviorSpec::ControlledMove {
		entity: key("actor"),
		speed: 2.0,
		space: BehaviorSpace::World,
	}];
	project
}

#[test]
fn editor_play_and_derived_simulation_produce_identical_motion_regardless_of_selection() {
	let data = scene(vec![
		entity("actor", [0.0; 3]),
		entity("selected", [5.0, 0.0, 0.0]),
	]);
	let project = configured(&data);
	let mut editor = EditorSession::for_project(data.clone(), project.clone()).unwrap();
	let mut player = SimulationSession::new(project, Arc::new(data.clone())).unwrap();
	run(
		&mut editor,
		Command::Select {
			keys: vec!["selected".into()],
		},
	);
	run(&mut editor, Command::StartPlay);
	run(
		&mut editor,
		Command::SetEntityInput {
			entity: "actor".into(),
			movement: [1.0, 0.0, 0.0],
		},
	);
	player.set_input(&key("actor"), [1.0, 0.0, 0.0]).unwrap();
	let revision = editor.revision();
	for _ in 0..12 {
		editor.tick(1.0 / 120.0).unwrap();
		player.tick(1.0 / 120.0).unwrap();
	}
	assert_eq!(editor.render_document(), player.document());
	close(
		editor.render_document().entities[0].transform.position[0],
		0.2,
	);
	assert_eq!(editor.render_document().entities[1], data.entities[1]);
	assert_eq!(editor.revision(), revision);
	run(&mut editor, Command::Select { keys: vec![] });
	editor.tick(0.1).unwrap();
	close(
		editor.render_document().entities[0].transform.position[0],
		0.4,
	);
	run(&mut editor, Command::StopPlay);
	assert_eq!(editor.render_document().as_ref(), &data);
	assert!(!editor.snapshot().unwrap().dirty);
}

#[test]
fn project_configuration_is_undoable_groupable_and_rejected_changes_preserve_project_scene_and_revision()
 {
	let data = scene(vec![entity("actor", [0.0; 3])]);
	let mut editor = EditorSession::new(data.clone()).unwrap();
	let original = editor.project().clone();
	assert!(original.behaviors.is_empty());
	let project = configured(&data);
	run(
		&mut editor,
		Command::SetProject {
			project: project.clone(),
		},
	);
	assert_eq!(editor.project(), &project);
	assert!(editor.snapshot().unwrap().dirty);
	run(&mut editor, Command::Undo);
	assert_eq!(editor.project(), &original);
	assert!(!editor.snapshot().unwrap().dirty);
	run(&mut editor, Command::Redo);
	assert_eq!(editor.project(), &project);
	let snapshot = serde_json::to_value(editor.snapshot().unwrap()).unwrap();
	let mut invalid = project;
	invalid.behaviors[0] = BehaviorSpec::ControlledMove {
		entity: key("missing"),
		speed: 1.0,
		space: BehaviorSpace::World,
	};
	assert!(
		editor
			.execute_command(EditorCommandRequest {
				expected_revision: editor.revision(),
				command: Command::SetProject { project: invalid }
			})
			.is_err()
	);
	assert_eq!(
		serde_json::to_value(editor.snapshot().unwrap()).unwrap(),
		snapshot
	);
	run(
		&mut editor,
		Command::BeginTransaction {
			label: "Project and entity".into(),
		},
	);
	let mut changed = editor.project().clone();
	changed.name = "Grouped".into();
	run(&mut editor, Command::SetProject { project: changed });
	run(
		&mut editor,
		Command::Rename {
			key: "actor".into(),
			name: "Grouped actor".into(),
		},
	);
	run(&mut editor, Command::CommitTransaction);
	run(&mut editor, Command::Undo);
	assert_eq!(editor.project().name, "Authored");
	assert_eq!(editor.authored_document().entities[0].name, "actor");
}

#[test]
fn deleting_a_behavior_target_removes_and_undo_restores_its_authored_configuration() {
	let data = scene(vec![entity("actor", [0.0; 3]), entity("other", [0.0; 3])]);
	let project = configured(&data);
	let mut editor = EditorSession::for_project(data.clone(), project.clone()).unwrap();
	run(
		&mut editor,
		Command::Select {
			keys: vec!["actor".into()],
		},
	);
	run(&mut editor, Command::DeleteSelection);
	assert!(editor.project().behaviors.is_empty());
	editor
		.project()
		.validate_scene(&editor.authored_document())
		.unwrap();
	run(&mut editor, Command::Undo);
	assert_eq!(editor.project(), &project);
	assert_eq!(editor.authored_document().as_ref(), &data);
	run(&mut editor, Command::Redo);
	assert!(editor.project().behaviors.is_empty());
}

#[test]
fn default_editor_projects_have_no_selection_driven_gameplay_and_clear_input_is_transient() {
	let data = scene(vec![entity("actor", [0.0; 3])]);
	let mut editor = EditorSession::new(data.clone()).unwrap();
	run(
		&mut editor,
		Command::Select {
			keys: vec!["actor".into()],
		},
	);
	run(&mut editor, Command::StartPlay);
	let revision = editor.revision();
	assert!(
		editor
			.execute_command(EditorCommandRequest {
				expected_revision: revision,
				command: Command::SetPlayInput {
					movement: [1.0, 0.0, 0.0]
				}
			})
			.is_err()
	);
	assert_eq!(editor.revision(), revision);
	assert!(!editor.needs_tick());
	editor.tick(10.0).unwrap();
	assert_eq!(editor.render_document().as_ref(), &data);
	run(&mut editor, Command::StopPlay);
	run(
		&mut editor,
		Command::SetProject {
			project: configured(&data),
		},
	);
	run(&mut editor, Command::StartPlay);
	run(
		&mut editor,
		Command::SetPlayInput {
			movement: [1.0, 0.0, 0.0],
		},
	);
	let revision = editor.revision();
	editor.clear_play_input();
	assert_eq!(editor.revision(), revision);
	assert!(!editor.needs_tick());
}

#[test]
fn scene_and_project_save_baselines_are_independent_and_noop_save_does_not_bump_revision() {
	let data = scene(vec![entity("actor", [0.0; 3])]);
	let mut editor = EditorSession::for_project(data.clone(), configured(&data)).unwrap();
	run(
		&mut editor,
		Command::Rename {
			key: "actor".into(),
			name: "Changed scene".into(),
		},
	);
	let mut project = editor.project().clone();
	project.name = "Changed project".into();
	run(&mut editor, Command::SetProject { project });
	assert!(editor.is_dirty());
	editor.mark_scene_saved().unwrap();
	assert!(
		editor.is_dirty(),
		"saving a scene cannot mark project settings clean"
	);
	let revision = editor.revision();
	editor.mark_scene_saved().unwrap();
	assert_eq!(editor.revision(), revision);
	editor.mark_project_saved().unwrap();
	assert!(!editor.is_dirty());
	run(
		&mut editor,
		Command::Rename {
			key: "actor".into(),
			name: "Another scene change".into(),
		},
	);
	editor.mark_project_saved().unwrap();
	assert!(
		editor.is_dirty(),
		"saving the project cannot mark scene edits clean"
	);
	run(&mut editor, Command::Undo);
	assert!(!editor.is_dirty());
	run(
		&mut editor,
		Command::BeginTransaction {
			label: "Preview".into(),
		},
	);
	let snapshot = serde_json::to_value(editor.snapshot().unwrap()).unwrap();
	assert!(editor.mark_scene_saved().is_err());
	assert!(editor.mark_project_saved().is_err());
	assert_eq!(
		serde_json::to_value(editor.snapshot().unwrap()).unwrap(),
		snapshot
	);
	run(&mut editor, Command::CancelTransaction);
}

#[test]
fn cancelling_grouped_project_edits_restores_configuration_and_preserves_redo() {
	let data = scene(vec![entity("actor", [0.0; 3])]);
	let mut editor = EditorSession::for_project(data.clone(), configured(&data)).unwrap();
	let original = editor.project().clone();
	run(
		&mut editor,
		Command::Rename {
			key: "actor".into(),
			name: "Redo scene".into(),
		},
	);
	run(&mut editor, Command::Undo);
	run(
		&mut editor,
		Command::BeginTransaction {
			label: "Cancelled project".into(),
		},
	);
	let mut project = original.clone();
	project.fixed_hz = 30;
	project.behaviors.clear();
	run(&mut editor, Command::SetProject { project });
	run(&mut editor, Command::CancelTransaction);
	assert_eq!(editor.project(), &original);
	assert!(!editor.is_dirty());
	assert!(editor.snapshot().unwrap().can_redo);
	run(&mut editor, Command::Redo);
	assert_eq!(editor.authored_document().entities[0].name, "Redo scene");
}

#[test]
fn replacing_project_and_scene_is_atomic_then_clears_play_selection_history_and_dirty_state() {
	let data = scene(vec![entity("actor", [0.0; 3])]);
	let mut editor = EditorSession::for_project(data.clone(), configured(&data)).unwrap();
	run(
		&mut editor,
		Command::Rename {
			key: "actor".into(),
			name: "Unsaved".into(),
		},
	);
	run(
		&mut editor,
		Command::Select {
			keys: vec!["actor".into()],
		},
	);
	run(&mut editor, Command::StartPlay);
	let snapshot = serde_json::to_value(editor.snapshot().unwrap()).unwrap();
	assert!(
		editor
			.replace_project_document(scene(vec![]), configured(&data))
			.is_err()
	);
	assert_eq!(
		serde_json::to_value(editor.snapshot().unwrap()).unwrap(),
		snapshot
	);
	let next = scene(vec![entity("next", [1.0, 2.0, 3.0])]);
	let next_project = ProjectData::for_scene("Next project", "scenes/next.json", &next).unwrap();
	let revision = editor.revision();
	editor
		.replace_project_document(next.clone(), next_project.clone())
		.unwrap();
	assert_eq!(editor.authored_document().as_ref(), &next);
	assert_eq!(editor.project(), &next_project);
	assert!(!editor.is_playing());
	assert!(editor.selection().is_empty());
	assert!(!editor.is_dirty());
	assert!(!editor.snapshot().unwrap().can_undo);
	assert!(editor.revision() > revision);
}
