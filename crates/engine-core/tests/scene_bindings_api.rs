mod support;

use engine_core::{
	BackendErrorCode, CommandReceipt, Engine, EngineBuilder,
	command::{EditorCommandDto as Command, EditorCommandRequest, SceneBindingTarget},
	editor::CommandFactory,
	project::ProjectData,
	scene_catalog::{SceneDescriptor, SceneId},
};
use support::{entity, scene};

fn fixture() -> (Engine, SceneId, SceneId) {
	let document = scene(vec![entity("first", [0.0; 3]), entity("second", [1.0; 3])]);
	let mut project = ProjectData::for_scene("Roles", "scenes/start.json", &document).unwrap();
	let start = project.startup_scene_id.clone();
	let next = SceneId::new("next").unwrap();
	project
		.scenes
		.insert(SceneDescriptor::new(next.clone(), "scenes/next.json").unwrap())
		.unwrap();
	let engine = EngineBuilder::new(document.clone(), project)
		.with_scenes(vec![(next.clone(), document)])
		.build()
		.unwrap();
	(engine, start, next)
}

fn binding(scene_id: &SceneId, key: Option<&str>) -> Command {
	Command::SetSceneBinding {
		scene_id: scene_id.clone(),
		name: "objective".into(),
		target: match key {
			Some(key) => SceneBindingTarget::Assign { key: key.into() },
			None => SceneBindingTarget::Clear {},
		},
	}
}

fn execute(
	engine: &mut Engine,
	command: Command,
) -> Result<CommandReceipt, engine_core::BackendError> {
	let expected_revision = engine.session().revision();
	engine.execute(EditorCommandRequest {
		expected_revision,
		command,
	})
}

fn role(engine: &Engine, expected: Option<&str>) {
	let snapshot = engine.snapshot().unwrap();
	let target = snapshot
		.document
		.bindings
		.get("objective")
		.map(|key| key.as_str());
	assert_eq!(target, expected);
}

#[test]
fn assign_reassign_and_clear_are_undoable_and_identical_edits_are_noops() {
	let (mut engine, start, _) = fixture();
	let before = engine.snapshot().unwrap();
	assert!(
		!execute(&mut engine, binding(&start, None))
			.unwrap()
			.state_changed
	);

	execute(&mut engine, binding(&start, Some("first"))).unwrap();
	role(&engine, Some("first"));
	let assigned = engine.snapshot().unwrap();
	assert!(assigned.dirty);
	assert!(!assigned.project_dirty);
	assert_eq!(before.selection, assigned.selection);

	let cursor = engine.change_cursor();
	let noop = execute(&mut engine, binding(&start, Some("first"))).unwrap();
	assert!(!noop.state_changed);
	assert_eq!(noop.revision, assigned.revision);
	assert_eq!(noop.change_cursor, cursor);

	execute(&mut engine, binding(&start, Some("second"))).unwrap();
	role(&engine, Some("second"));
	execute(&mut engine, binding(&start, None)).unwrap();
	role(&engine, None);

	execute(&mut engine, Command::Undo).unwrap();
	role(&engine, Some("second"));
	execute(&mut engine, Command::Undo).unwrap();
	role(&engine, Some("first"));
	execute(&mut engine, Command::Undo).unwrap();
	role(&engine, None);
	assert!(!engine.snapshot().unwrap().dirty);

	execute(&mut engine, Command::Redo).unwrap();
	role(&engine, Some("first"));
	execute(&mut engine, Command::Redo).unwrap();
	role(&engine, Some("second"));
	execute(&mut engine, Command::Redo).unwrap();
	role(&engine, None);
}

#[test]
fn wrong_scene_missing_entity_stale_revision_and_play_leave_binding_unchanged() {
	let (mut engine, start, next) = fixture();
	let initial_revision = engine.session().revision();
	execute(&mut engine, binding(&start, Some("first"))).unwrap();
	let revision = engine.session().revision();
	let cursor = engine.change_cursor();
	assert_eq!(
		engine
			.execute(EditorCommandRequest {
				expected_revision: initial_revision,
				command: binding(&start, None),
			})
			.unwrap_err()
			.code,
		BackendErrorCode::Conflict,
	);
	for command in [
		binding(&next, Some("second")),
		binding(&start, Some("missing")),
	] {
		assert_eq!(
			execute(&mut engine, command).unwrap_err().code,
			BackendErrorCode::CommandRejected
		);
	}
	assert_eq!(engine.session().revision(), revision);
	assert_eq!(engine.change_cursor(), cursor);
	role(&engine, Some("first"));
	assert!(
		engine
			.session()
			.scene_document(&next)
			.unwrap()
			.bindings
			.is_empty()
	);

	execute(&mut engine, Command::StartPlay).unwrap();
	let revision = engine.session().revision();
	assert_eq!(
		execute(&mut engine, binding(&start, None))
			.unwrap_err()
			.code,
		BackendErrorCode::CommandRejected,
	);
	assert_eq!(engine.session().revision(), revision);
	execute(&mut engine, Command::StopPlay).unwrap();
	role(&engine, Some("first"));
}

#[test]
fn an_entity_binding_follows_its_key_through_rename_duplicate_and_delete_undo() {
	let (mut engine, start, _) = fixture();
	execute(&mut engine, binding(&start, Some("first"))).unwrap();
	execute(
		&mut engine,
		Command::Rename {
			key: "first".into(),
			name: "Renamed".into(),
		},
	)
	.unwrap();
	role(&engine, Some("first"));
	execute(
		&mut engine,
		Command::Select {
			keys: vec!["first".into()],
		},
	)
	.unwrap();
	execute(
		&mut engine,
		Command::DuplicateSelection {
			offset: [1.0, 0.0, 0.0],
		},
	)
	.unwrap();
	role(&engine, Some("first"));
	assert_eq!(engine.snapshot().unwrap().document.entities.len(), 3);

	execute(
		&mut engine,
		Command::Select {
			keys: vec!["first".into()],
		},
	)
	.unwrap();
	execute(&mut engine, Command::DeleteSelection).unwrap();
	role(&engine, None);
	execute(&mut engine, Command::Undo).unwrap();
	role(&engine, Some("first"));
	assert!(
		engine
			.snapshot()
			.unwrap()
			.document
			.entities
			.iter()
			.any(|entity| entity.key.as_str() == "first" && entity.name == "Renamed")
	);
}

#[test]
fn undo_and_redo_restore_binding_on_the_original_scene() {
	let (mut engine, start, next) = fixture();
	execute(&mut engine, binding(&start, Some("first"))).unwrap();
	execute(
		&mut engine,
		Command::SwitchScene {
			scene_id: next.clone(),
		},
	)
	.unwrap();
	role(&engine, None);
	execute(&mut engine, Command::Undo).unwrap();
	assert_eq!(engine.session().active_scene_id(), &start);
	role(&engine, None);
	execute(&mut engine, Command::Redo).unwrap();
	assert_eq!(engine.session().active_scene_id(), &start);
	role(&engine, Some("first"));
	assert!(
		engine
			.session()
			.scene_document(&next)
			.unwrap()
			.bindings
			.is_empty()
	);
}

#[test]
fn binding_command_is_advertised_as_an_undoable_scene_edit() {
	let descriptor = CommandFactory::capabilities()
		.into_iter()
		.find(|command| command.name == "set_scene_binding")
		.expect("scene binding capability");
	assert_eq!(descriptor.category, "scene");
	assert!(descriptor.undoable);
	assert!(!descriptor.available_in_play);
}

#[test]
fn invalid_binding_names_and_keys_do_not_mutate_scene() {
	let (mut engine, start, _) = fixture();
	let before = engine.snapshot().unwrap();
	let invalid = [
		Command::SetSceneBinding {
			scene_id: start.clone(),
			name: " ".into(),
			target: SceneBindingTarget::Clear {},
		},
		Command::SetSceneBinding {
			scene_id: start.clone(),
			name: "a".repeat(1025),
			target: SceneBindingTarget::Assign {
				key: "first".into(),
			},
		},
		binding(&start, Some("invalid key")),
	];
	for command in invalid {
		assert_eq!(
			execute(&mut engine, command).unwrap_err().code,
			BackendErrorCode::InvalidCommand,
		);
	}
	let after = engine.snapshot().unwrap();
	assert_eq!(after.revision, before.revision);
	assert_eq!(after.document, before.document);
	assert!(!after.can_undo);
}
