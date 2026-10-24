mod support;
use bridge::editor::{
	EditorCommandDto as Command, EditorCommandRequest, EditorSnapshotDto, SnapSettingsDto,
	TransformDto, TransformEditDto, TransformSpace,
};
use runtime::{
	Bounds3,
	editor::{CommandFactory, EditorSession, PrefabData},
};
use support::*;

fn run(session: &mut EditorSession, command: Command) -> EditorSnapshotDto {
	session
		.execute(EditorCommandRequest {
			expected_revision: session.revision(),
			command,
		})
		.unwrap()
}
fn reject(session: &mut EditorSession, command: Command) -> String {
	let before = serde_json::to_value(session.snapshot().unwrap()).unwrap();
	let error = session
		.execute(EditorCommandRequest {
			expected_revision: session.revision(),
			command,
		})
		.unwrap_err();
	assert_eq!(
		serde_json::to_value(session.snapshot().unwrap()).unwrap(),
		before,
		"failed command mutated session: {error}"
	);
	error
}
fn select(session: &mut EditorSession, keys: &[&str]) {
	run(
		session,
		Command::Select {
			keys: keys.iter().map(|s| (*s).into()).collect(),
		},
	);
}
fn fixture() -> EditorSession {
	let data = scene(vec![entity("a", [0.0; 3]), entity("b", [2.0, 0.0, 0.0])]);
	let mut project =
		engine_core::project::ProjectData::for_scene("Test project", "scenes/main.json", &data)
			.unwrap();
	project.behaviors = vec![engine_core::project::BehaviorSpec::ControlledMove {
		entity: key("a"),
		speed: 2.0,
		space: engine_core::project::BehaviorSpace::World,
	}];
	EditorSession::for_project(data, project).unwrap()
}
fn transform(key: &str, position: [f32; 3]) -> TransformEditDto {
	TransformEditDto {
		key: key.into(),
		transform: TransformDto {
			position,
			..Default::default()
		},
	}
}

#[test]
fn stale_revisions_and_invalid_commands_are_atomic_and_noops_do_not_bump() {
	let mut s = fixture();
	let before = serde_json::to_value(s.snapshot().unwrap()).unwrap();
	assert!(
		s.execute(EditorCommandRequest {
			expected_revision: 1,
			command: Command::DeleteSelection
		})
		.is_err()
	);
	assert_eq!(serde_json::to_value(s.snapshot().unwrap()).unwrap(), before);
	run(
		&mut s,
		Command::Rename {
			key: "a".into(),
			name: "a".into(),
		},
	);
	assert_eq!(s.revision(), 0);
	reject(
		&mut s,
		Command::Rename {
			key: "a".into(),
			name: " \n".into(),
		},
	);
	reject(
		&mut s,
		Command::Select {
			keys: vec!["a".into(), "missing".into()],
		},
	);
	select(&mut s, &["b", "a", "b"]);
	assert_eq!(s.selection(), &[key("b"), key("a")]);
	let revision = s.revision();
	select(&mut s, &["b", "a"]);
	assert_eq!(s.revision(), revision);
}

#[test]
fn selection_limit_is_enforced_before_mutation_and_capabilities_have_unique_names() {
	let entities = (0..4097)
		.map(|i| entity(&format!("entity-{i}"), [0.0; 3]))
		.collect();
	let mut s = EditorSession::new(scene(entities)).unwrap();
	let keys = (0..4096).map(|i| format!("entity-{i}")).collect::<Vec<_>>();
	run(&mut s, Command::Select { keys: keys.clone() });
	assert_eq!(s.selection().len(), 4096);
	let mut excessive = keys;
	excessive.push("entity-4096".into());
	reject(&mut s, Command::Select { keys: excessive });
	let descriptors = CommandFactory::capabilities();
	let names = descriptors
		.iter()
		.map(|d| d.name.as_str())
		.collect::<std::collections::HashSet<_>>();
	assert_eq!(names.len(), descriptors.len());
	assert!(names.contains("reparent"));
	assert!(names.contains("commit_transaction"));
}

#[test]
fn grouped_edits_commit_one_record_restore_selection_and_cancel_preserves_redo() {
	let mut s = fixture();
	select(&mut s, &["a"]);
	let original = s.authored_document();
	run(
		&mut s,
		Command::BeginTransaction {
			label: "Group".into(),
		},
	);
	run(
		&mut s,
		Command::Rename {
			key: "a".into(),
			name: "Changed".into(),
		},
	);
	select(&mut s, &["b"]);
	run(
		&mut s,
		Command::TranslateSelection {
			delta: [0.0, 3.0, 0.0],
			snap: false,
		},
	);
	reject(&mut s, Command::Undo);
	reject(&mut s, Command::StartPlay);
	assert!(s.mark_saved().is_err());
	run(&mut s, Command::CommitTransaction);
	let changed = s.authored_document();
	run(&mut s, Command::Undo);
	assert_eq!(s.authored_document(), original);
	assert_eq!(s.selection(), &[key("a")]);
	assert!(!s.snapshot().unwrap().can_undo);
	run(
		&mut s,
		Command::BeginTransaction {
			label: "Cancel".into(),
		},
	);
	run(
		&mut s,
		Command::Rename {
			key: "b".into(),
			name: "Discard".into(),
		},
	);
	select(&mut s, &["b"]);
	run(&mut s, Command::CancelTransaction);
	assert_eq!(s.authored_document(), original);
	assert_eq!(s.selection(), &[key("a")]);
	run(&mut s, Command::Redo);
	assert_eq!(s.authored_document(), changed);
	assert_eq!(s.selection(), &[key("b")]);
}

#[test]
fn noop_and_rejected_transactions_leave_redo_available() {
	let mut s = fixture();
	run(
		&mut s,
		Command::Rename {
			key: "a".into(),
			name: "Changed".into(),
		},
	);
	run(&mut s, Command::Undo);
	reject(&mut s, Command::BeginTransaction { label: "\n".into() });
	run(
		&mut s,
		Command::BeginTransaction {
			label: "No-op".into(),
		},
	);
	reject(
		&mut s,
		Command::BeginTransaction {
			label: "Nested".into(),
		},
	);
	reject(
		&mut s,
		Command::SetTransforms {
			edits: vec![transform("a", [1.0; 3]), transform("missing", [2.0; 3])],
			space: TransformSpace::Local,
			snap: false,
		},
	);
	run(&mut s, Command::CommitTransaction);
	assert!(s.snapshot().unwrap().can_redo);
	run(&mut s, Command::Redo);
	assert_eq!(s.authored_document().entities[0].name, "Changed");
}

#[test]
fn history_evicts_oldest_record_at_128_and_a_new_edit_discards_redo() {
	let mut s = fixture();
	for n in 1..=130 {
		run(
			&mut s,
			Command::Rename {
				key: "a".into(),
				name: format!("Name {n}"),
			},
		);
	}
	let mut undone = 0;
	while s.snapshot().unwrap().can_undo {
		run(&mut s, Command::Undo);
		undone += 1;
	}
	assert_eq!(undone, 128);
	assert_eq!(s.authored_document().entities[0].name, "Name 2");
	run(
		&mut s,
		Command::Rename {
			key: "b".into(),
			name: "Branch".into(),
		},
	);
	assert!(!s.snapshot().unwrap().can_redo);
}

#[test]
fn oversized_patch_fails_atomically_and_preserves_redo_and_open_transaction() {
	let entities = (0..2000)
		.map(|i| {
			let mut e = entity(&format!("entity-{i}"), [0.0; 3]);
			e.name = "n".repeat(1024);
			// Keep each command below the target limit while making the owned
			// before/after history patch exceed its separate 16 MiB budget.
			e.model_path = format!("models/{}.glb", "p".repeat(3980));
			e
		})
		.collect();
	let mut s = EditorSession::new(scene(entities)).unwrap();
	run(
		&mut s,
		Command::Rename {
			key: "entity-0".into(),
			name: "small".into(),
		},
	);
	run(&mut s, Command::Undo);
	let edits = (0..2000)
		.map(|i| transform(&format!("entity-{i}"), [1.0; 3]))
		.collect::<Vec<_>>();
	reject(
		&mut s,
		Command::SetTransforms {
			edits: edits.clone(),
			space: TransformSpace::Local,
			snap: false,
		},
	);
	assert!(s.snapshot().unwrap().can_redo);
	run(
		&mut s,
		Command::BeginTransaction {
			label: "Too large".into(),
		},
	);
	run(
		&mut s,
		Command::SetTransforms {
			edits,
			space: TransformSpace::Local,
			snap: false,
		},
	);
	reject(&mut s, Command::CommitTransaction);
	assert!(s.has_transaction());
	run(&mut s, Command::CancelTransaction);
	run(&mut s, Command::Redo);
	assert_eq!(s.authored_document().entities[0].name, "small");
}

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

#[test]
fn camera_frame_orbit_pan_dolly_use_bounds_and_do_not_rebuild_scene_geometry() {
	let mut s = EditorSession::new(scene(vec![entity("a", [4.0, 2.0, 0.0])])).unwrap();
	select(&mut s, &["a"]);
	reject(&mut s, Command::FrameSelection);
	s.set_model_bounds(
		"props/robot.glb".into(),
		0,
		Bounds3::new([-1.0; 3], [1.0; 3]).unwrap(),
	);
	let original = s.authored_document();
	let geometry_revision = s.scene_revision();
	run(&mut s, Command::FrameSelection);
	let framed = s.authored_document();
	close(framed.camera.position[0], 4.0);
	close(framed.camera.position[1], 2.0);
	assert!(framed.camera.position[2] > 1.0);
	run(
		&mut s,
		Command::OrbitCamera {
			yaw: 0.5,
			pitch: 0.2,
		},
	);
	let camera = &s.authored_document().camera;
	let distance = glam::Vec3::from_array(camera.position).distance(glam::Vec3::new(4.0, 2.0, 0.0));
	let previous =
		glam::Vec3::from_array(framed.camera.position).distance(glam::Vec3::new(4.0, 2.0, 0.0));
	close(distance, previous);
	run(
		&mut s,
		Command::PanCamera {
			right: 1.0,
			up: 0.5,
		},
	);
	run(&mut s, Command::DollyCamera { distance: 0.2 });
	assert_eq!(s.scene_revision(), geometry_revision);
	reject(
		&mut s,
		Command::OrbitCamera {
			yaw: f32::NAN,
			pitch: 0.0,
		},
	);
	for _ in 0..4 {
		run(&mut s, Command::Undo);
	}
	assert_eq!(s.authored_document(), original);
}

#[test]
fn play_ticks_copy_authored_state_without_invalidating_command_revision_and_pause_discards_backlog()
{
	let mut s = fixture();
	select(&mut s, &["a"]);
	let authored = s.authored_document();
	run(&mut s, Command::StartPlay);
	assert!(std::sync::Arc::ptr_eq(&s.render_document(), &authored));
	run(
		&mut s,
		Command::SetPlayInput {
			movement: [3.0, 4.0, 0.0],
		},
	);
	let revision = s.revision();
	let scene_revision = s.scene_revision();
	for _ in 0..6 {
		s.tick(1.0 / 60.0).unwrap();
	}
	assert_eq!(s.revision(), revision);
	assert!(s.scene_revision() > scene_revision);
	close(s.render_document().entities[0].transform.position[0], 0.12);
	close(s.render_document().entities[0].transform.position[1], 0.16);
	assert_eq!(s.authored_document(), authored);
	assert!(!s.snapshot().unwrap().dirty);
	reject(
		&mut s,
		Command::Rename {
			key: "a".into(),
			name: "Forbidden".into(),
		},
	);
	run(&mut s, Command::SetPaused { paused: true });
	let paused = s.render_document();
	s.tick(60.0).unwrap();
	assert_eq!(s.render_document(), paused);
	run(&mut s, Command::SetPaused { paused: false });
	s.tick(0.1).unwrap();
	assert_eq!(s.render_document(), paused);
	run(
		&mut s,
		Command::SetPlayInput {
			movement: [1.0, 0.0, 0.0],
		},
	);
	s.tick(1.0 / 120.0).unwrap();
	assert_eq!(s.render_document(), paused);
	run(&mut s, Command::StopPlay);
	assert_eq!(s.render_document(), authored);
	assert!(s.tick(f64::NAN).is_err());
	assert_eq!(s.authored_document(), authored);
}

#[test]
fn saved_baseline_and_document_replacement_reset_history_and_supersede_old_requests() {
	let mut s = fixture();
	run(
		&mut s,
		Command::Rename {
			key: "a".into(),
			name: "Saved".into(),
		},
	);
	assert!(s.snapshot().unwrap().dirty);
	s.mark_saved().unwrap();
	assert!(!s.snapshot().unwrap().dirty);
	run(&mut s, Command::Undo);
	assert!(s.snapshot().unwrap().dirty);
	let old = s.revision();
	let mut next = EditorSession::new(scene(vec![])).unwrap();
	next.supersede_revision(old).unwrap();
	assert!(next.revision() > old);
	assert!(
		next.execute(EditorCommandRequest {
			expected_revision: old,
			command: Command::StartPlay
		})
		.is_err()
	);
	run(&mut s, Command::StartPlay);
	s.replace_document(scene(vec![])).unwrap();
	let snapshot = s.snapshot().unwrap();
	assert_eq!(snapshot.mode, "edit");
	assert!(!snapshot.dirty);
	assert!(!snapshot.can_undo);
	assert!(snapshot.selection.is_empty());
	assert!(next.supersede_revision(u64::MAX).is_err());
}

#[test]
fn prefab_export_includes_subtrees_remaps_keys_and_undo_redo_restores_the_same_instance() {
	let child = entity("child", [2.0, 0.0, 0.0]).with_parent(Some(key("parent")));
	let mut s = EditorSession::new(scene(vec![entity("parent", [5.0, 0.0, 0.0]), child])).unwrap();
	select(&mut s, &["parent"]);
	let original = s.authored_document();
	let prefab = s.export_prefab().unwrap();
	assert_eq!(prefab.entities.len(), 2);
	assert_eq!(prefab.entities[0].transform.position, [0.0; 3]);
	let serialized = prefab.to_json().unwrap();
	let prefab = PrefabData::from_json(&serialized).unwrap();
	s.instantiate_prefab(prefab, [10.0, 0.0, 0.0]).unwrap();
	let instantiated = s.authored_document();
	assert_eq!(instantiated.entities.len(), 4);
	assert_ne!(instantiated.entities[2].key, key("parent"));
	assert_eq!(
		instantiated.entities[3].parent,
		Some(instantiated.entities[2].key.clone())
	);
	close(instantiated.world_matrices().unwrap()[3].w_axis.x, 12.0);
	assert_eq!(s.selection(), &[instantiated.entities[2].key.clone()]);
	run(&mut s, Command::Undo);
	assert_eq!(s.authored_document(), original);
	run(&mut s, Command::Redo);
	assert_eq!(s.authored_document(), instantiated);
}

#[test]
fn prefab_detaches_external_parent_and_invalid_prefabs_do_not_change_document_or_redo() {
	let child = entity("child", [2.0, 0.0, 0.0]).with_parent(Some(key("parent")));
	let mut s = EditorSession::new(scene(vec![entity("parent", [5.0, 0.0, 0.0]), child])).unwrap();
	select(&mut s, &["child"]);
	let exported = s.export_prefab().unwrap();
	assert_eq!(exported.entities.len(), 1);
	assert!(exported.entities[0].parent.is_none());
	run(
		&mut s,
		Command::Rename {
			key: "child".into(),
			name: "Redo".into(),
		},
	);
	run(&mut s, Command::Undo);
	let before = s.authored_document();
	for prefab in [
		PrefabData {
			schema_version: 1,
			entities: vec![],
		},
		PrefabData {
			schema_version: 2,
			entities: exported.entities.clone(),
		},
		PrefabData {
			schema_version: 1,
			entities: vec![entity("child", [0.0; 3]).with_parent(Some(key("outside")))],
		},
	] {
		assert!(s.instantiate_prefab(prefab, [0.0; 3]).is_err());
		assert_eq!(s.authored_document(), before);
		assert!(s.snapshot().unwrap().can_redo);
	}
	assert!(s.instantiate_prefab(exported, [f32::NAN; 3]).is_err());
	run(&mut s, Command::Redo);
	assert_eq!(s.authored_document().entities[1].name, "Redo");
}

#[test]
fn picking_skips_transform_only_groups_and_framing_uses_descendant_geometry() {
	let group = entity("group", [0.0; 3]).with_renderable(false);
	let child = entity("child", [0.0; 3])
		.with_parent(Some(key("group")))
		.with_mesh_index(Some(1));
	let mut s = EditorSession::new(scene(vec![group, child])).unwrap();
	s.set_model_bounds(
		"props/robot.glb".into(),
		0,
		Bounds3::new([-100.0; 3], [100.0; 3]).unwrap(),
	);
	s.set_model_bounds(
		"props/robot.glb".into(),
		1,
		Bounds3::new([-0.5; 3], [0.5; 3]).unwrap(),
	);
	run(
		&mut s,
		Command::PickSelection {
			pixel: [400.0, 300.0],
			extent: [800, 600],
			additive: false,
		},
	);
	assert_eq!(s.selection(), &[key("child")]);
	select(&mut s, &["group"]);
	run(&mut s, Command::FrameSelection);
	let framed_group = s.authored_document().camera.clone();
	run(&mut s, Command::Undo);
	select(&mut s, &["child"]);
	run(&mut s, Command::FrameSelection);
	assert_eq!(s.authored_document().camera, framed_group);
	assert!(
		framed_group.position[2] < 10.0,
		"framed phantom mesh-0 geometry"
	);
}

#[test]
fn play_tick_demand_requires_target_and_movement_and_motion_error_pauses_without_partial_edit() {
	let mut parent = entity("parent", [0.0; 3]);
	parent.transform.scale = [0.0, 1.0, 1.0];
	let child = entity("child", [0.0; 3]).with_parent(Some(key("parent")));
	let data = scene(vec![parent, child]);
	let mut project =
		engine_core::project::ProjectData::for_scene("Test project", "scenes/main.json", &data)
			.unwrap();
	project.behaviors = vec![engine_core::project::BehaviorSpec::ControlledMove {
		entity: key("child"),
		speed: 2.0,
		space: engine_core::project::BehaviorSpace::World,
	}];
	let mut s = EditorSession::for_project(data, project).unwrap();
	assert!(!s.needs_tick());
	run(&mut s, Command::StartPlay);
	assert!(!s.needs_tick());
	run(
		&mut s,
		Command::SetPlayInput {
			movement: [1.0, 0.0, 0.0],
		},
	);
	assert!(s.needs_tick());
	select(&mut s, &["parent"]); // UI selection does not control the configured actor.
	assert!(s.needs_tick());
	let before = s.render_document();
	let revision = s.revision();
	assert!(s.tick(0.1).unwrap_err().contains("paused"));
	assert!(s.is_paused());
	assert!(!s.needs_tick());
	assert_eq!(s.render_document(), before);
	assert_eq!(s.revision(), revision);
	s.tick(1.0).unwrap();
	assert_eq!(s.render_document(), before);
}
