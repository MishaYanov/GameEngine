use super::support::{close, entity, key, matrix_close, scene};
use super::{fixture, reject, run, select, transform};
use bridge::editor::{EditorCommandDto as Command, SnapSettingsDto, TransformSpace};
use runtime::editor::EditorSession;

#[test]
fn world_transform_batch_resolves_edited_parent_before_child_independent_of_storage_order() {
	let child = entity("child", [2.0, 0.0, 0.0]).with_parent(Some(key("parent")));
	let mut s = EditorSession::new(scene(vec![child, entity("parent", [5.0, 0.0, 0.0])])).unwrap();
	run(
		&mut s,
		Command::SetTransforms {
			edits: vec![
				transform("child", [12.0, 0.0, 0.0]),
				transform("parent", [10.0, 0.0, 0.0]),
			],
			space: TransformSpace::World,
			snap: false,
		},
	);
	assert_eq!(
		s.authored_document().entities[0].transform.position,
		[2.0, 0.0, 0.0]
	);
	close(
		s.authored_document().world_matrices().unwrap()[0].w_axis.x,
		12.0,
	);
	let before = s.authored_document();
	reject(
		&mut s,
		Command::SetTransforms {
			edits: vec![transform("child", [3.0; 3]), transform("child", [4.0; 3])],
			space: TransformSpace::World,
			snap: false,
		},
	);
	assert_eq!(s.authored_document(), before);
}

#[test]
fn selected_parent_and_child_translate_once_and_duplicate_remaps_descendants_without_rebinding_roles()
 {
	let mut data = scene(vec![
		entity("parent", [1.0, 0.0, 0.0]),
		entity("child", [2.0, 0.0, 0.0]).with_parent(Some(key("parent"))),
		entity("other", [0.0; 3]),
	]);
	data.bindings.insert("player".into(), key("child"));
	let mut s = EditorSession::new(data).unwrap();
	select(&mut s, &["parent", "child"]);
	run(
		&mut s,
		Command::TranslateSelection {
			delta: [3.0, 0.0, 0.0],
			snap: false,
		},
	);
	assert_eq!(
		s.authored_document().entities[1].transform.position,
		[2.0, 0.0, 0.0]
	);
	close(
		s.authored_document().world_matrices().unwrap()[1].w_axis.x,
		6.0,
	);
	let before = s.authored_document();
	run(
		&mut s,
		Command::DuplicateSelection {
			offset: [1.0, 0.0, 0.0],
		},
	);
	let duplicated = s.authored_document();
	assert_eq!(duplicated.entities.len(), 5);
	assert_eq!(
		duplicated.entities[4].parent,
		Some(duplicated.entities[3].key.clone())
	);
	assert_eq!(duplicated.bindings["player"], key("child"));
	close(duplicated.world_matrices().unwrap()[4].w_axis.x, 7.0);
	run(&mut s, Command::DeleteSelection);
	assert_eq!(s.authored_document(), before);
	run(&mut s, Command::Undo);
	assert_eq!(s.authored_document(), duplicated);
	run(&mut s, Command::Undo);
	assert_eq!(s.authored_document(), before);
}

#[test]
fn reparent_preserves_world_or_local_and_rejects_cycles_singular_parent_and_shear_atomically() {
	let mut s = EditorSession::new(scene(vec![
		entity("parent", [5.0, 0.0, 0.0]),
		entity("child", [2.0, 0.0, 0.0]),
	]))
	.unwrap();
	let world = s.authored_document().world_matrices().unwrap()[1];
	run(
		&mut s,
		Command::Reparent {
			key: "child".into(),
			parent: Some("parent".into()),
			keep_world: true,
		},
	);
	matrix_close(s.authored_document().world_matrices().unwrap()[1], world);
	assert_eq!(
		s.authored_document().entities[1].transform.position,
		[-3.0, 0.0, 0.0]
	);
	reject(
		&mut s,
		Command::Reparent {
			key: "parent".into(),
			parent: Some("child".into()),
			keep_world: false,
		},
	);
	run(&mut s, Command::Undo);
	run(
		&mut s,
		Command::Reparent {
			key: "child".into(),
			parent: Some("parent".into()),
			keep_world: false,
		},
	);
	close(
		s.authored_document().world_matrices().unwrap()[1].w_axis.x,
		7.0,
	);
	for scale in [[0.0, 1.0, 1.0], [2.0, 1.0, 1.0]] {
		let mut parent = entity("parent", [0.0; 3]);
		parent.transform.scale = scale;
		parent.transform.rotation[2] = 0.7;
		let mut s = EditorSession::new(scene(vec![parent, entity("child", [0.0; 3])])).unwrap();
		reject(
			&mut s,
			Command::Reparent {
				key: "child".into(),
				parent: Some("parent".into()),
				keep_world: true,
			},
		);
	}
}

#[test]
fn snapping_requires_both_settings_and_command_opt_in_and_remains_undoable() {
	let mut s = fixture();
	select(&mut s, &["a"]);
	run(
		&mut s,
		Command::SetSnap {
			settings: SnapSettingsDto {
				enabled: true,
				translation: 0.5,
				..Default::default()
			},
		},
	);
	run(
		&mut s,
		Command::TranslateSelection {
			delta: [0.74, -0.26, 0.0],
			snap: true,
		},
	);
	assert_eq!(
		s.authored_document().entities[0].transform.position,
		[0.5, -0.5, 0.0]
	);
	run(
		&mut s,
		Command::TranslateSelection {
			delta: [0.1, 0.0, 0.0],
			snap: false,
		},
	);
	close(s.authored_document().entities[0].transform.position[0], 0.6);
	reject(
		&mut s,
		Command::SetSnap {
			settings: SnapSettingsDto {
				translation: 0.0,
				..Default::default()
			},
		},
	);
	run(&mut s, Command::Undo);
	close(s.authored_document().entities[0].transform.position[0], 0.5);
}

#[test]
fn material_collider_and_light_changes_are_validated_and_undoable() {
	let mut s = fixture();
	let before = s.authored_document();
	run(
		&mut s,
		Command::SetMaterial {
			key: "a".into(),
			material: Some(runtime::scene_data::MaterialOverrideData {
				base_color_factor: [0.2, 0.3, 0.4, 1.0],
			}),
		},
	);
	run(
		&mut s,
		Command::SetCollider {
			key: "a".into(),
			collider: Some(runtime::scene_data::ColliderData::Sphere {
				interaction: Default::default(),
				radius: 0.5,
				offset: [0.0; 3],
			}),
		},
	);
	let light = runtime::scene_data::LightData::new(
		key("sun"),
		"Sun",
		runtime::scene_data::LightKind::Directional,
	);
	run(&mut s, Command::UpsertLight { light });
	let changed = s.authored_document();
	reject(
		&mut s,
		Command::SetMaterial {
			key: "a".into(),
			material: Some(runtime::scene_data::MaterialOverrideData {
				base_color_factor: [2.0, 0.0, 0.0, 1.0],
			}),
		},
	);
	reject(
		&mut s,
		Command::SetCollider {
			key: "a".into(),
			collider: Some(runtime::scene_data::ColliderData::Sphere {
				interaction: Default::default(),
				radius: -1.0,
				offset: [0.0; 3],
			}),
		},
	);
	for _ in 0..3 {
		run(&mut s, Command::Undo);
	}
	assert_eq!(s.authored_document(), before);
	for _ in 0..3 {
		run(&mut s, Command::Redo);
	}
	assert_eq!(s.authored_document(), changed);
}
