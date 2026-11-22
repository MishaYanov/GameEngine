mod support;

use engine_core::{
	EntityKey,
	command::{EditorCommandDto as Command, EditorCommandRequest},
	editor::EditorSession,
	scene_data::{FolderId, FolderRecord},
};
use support::{close, entity, key, scene};

fn issue(session: &mut EditorSession, command: Command) -> Result<(), String> {
	session.execute_command(EditorCommandRequest {
		expected_revision: session.revision(),
		command,
	})
}

fn reparent(keys: &[&str], parent: Option<&str>, keep_world: bool) -> Command {
	Command::ReparentObjects {
		keys: keys.iter().map(|name| key(name)).collect(),
		parent: parent.map(key),
		keep_world,
	}
}

#[test]
fn batch_parenting_reduces_selected_descendants_and_keeps_world_poses_in_one_edit() {
	let mut assembly = entity("assembly", [10.0, 0.0, 0.0]);
	assembly.transform.rotation = [0.0, 0.5, 0.0];
	let mut document = scene(vec![
		entity("detail", [1.0, 0.0, 0.0]).with_parent(Some(key("floor1"))),
		entity("floor2", [3.0, 0.0, 0.0]),
		assembly,
		entity("floor1", [2.0, 0.0, 0.0]),
	]);
	let folder = FolderId::new("floor").unwrap();
	document.organization.folders.push(FolderRecord {
		id: folder.clone(),
		name: "Floor".into(),
		parent: None,
	});
	document
		.organization
		.root_membership
		.extend([(key("floor1"), folder.clone()), (key("floor2"), folder)]);
	let original_world = document.world_matrices().unwrap();
	let mut session = EditorSession::new(document.clone()).unwrap();
	issue(
		&mut session,
		reparent(&["detail", "floor1", "floor2"], Some("assembly"), true),
	)
	.unwrap();
	let nested = session.authored_document();
	assert_eq!(nested.entities[0].parent.as_ref(), Some(&key("floor1")));
	assert_eq!(nested.entities[1].parent.as_ref(), Some(&key("assembly")));
	assert_eq!(nested.entities[3].parent.as_ref(), Some(&key("assembly")));
	assert!(nested.organization.root_membership.is_empty());
	for (before, after) in original_world.iter().zip(nested.world_matrices().unwrap()) {
		assert!(before.abs_diff_eq(after, 1.0e-4));
	}
	issue(
		&mut session,
		Command::Select {
			keys: vec!["assembly".into()],
		},
	)
	.unwrap();
	issue(
		&mut session,
		Command::TranslateSelection {
			delta: [4.0, 0.0, 0.0],
			snap: false,
		},
	)
	.unwrap();
	let moved = session.authored_document().world_matrices().unwrap();
	for (before, after) in original_world.iter().zip(moved) {
		close(after.w_axis.x, before.w_axis.x + 4.0);
		close(after.w_axis.z, before.w_axis.z);
	}
	issue(&mut session, Command::Undo).unwrap();
	assert_eq!(session.authored_document(), nested);
	issue(&mut session, Command::Undo).unwrap();
	assert_eq!(session.authored_document().as_ref(), &document);
	assert!(!session.snapshot().unwrap().can_undo);
	issue(&mut session, Command::Redo).unwrap();
	assert_eq!(session.authored_document(), nested);
}

#[test]
fn detached_objects_inherit_their_original_root_folder_without_moving() {
	let mut document = scene(vec![
		entity("assembly", [5.0, 0.0, 0.0]),
		entity("floor", [1.0, 0.0, 0.0]).with_parent(Some(key("assembly"))),
		entity("tile1", [1.0, 0.0, 0.0]).with_parent(Some(key("floor"))),
		entity("tile2", [2.0, 0.0, 0.0]).with_parent(Some(key("floor"))),
	]);
	let folder = FolderId::new("floor").unwrap();
	document.organization.folders.push(FolderRecord {
		id: folder.clone(),
		name: "Floor".into(),
		parent: None,
	});
	document
		.organization
		.root_membership
		.insert(key("assembly"), folder.clone());
	let before = document.world_matrices().unwrap();
	let mut session = EditorSession::new(document.clone()).unwrap();
	issue(
		&mut session,
		reparent(&["tile2", "tile1", "assembly"], None, true),
	)
	.unwrap();
	assert_eq!(session.authored_document().as_ref(), &document);
	issue(&mut session, reparent(&["tile2", "tile1"], None, true)).unwrap();
	let detached = session.authored_document();
	assert_eq!(detached.entities[2].parent, None);
	assert_eq!(detached.entities[3].parent, None);
	assert_eq!(detached.organization.root_membership[&key("tile1")], folder);
	assert_eq!(detached.organization.root_membership[&key("tile2")], folder);
	assert_eq!(detached.world_matrices().unwrap(), before);
	issue(&mut session, Command::Undo).unwrap();
	assert_eq!(session.authored_document().as_ref(), &document);
}

#[test]
fn rejects_a_cycle_or_an_unrepresentable_world_pose_without_partial_edits() {
	let mut scaled = entity("scaled", [0.0; 3]);
	scaled.transform.scale = [2.0, 1.0, 1.0];
	let mut rotated = entity("rotated", [0.0; 3]);
	rotated.transform.rotation = [0.0, 0.7, 0.0];
	let document = scene(vec![
		entity("ready", [1.0, 0.0, 0.0]),
		entity("child", [0.0; 3]).with_parent(Some(key("ready"))),
		rotated,
		scaled,
	]);
	let mut session = EditorSession::new(document.clone()).unwrap();
	let invalid = [
		(
			reparent(&["rotated", "ready"], Some("child"), false),
			"descendant",
		),
		(
			reparent(&["ready", "rotated"], Some("scaled"), true),
			"shear",
		),
		(
			reparent(&["rotated", "missing"], Some("scaled"), false),
			"no longer exists",
		),
	];
	for (command, message) in invalid {
		assert!(issue(&mut session, command).unwrap_err().contains(message));
		assert_eq!(session.revision(), 0);
		assert_eq!(session.authored_document().as_ref(), &document);
		assert!(!session.snapshot().unwrap().can_undo);
	}
}

#[test]
fn keep_local_allows_ordinary_parenting_when_keep_world_would_require_shear() {
	let mut scaled = entity("scaled", [0.0; 3]);
	scaled.transform.scale = [2.0, 1.0, 1.0];
	let mut rotated = entity("rotated", [0.0; 3]);
	rotated.transform.rotation = [0.0, 0.7, 0.0];
	let document = scene(vec![scaled, rotated]);
	let mut session = EditorSession::new(document.clone()).unwrap();
	issue(&mut session, reparent(&["rotated"], Some("scaled"), false)).unwrap();
	let changed = session.authored_document();
	assert_eq!(
		changed.entities[1].transform,
		document.entities[1].transform
	);
	assert_eq!(
		changed.entities[1].parent,
		Some(EntityKey::new("scaled").unwrap())
	);
}
