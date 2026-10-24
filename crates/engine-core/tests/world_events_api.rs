mod support;
use engine_core::{
	events::{BackendEventKind, ChangeJournal},
	scene_data::{LightData, LightKind, TransformData},
	world::{MAX_CHANGE_KEYS, RuntimeWorld, WorldChange},
};
use std::sync::Arc;
use support::*;

#[test]
fn world_updates_only_the_affected_subtree_and_keeps_the_authored_arc_unchanged() {
	let data = Arc::new(scene(vec![
		entity("child", [1.0, 0.0, 0.0]).with_parent(Some(key("parent"))),
		entity("unrelated", [5.0, 0.0, 0.0]),
		entity("parent", [2.0, 0.0, 0.0]),
	]));
	let mut world = RuntimeWorld::new(Arc::clone(&data)).unwrap();
	assert!(Arc::ptr_eq(&data, &world.document()));
	let initial_updates = world.matrix_updates();
	let unrelated = world.world_matrices()[1];
	let change = world
		.set_transform(
			&key("parent"),
			TransformData {
				position: [4.0, 0.0, 0.0],
				..Default::default()
			},
		)
		.unwrap();
	assert_eq!(change.changed_entities, vec![key("parent"), key("child")]);
	assert!(!change.topology_changed);
	assert_eq!(world.matrix_updates() - initial_updates, 2);
	close(world.world_matrices()[0].w_axis.x, 5.0);
	assert_eq!(world.world_matrices()[1], unrelated);
	assert_eq!(data.entities[2].transform.position, [2.0, 0.0, 0.0]);
	let unchanged = world.matrix_updates();
	assert!(
		world
			.set_transform(&key("parent"), world.data().entities[2].transform.clone())
			.unwrap()
			.is_empty()
	);
	assert_eq!(world.matrix_updates(), unchanged);
}

#[test]
fn invalid_world_transform_and_descendant_overflow_preserve_document_matrices_and_counters() {
	let mut child = entity("child", [0.0; 3]).with_parent(Some(key("parent")));
	child.transform.scale = [2.0; 3];
	let mut world =
		RuntimeWorld::new(Arc::new(scene(vec![entity("parent", [0.0; 3]), child]))).unwrap();
	let before = world.document();
	let matrices = world.world_matrices().to_vec();
	let updates = world.matrix_updates();
	for (target, transform) in [
		("missing", TransformData::default()),
		(
			"parent",
			TransformData {
				position: [f32::NAN; 3],
				..Default::default()
			},
		),
		(
			"parent",
			TransformData {
				scale: [f32::MAX; 3],
				..Default::default()
			},
		),
	] {
		assert!(world.set_transform(&key(target), transform).is_err());
		assert_eq!(world.document(), before);
		assert_eq!(world.world_matrices(), matrices);
		assert_eq!(world.matrix_updates(), updates);
	}
	assert!(world.parent_matrix(2).is_err());
	assert!(world.subtree_indices(2).is_err());
}

#[test]
fn change_diff_distinguishes_topology_camera_lights_and_component_edits() {
	let before = scene(vec![entity("a", [0.0; 3]), entity("b", [1.0, 0.0, 0.0])]);
	let mut after = before.clone();
	after.entities[0].name = "Renamed".into();
	let change = WorldChange::between(&before, &after);
	assert_eq!(change.changed_entities, vec![key("a")]);
	assert!(!change.topology_changed);
	after.entities[0].parent = Some(key("b"));
	after.camera.position[0] = 1.0;
	after
		.lights
		.push(LightData::new(key("sun"), "Sun", LightKind::Directional));
	let change = WorldChange::between(&before, &after);
	assert!(change.topology_changed && change.camera_changed && change.lights_changed);
	after.entities.remove(0);
	let change = WorldChange::between(&before, &after);
	assert_eq!(change.removed_entities, vec![key("a")]);
}

#[test]
fn bounded_change_merge_deduplicates_keys_and_collapses_large_updates_to_full_resync() {
	let mut first = WorldChange {
		changed_entities: vec![key("a")],
		..Default::default()
	};
	first.merge(WorldChange {
		changed_entities: vec![key("a"), key("b")],
		camera_changed: true,
		..Default::default()
	});
	assert_eq!(first.changed_entities, vec![key("a"), key("b")]);
	assert!(first.camera_changed);
	first.merge(WorldChange {
		changed_entities: (0..MAX_CHANGE_KEYS)
			.map(|i| key(&format!("entity-{i}")))
			.collect(),
		..Default::default()
	});
	assert!(first.full_resync);
	assert!(first.changed_entities.is_empty());
	assert!(first.removed_entities.is_empty());
	first.merge(WorldChange {
		changed_entities: vec![key("c")],
		lights_changed: true,
		..Default::default()
	});
	assert!(first.changed_entities.is_empty());
	assert!(first.lights_changed);
}

#[test]
fn journal_cursors_are_monotonic_reject_future_cursors_and_do_not_replay_consumed_events() {
	let mut journal = ChangeJournal::default();
	assert_eq!(journal.cursor(), 0);
	assert!(journal.since(0).unwrap().events.is_empty());
	journal
		.publish(3, 2, BackendEventKind::StateChanged)
		.unwrap();
	journal
		.publish(
			4,
			3,
			BackendEventKind::DocumentChanged {
				changes: WorldChange {
					camera_changed: true,
					..Default::default()
				},
			},
		)
		.unwrap();
	let page = journal.since(0).unwrap();
	assert_eq!(page.cursor, 2);
	assert!(!page.resync_required);
	assert_eq!(
		page.events
			.iter()
			.map(|e| (e.sequence, e.revision, e.scene_revision))
			.collect::<Vec<_>>(),
		vec![(1, 3, 2), (2, 4, 3)]
	);
	assert_eq!(journal.since(1).unwrap().events.len(), 1);
	assert!(journal.since(2).unwrap().events.is_empty());
	assert!(journal.since(3).is_err());
}

#[test]
fn journal_entry_and_byte_eviction_make_slow_consumers_resync_without_partial_history() {
	let mut journal = ChangeJournal::default();
	for revision in 0..=ChangeJournal::MAX_ENTRIES {
		journal
			.publish(revision as u64, 0, BackendEventKind::StateChanged)
			.unwrap();
	}
	assert!(journal.since(0).unwrap().resync_required);
	assert!(journal.since(0).unwrap().events.is_empty());
	assert_eq!(
		journal.since(1).unwrap().events.len(),
		ChangeJournal::MAX_ENTRIES
	);
	let huge_key = key(&"x".repeat(256));
	let changes = WorldChange {
		changed_entities: vec![huge_key; MAX_CHANGE_KEYS],
		..Default::default()
	};
	journal
		.publish(1000, 1000, BackendEventKind::DocumentChanged { changes })
		.unwrap();
	assert!(journal.retained_bytes() <= ChangeJournal::MAX_BYTES);
	let page = journal.since(1).unwrap();
	assert!(page.resync_required);
	assert!(page.events.is_empty());
	assert!(!journal.since(journal.cursor()).unwrap().resync_required);
}

#[test]
fn steady_leaf_updates_in_a_large_world_keep_document_storage_and_skip_unrelated_matrices() {
	let data = scene(
		(0..10_000)
			.map(|i| entity(&format!("entity-{i}"), [0.0; 3]))
			.collect(),
	);
	let mut world = RuntimeWorld::new(Arc::new(data)).unwrap();
	let before = world.matrix_updates();
	let allocation = Arc::as_ptr(&world.document());
	for frame in 1..=8 {
		let change = world
			.set_transform(
				&key("entity-9999"),
				TransformData {
					position: [frame as f32, 0.0, 0.0],
					..Default::default()
				},
			)
			.unwrap();
		assert_eq!(change.changed_entities, vec![key("entity-9999")]);
		assert!(!change.full_resync);
		assert_eq!(Arc::as_ptr(&world.document()), allocation);
	}
	assert_eq!(world.matrix_updates() - before, 8);
	assert_eq!(world.world_matrices()[0], glam::Mat4::IDENTITY);
}

#[test]
fn change_journal_preserves_scene_identity_through_serialization_and_cursor_filtering() {
	let mut journal = ChangeJournal::default();
	let first = engine_core::scene_catalog::SceneId::new("first").unwrap();
	let second = engine_core::scene_catalog::SceneId::new("second").unwrap();
	journal
		.publish(1, 1, BackendEventKind::StateChanged)
		.unwrap();
	journal
		.publish_for_scene(2, 2, BackendEventKind::StateChanged, Some(first.clone()))
		.unwrap();
	journal
		.publish_for_scene(3, 3, BackendEventKind::StateChanged, Some(second.clone()))
		.unwrap();
	let events = journal.since(0).unwrap().events;
	assert!(events[0].scene_id.is_none());
	assert_eq!(events[1].scene_id, Some(first));
	assert_eq!(events[2].scene_id, Some(second.clone()));
	let encoded = serde_json::to_string(&events[2]).unwrap();
	let decoded: engine_core::events::BackendEvent = serde_json::from_str(&encoded).unwrap();
	assert_eq!(decoded.scene_id, Some(second));
	assert_eq!(decoded.sequence, 3);
	let next = journal.since(2).unwrap();
	assert_eq!(next.events.len(), 1);
	assert_eq!(next.events[0].scene_id, decoded.scene_id);
}
