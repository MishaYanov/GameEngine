mod support;

use engine_core::{
	BackendErrorCode, Engine, EngineBuilder,
	command::{EditorCommandDto as Command, EditorCommandRequest},
	project::ProjectData,
	scene_catalog::{SceneDescriptor, SceneId},
};
use support::{entity, scene};

fn editor() -> (Engine, SceneId, SceneId) {
	let startup = scene(vec![entity("startup-actor", [0.0; 3])]);
	let preview = scene(vec![entity("preview-actor", [0.0; 3])]);
	let mut project = ProjectData::for_scene("Game", "scenes/startup.json", &startup).unwrap();
	let startup_id = project.startup_scene_id.clone();
	let preview_id = SceneId::new("preview").unwrap();
	project
		.scenes
		.insert(SceneDescriptor::new(preview_id.clone(), "scenes/preview.json").unwrap())
		.unwrap();
	let engine = EngineBuilder::new(startup, project)
		.with_scenes(vec![(preview_id.clone(), preview)])
		.build()
		.unwrap();
	(engine, startup_id, preview_id)
}

fn execute(engine: &mut Engine, command: Command) {
	engine
		.execute(EditorCommandRequest {
			expected_revision: engine.session().revision(),
			command,
		})
		.unwrap();
}

#[test]
fn restart_uses_retained_startup_and_preserves_editor_state() {
	let (mut engine, startup, preview) = editor();
	assert!(engine.play_status().is_none());
	assert!(serde_json::to_value(engine.snapshot().unwrap()).unwrap()["play_session_id"].is_null());
	execute(
		&mut engine,
		Command::Rename {
			key: "startup-actor".into(),
			name: "Unsaved startup change".into(),
		},
	);
	execute(
		&mut engine,
		Command::SwitchScene {
			scene_id: preview.clone(),
		},
	);
	execute(
		&mut engine,
		Command::Select {
			keys: vec!["preview-actor".into()],
		},
	);
	execute(&mut engine, Command::StartPlay);
	execute(&mut engine, Command::SetPaused { paused: true });

	let status = engine.play_status().unwrap();
	let authored = engine.snapshot().unwrap();
	assert_eq!(status.scene_id, preview);
	assert!(status.paused);
	assert!(status.gameplay.is_none());
	assert_eq!(
		authored.play_session_id.as_deref(),
		Some(status.play_session_id.as_str())
	);
	assert_eq!(
		engine.session().state().play_session_id,
		authored.play_session_id
	);
	assert_eq!(
		engine
			.validate_restart_play(&status.run_id, authored.revision - 1)
			.unwrap_err()
			.code,
		BackendErrorCode::Conflict
	);

	let (prepared, document) = engine
		.prepare_restart_play(&status.run_id, authored.revision)
		.unwrap();
	assert_eq!(document.entities[0].name, "Unsaved startup change");
	assert_eq!(engine.play_status(), Some(status.clone()));
	engine.commit_play_scene(prepared).unwrap();

	let new_status = engine.play_status().unwrap();
	assert_eq!(new_status.scene_id, startup);
	assert_eq!(new_status.play_session_id, status.play_session_id);
	assert_ne!(new_status.run_id, status.run_id);
	assert!(!new_status.paused);
	assert_eq!(engine.session().render_document(), document);
	assert_eq!(
		engine
			.validate_restart_play(&status.run_id, authored.revision)
			.unwrap_err()
			.code,
		BackendErrorCode::Conflict
	);
	let after = engine.snapshot().unwrap();
	assert_eq!(after.revision, authored.revision);
	assert_eq!(after.active_scene_id, authored.active_scene_id);
	assert_eq!(after.document, authored.document);
	assert_eq!(after.project, authored.project);
	assert_eq!(after.selection, authored.selection);
	assert!(after.dirty && after.can_undo);
}

#[test]
fn play_and_run_ids_distinguish_scene_changes_restarts_and_new_play() {
	let (mut engine, startup, preview) = editor();
	execute(&mut engine, Command::StartPlay);
	let original = engine.play_status().unwrap();

	let (stale, _) = engine
		.prepare_restart_play(&original.run_id, engine.session().revision())
		.unwrap();
	let document = engine.session().scene_document(&preview).unwrap();
	let transition = engine
		.session()
		.prepare_play_scene(preview.clone(), document)
		.unwrap();
	engine.commit_play_scene(transition).unwrap();
	let other_scene = engine.play_status().unwrap();
	assert_eq!(other_scene.scene_id, preview);
	assert_eq!(other_scene.play_session_id, original.play_session_id);
	assert_eq!(other_scene.run_id, original.run_id);
	assert!(engine.commit_play_scene(stale).is_err());
	assert_eq!(engine.play_status(), Some(other_scene));

	let (restart, _) = engine
		.prepare_restart_play(&original.run_id, engine.session().revision())
		.unwrap();
	engine.commit_play_scene(restart).unwrap();

	let first_restart = engine.play_status().unwrap();
	assert_eq!(first_restart.scene_id, startup);
	let (restart, _) = engine
		.prepare_restart_play(&first_restart.run_id, engine.session().revision())
		.unwrap();
	engine.commit_play_scene(restart).unwrap();
	let same_scene_restart = engine.play_status().unwrap();
	assert_eq!(same_scene_restart.scene_id, startup);
	assert_eq!(same_scene_restart.play_session_id, original.play_session_id);
	assert_ne!(same_scene_restart.run_id, first_restart.run_id);

	let (stale, _) = engine
		.prepare_restart_play(&same_scene_restart.run_id, engine.session().revision())
		.unwrap();
	execute(&mut engine, Command::StopPlay);
	assert!(engine.play_status().is_none());
	assert!(engine.snapshot().unwrap().play_session_id.is_none());
	assert_eq!(
		engine
			.validate_restart_play(&same_scene_restart.run_id, engine.session().revision())
			.unwrap_err()
			.code,
		BackendErrorCode::CommandRejected
	);
	execute(&mut engine, Command::StartPlay);
	let new_play = engine.play_status().unwrap();
	assert_ne!(new_play.play_session_id, original.play_session_id);
	assert_ne!(new_play.run_id, same_scene_restart.run_id);
	assert!(engine.commit_play_scene(stale).is_err());
	assert_eq!(engine.play_status(), Some(new_play));
}
