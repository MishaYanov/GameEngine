mod support;

use engine_core::{
	SceneData,
	command::{
		EditorCommandDto as Command, EditorCommandRequest, TransformDto, TransformEditDto,
		TransformSpace,
	},
	editor::EditorSession,
};
use support::*;

fn run(session: &mut EditorSession, command: Command) {
	session
		.execute_command(EditorCommandRequest {
			expected_revision: session.revision(),
			command,
		})
		.unwrap();
}

fn edit(name: &str, x: f32) -> TransformEditDto {
	TransformEditDto {
		key: name.into(),
		transform: TransformDto {
			position: [x, 0.0, 0.0],
			..Default::default()
		},
	}
}

#[test]
fn reversed_transform_batch_changes_only_requested_entities_and_is_one_undo_step() {
	let data = scene(
		(0..512)
			.map(|index| entity(&format!("entity-{index}"), [index as f32, 0.0, 0.0]))
			.collect(),
	);
	let mut session = EditorSession::new(data.clone()).unwrap();
	run(
		&mut session,
		Command::SetTransforms {
			edits: (0..256)
				.rev()
				.map(|index| edit(&format!("entity-{}", index * 2), 1000.0 + index as f32))
				.collect(),
			space: TransformSpace::Local,
			snap: false,
		},
	);
	let changed = session.authored_document();
	for (index, entity) in changed.entities.iter().enumerate() {
		if index % 2 == 0 {
			close(entity.transform.position[0], 1000.0 + (index / 2) as f32);
			assert_eq!(entity.key, data.entities[index].key);
		} else {
			assert_eq!(entity, &data.entities[index]);
		}
	}
	run(&mut session, Command::Undo);
	assert_eq!(session.authored_document().as_ref(), &data);
	run(&mut session, Command::Redo);
	assert_eq!(session.authored_document(), changed);
}

#[test]
fn world_batch_resolves_selected_ancestors_independently_of_scene_and_edit_order() {
	let data = scene(vec![
		entity("grandchild", [1.0, 0.0, 0.0]).with_parent(Some(key("child"))),
		entity("child", [2.0, 0.0, 0.0]).with_parent(Some(key("parent"))),
		entity("unselected", [3.0, 0.0, 0.0]).with_parent(Some(key("parent"))),
		entity("parent", [1.0, 0.0, 0.0]),
	]);
	for edits in [
		vec![
			edit("grandchild", 20.0),
			edit("child", 15.0),
			edit("parent", 10.0),
		],
		vec![
			edit("parent", 10.0),
			edit("child", 15.0),
			edit("grandchild", 20.0),
		],
	] {
		let mut session = EditorSession::new(data.clone()).unwrap();
		run(
			&mut session,
			Command::SetTransforms {
				edits,
				space: TransformSpace::World,
				snap: false,
			},
		);
		let changed = session.authored_document();
		let world = changed.world_matrices().unwrap();
		for (index, expected) in [20.0, 15.0, 13.0, 10.0].into_iter().enumerate() {
			close(world[index].w_axis.x, expected);
		}
		assert_eq!(changed.entities[2], data.entities[2]);
		run(&mut session, Command::Undo);
		assert_eq!(session.authored_document().as_ref(), &data);
	}
}

fn unordered_tree() -> SceneData {
	let mut data = scene(vec![
		entity("grandchild", [1.0, 0.0, 0.0]).with_parent(Some(key("child"))),
		entity("child", [2.0, 0.0, 0.0]).with_parent(Some(key("parent"))),
		entity("external-child", [3.0, 0.0, 0.0]).with_parent(Some(key("outer"))),
		entity("parent", [10.0, 0.0, 0.0]),
		entity("outer", [100.0, 0.0, 0.0]),
		entity("sibling", [4.0, 0.0, 0.0]).with_parent(Some(key("parent"))),
	]);
	data.bindings.insert("main".into(), key("parent"));
	data
}

#[test]
fn overlapping_selection_duplicate_keeps_source_order_parent_links_bindings_and_selection_order() {
	let data = unordered_tree();
	let original_world = data.world_matrices().unwrap();
	let mut session = EditorSession::new(data.clone()).unwrap();
	run(
		&mut session,
		Command::Select {
			keys: vec!["child".into(), "parent".into(), "external-child".into()],
		},
	);
	run(
		&mut session,
		Command::DuplicateSelection {
			offset: [2.0, 0.0, 0.0],
		},
	);
	let duplicated = session.authored_document();
	assert_eq!(duplicated.entities.len(), 11);
	assert_eq!(&duplicated.entities[..6], data.entities.as_slice());
	assert_eq!(duplicated.bindings, data.bindings);
	let copies = &duplicated.entities[6..];
	let copied_world = duplicated.world_matrices().unwrap();
	for (copy, original_index) in [0, 1, 2, 3, 5].into_iter().enumerate() {
		assert_eq!(
			copies[copy].name,
			format!("{} Copy", data.entities[original_index].name)
		);
		assert_ne!(copies[copy].key, data.entities[original_index].key);
		close(
			copied_world[6 + copy].w_axis.x,
			original_world[original_index].w_axis.x + 2.0,
		);
	}
	assert_eq!(copies[0].parent.as_ref(), Some(&copies[1].key));
	assert_eq!(copies[1].parent.as_ref(), Some(&copies[3].key));
	assert_eq!(copies[2].parent, Some(key("outer")));
	assert_eq!(copies[3].parent, None);
	assert_eq!(copies[4].parent.as_ref(), Some(&copies[3].key));
	assert_eq!(
		session.selection(),
		&[
			copies[1].key.clone(),
			copies[3].key.clone(),
			copies[2].key.clone()
		]
	);
	run(&mut session, Command::DeleteSelection);
	assert_eq!(session.authored_document().as_ref(), &data);
	assert!(session.selection().is_empty());
	run(&mut session, Command::Undo);
	assert_eq!(session.authored_document(), duplicated);
}

#[test]
fn parent_and_child_selection_transform_only_roots_in_all_transform_operations() {
	let data = unordered_tree();
	let mut session = EditorSession::new(data.clone()).unwrap();
	run(
		&mut session,
		Command::Select {
			keys: vec!["child".into(), "parent".into()],
		},
	);
	for command in [
		Command::TranslateSelection {
			delta: [2.0, 0.0, 0.0],
			snap: false,
		},
		Command::RotateSelection {
			radians: [0.0, 0.0, 0.5],
			space: TransformSpace::World,
			snap: false,
		},
		Command::ScaleSelection {
			factor: [2.0; 3],
			snap: false,
		},
	] {
		run(&mut session, command);
		let changed = session.authored_document();
		for index in [0, 1, 2, 4, 5] {
			assert_eq!(changed.entities[index], data.entities[index]);
		}
		assert_ne!(changed.entities[3].transform, data.entities[3].transform);
		run(&mut session, Command::Undo);
		assert_eq!(session.authored_document().as_ref(), &data);
	}
}

#[test]
fn rejected_batches_leave_document_selection_revision_and_history_unchanged() {
	let data = unordered_tree();
	let mut session = EditorSession::new(data.clone()).unwrap();
	run(
		&mut session,
		Command::Select {
			keys: vec!["child".into()],
		},
	);
	for edits in [
		vec![
			edit("parent", 20.0),
			edit("first-missing", 30.0),
			edit("second-missing", 40.0),
		],
		vec![edit("child", 20.0), edit("child", 30.0)],
	] {
		let before = serde_json::to_value(session.snapshot().unwrap()).unwrap();
		let missing = edits[1].key == "first-missing";
		let error = session
			.execute_command(EditorCommandRequest {
				expected_revision: session.revision(),
				command: Command::SetTransforms {
					edits,
					space: TransformSpace::Local,
					snap: false,
				},
			})
			.unwrap_err();
		if missing {
			assert_eq!(error, "Entity key \"first-missing\" does not exist");
		} else {
			assert_eq!(
				error,
				"A transform command may target each entity only once"
			);
		}
		assert_eq!(
			serde_json::to_value(session.snapshot().unwrap()).unwrap(),
			before
		);
	}
}
