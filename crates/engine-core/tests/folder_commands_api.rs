mod support;

use engine_core::{
	command::{EditorCommandDto as Command, EditorCommandRequest},
	editor::{CommandFactory, EditorSession},
	scene_catalog::SceneId,
	scene_data::FolderId,
};
use support::{entity, key, scene};

fn execute(session: &mut EditorSession, command: Command) -> Result<(), String> {
	session.execute_command(EditorCommandRequest {
		expected_revision: session.revision(),
		command,
	})
}

fn run(session: &mut EditorSession, command: Command) {
	execute(session, command).unwrap();
}

fn create(session: &mut EditorSession, name: &str, parent: Option<FolderId>) -> FolderId {
	run(
		session,
		Command::CreateFolder {
			name: name.into(),
			parent,
		},
	);
	session
		.authored_document()
		.organization
		.folders
		.last()
		.unwrap()
		.id
		.clone()
}

#[test]
fn folder_edits_keep_pose_selection_and_sibling_order_and_are_undoable() {
	let mut session = EditorSession::new(scene(vec![
		entity("floor1", [1.0, 0.0, 0.0]),
		entity("floor2", [2.0, 0.0, 0.0]),
	]))
	.unwrap();
	run(
		&mut session,
		Command::Select {
			keys: vec!["floor2".into()],
		},
	);
	let initial = session.authored_document();
	let level = create(&mut session, "Level", None);
	let floors = create(&mut session, "Floor", Some(level.clone()));
	let details = create(&mut session, "Details", Some(floors.clone()));
	run(
		&mut session,
		Command::MoveRootsToFolder {
			roots: vec![key("floor1"), key("floor2")],
			folder: Some(floors.clone()),
		},
	);
	let moved = session.authored_document();
	assert_eq!(moved.organization.root_membership.len(), 2);
	assert_eq!(
		moved.world_matrices().unwrap(),
		initial.world_matrices().unwrap()
	);
	assert_eq!(session.selection(), &[key("floor2")]);
	assert_eq!(session.scene_revision(), 0);
	assert!(session.is_dirty());

	run(&mut session, Command::Undo);
	assert!(
		session
			.authored_document()
			.organization
			.root_membership
			.is_empty()
	);
	run(&mut session, Command::Redo);
	assert_eq!(session.authored_document(), moved);
	run(&mut session, Command::RemoveFolder { id: floors });
	let lifted = session.authored_document();
	assert_eq!(lifted.organization.folders[0].id, level);
	assert_eq!(lifted.organization.folders[1].id, details);
	assert_eq!(lifted.organization.folders[1].parent.as_ref(), Some(&level));
	assert_eq!(lifted.organization.root_membership[&key("floor1")], level);
	assert_eq!(lifted.organization.root_membership[&key("floor2")], level);
	assert_eq!(lifted.entities, initial.entities);
	run(&mut session, Command::Undo);
	assert_eq!(session.authored_document(), moved);
}

#[test]
fn rejects_stale_cross_scene_and_invalid_edits_without_a_partial_change() {
	let root = entity("root", [0.0; 3]);
	let child = entity("child", [0.0; 3]).with_parent(Some(key("root")));
	let mut session = EditorSession::new(scene(vec![root, child])).unwrap();
	let main = session.active_scene_id().clone();
	let a = create(&mut session, "A", None);
	let b = create(&mut session, "B", Some(a.clone()));
	let revision = session.revision();
	let before = session.authored_document();
	for rejected in [
		Command::MoveFolder {
			id: a.clone(),
			parent: Some(b),
		},
		Command::MoveRootsToFolder {
			roots: vec![key("root"), key("child")],
			folder: Some(a.clone()),
		},
		Command::MoveRootsToFolder {
			roots: vec![key("root"), key("root")],
			folder: None,
		},
		Command::RenameFolder {
			id: a.clone(),
			name: "\t".into(),
		},
	] {
		assert!(execute(&mut session, rejected).is_err());
		assert_eq!(session.revision(), revision);
		assert_eq!(session.authored_document(), before);
	}
	let other = SceneId::new("other").unwrap();
	run(
		&mut session,
		Command::AddScene {
			id: other.clone(),
			asset_path: "scenes/other.json".into(),
			document: scene(vec![entity("root", [0.0; 3])]),
		},
	);
	if session.active_scene_id() != &other {
		run(&mut session, Command::SwitchScene { scene_id: other });
	}
	let request = Command::RemoveFolder { id: a };
	assert!(
		session
			.execute_command(EditorCommandRequest {
				expected_revision: revision,
				command: request.clone(),
			})
			.unwrap_err()
			.contains("Revision conflict")
	);
	assert!(
		execute(&mut session, request)
			.unwrap_err()
			.contains("does not exist")
	);
	assert!(session.authored_document().organization.folders.is_empty());
	run(&mut session, Command::SwitchScene { scene_id: main });
	assert_eq!(session.authored_document(), before);
}

#[test]
fn folder_membership_tracks_duplication_deletion_and_parenting_through_history() {
	let mut session = EditorSession::new(scene(vec![
		entity("floor", [1.0, 0.0, 0.0]),
		entity("parent", [0.0, 0.0, 0.0]),
	]))
	.unwrap();
	let folder = create(&mut session, "Floor", None);
	run(
		&mut session,
		Command::MoveRootsToFolder {
			roots: vec![key("floor")],
			folder: Some(folder.clone()),
		},
	);
	run(
		&mut session,
		Command::Select {
			keys: vec!["floor".into()],
		},
	);
	run(
		&mut session,
		Command::DuplicateSelection { offset: [0.0; 3] },
	);
	let copy = session.selection()[0].clone();
	let duplicated = session.authored_document();
	assert_eq!(duplicated.organization.root_membership[&copy], folder);
	run(&mut session, Command::DeleteSelection);
	assert!(
		!session
			.authored_document()
			.organization
			.root_membership
			.contains_key(&copy)
	);
	run(&mut session, Command::Undo);
	assert_eq!(session.authored_document(), duplicated);
	run(
		&mut session,
		Command::Reparent {
			key: copy.as_str().into(),
			parent: Some("parent".into()),
			keep_world: true,
		},
	);
	assert!(
		!session
			.authored_document()
			.organization
			.root_membership
			.contains_key(&copy)
	);
	run(&mut session, Command::Undo);
	assert_eq!(session.authored_document(), duplicated);
}

#[test]
fn folder_commands_are_serializable_and_cancelled_group_edits_clear_metadata() {
	let mut session = EditorSession::new(scene(vec![entity("floor", [0.0; 3])])).unwrap();
	let before = session.authored_document();
	run(
		&mut session,
		Command::BeginTransaction {
			label: "Organize level".into(),
		},
	);
	let command: Command = serde_json::from_value(serde_json::json!({
		"type": "create_folder",
		"name": "Floor",
		"parent": null
	}))
	.unwrap();
	run(&mut session, command);
	run(&mut session, Command::CancelTransaction);
	assert_eq!(session.authored_document(), before);
	assert!(!session.is_dirty());
	assert!(
		CommandFactory::capabilities()
			.iter()
			.any(|command| command.name == "move_roots_to_folder"
				&& command.undoable
				&& !command.available_in_play)
	);
}
