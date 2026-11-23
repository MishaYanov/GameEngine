mod support;
use engine_core::{
	collider::ColliderInteraction,
	collision::{self, CollisionWorld, InteractionMatrix},
	project::PhysicsLayers,
	scene_data::{ColliderData, TransformData},
	world::{RuntimeWorld, WorldChange},
};
use std::sync::Arc;
use support::*;
fn cube(name: &str, position: [f32; 3]) -> engine_core::scene_data::EntityData {
	entity(name, position).with_collider(Some(ColliderData::Box {
		interaction: Default::default(),
		half_extents: [0.5; 3],
		offset: [0.0; 3],
	}))
}

#[test]
fn matrix_checks_the_same_raw_accepted_bits_in_both_directions() {
	let left = ColliderInteraction {
		sensor: false,
		layers: (1 << 1) | (1 << 2),
		mask: 1 << 3,
	};
	let right = ColliderInteraction {
		sensor: false,
		layers: (1 << 3) | (1 << 4),
		mask: 1 << 1,
	};
	let matrix = InteractionMatrix::compile(Some(&PhysicsLayers {
		layers: vec![],
		ignored_pairs: vec![[1, 3], [31, 31]],
	}))
	.unwrap();
	assert!(InteractionMatrix::ALLOW_ALL.allows_pair(left, right));
	assert!(!matrix.allows_pair(left, right));
	assert!(!matrix.allows_pair(right, left));
	assert!(matrix.allows_pair(
		left,
		ColliderInteraction {
			mask: 1 << 2,
			..right
		}
	));
	let edge = ColliderInteraction {
		layers: 1u32 << 31,
		..Default::default()
	};
	assert!(InteractionMatrix::ALLOW_ALL.allows_pair(edge, edge));
	assert!(!matrix.allows_pair(edge, edge));
}

#[test]
fn project_policy_persists_across_rebuilds_for_overlaps_movement_and_visibility() {
	let matrix = InteractionMatrix::compile(Some(&PhysicsLayers {
		layers: vec![],
		ignored_pairs: vec![[0, 1]],
	}))
	.unwrap();
	let layer_cube = |name, position, sensor| {
		entity(name, position).with_collider(Some(ColliderData::Box {
			interaction: ColliderInteraction {
				layers: 1 << 1,
				sensor,
				..Default::default()
			},
			half_extents: [0.5; 3],
			offset: [0.0; 3],
		}))
	};
	let mut world = RuntimeWorld::new(Arc::new(scene(vec![
		cube("actor", [-2.0, 0.0, 0.0]),
		layer_cube("wall", [0.0; 3], false),
		layer_cube("sensor", [-1.5, 0.0, 0.0], true),
		entity("target", [2.0, 0.0, 0.0]),
	])))
	.unwrap();
	let mut legacy = CollisionWorld::new(&world).unwrap();
	assert_eq!(legacy.query_overlaps().unwrap().len(), 1);
	assert!(!legacy.interaction_segment_clear(&world, &key("actor"), &key("target")));

	let mut collisions = CollisionWorld::with_matrix(&world, matrix).unwrap();
	collisions
		.synchronize(
			&world,
			&WorldChange {
				full_resync: true,
				..Default::default()
			},
		)
		.unwrap();
	assert_eq!(collisions.stats().rebuilds, 2);
	assert!(collisions.query_overlaps().unwrap().is_empty());
	assert!(collisions.interaction_segment_clear(&world, &key("actor"), &key("target")));
	let moved = collisions
		.move_entity(&mut world, &key("actor"), [4.0, 0.0, 0.0])
		.unwrap();
	assert_eq!(moved.applied_delta, [4.0, 0.0, 0.0]);
	assert!(moved.contacts.is_empty());
}

#[test]
fn persistent_overlap_queries_match_stateless_queries_after_incremental_subtree_updates() {
	let data = scene(vec![
		entity("parent", [0.0; 3]),
		cube("child", [0.0; 3]).with_parent(Some(key("parent"))),
		cube("other", [0.75, 0.0, 0.0]),
		cube("far", [10.0, 0.0, 0.0]),
	]);
	let mut world = RuntimeWorld::new(Arc::new(data)).unwrap();
	let mut collision = CollisionWorld::new(&world).unwrap();
	assert_eq!(
		collision.query_overlaps().unwrap(),
		collision::query_overlaps(world.data()).unwrap()
	);
	let stats = collision.stats();
	let changes = world
		.set_transform(
			&key("parent"),
			TransformData {
				position: [-2.0, 0.0, 0.0],
				..Default::default()
			},
		)
		.unwrap();
	collision.synchronize(&world, &changes).unwrap();
	assert_eq!(collision.stats().rebuilds, stats.rebuilds);
	assert_eq!(
		collision.stats().updated_colliders - stats.updated_colliders,
		1
	);
	assert_eq!(
		collision.query_overlaps().unwrap(),
		collision::query_overlaps(world.data()).unwrap()
	);
	assert!(collision.query_overlaps().unwrap().is_empty());
}

#[test]
fn persistent_swept_move_reports_blocker_and_only_updates_caches_for_applied_motion() {
	let mut world = RuntimeWorld::new(Arc::new(scene(vec![
		cube("actor", [-2.0, 0.0, 0.0]),
		cube("wall", [0.0; 3]),
	])))
	.unwrap();
	let mut collision = CollisionWorld::new(&world).unwrap();
	let before = world.document();
	let stats = collision.stats();
	let blocked = collision
		.move_entity(&mut world, &key("actor"), [4.0, 0.0, 0.0])
		.unwrap();
	assert_eq!(blocked.requested_delta, [4.0, 0.0, 0.0]);
	close(blocked.applied_delta[0], 0.9999);
	assert_eq!(blocked.blocked_by, Some(key("wall")));
	assert!(!blocked.changes.is_empty());
	assert_ne!(world.document(), before);
	assert_eq!(
		collision.stats().updated_colliders,
		stats.updated_colliders + 1
	);
	let moved = collision
		.move_entity(&mut world, &key("actor"), [0.0, 2.0, 0.0])
		.unwrap();
	assert_eq!(moved.applied_delta, [0.0, 2.0, 0.0]);
	assert!(moved.blocked_by.is_none());
	assert_eq!(collision.stats().rebuilds, 1);
	assert_eq!(
		collision.stats().updated_colliders - stats.updated_colliders,
		2
	);
	assert_eq!(
		collision.query_overlaps().unwrap(),
		collision::query_overlaps(world.data()).unwrap()
	);
}

#[test]
fn adding_or_removing_authored_colliders_updates_the_persistent_collision_world() {
	let original = scene(vec![entity("a", [0.0; 3]), cube("b", [0.75, 0.0, 0.0])]);
	let mut world = RuntimeWorld::new(Arc::new(original.clone())).unwrap();
	let mut collision = CollisionWorld::new(&world).unwrap();
	assert_eq!(collision.collider_count(), 1);
	let mut added = original.clone();
	added.entities[0].collider = cube("a", [0.0; 3]).collider;
	let changes = WorldChange::between(&original, &added);
	world = RuntimeWorld::new(Arc::new(added.clone())).unwrap();
	collision.synchronize(&world, &changes).unwrap();
	assert_eq!(
		collision.collider_count(),
		2,
		"new collider was ignored by incremental synchronization"
	);
	assert_eq!(
		collision.query_overlaps().unwrap(),
		collision::query_overlaps(world.data()).unwrap()
	);
	let changes = WorldChange::between(&added, &original);
	world = RuntimeWorld::new(Arc::new(original)).unwrap();
	collision.synchronize(&world, &changes).unwrap();
	assert_eq!(collision.collider_count(), 1);
	assert!(collision.query_overlaps().unwrap().is_empty());
}

#[test]
fn full_resync_rebuilds_collision_topology_and_failed_moves_preserve_world() {
	let mut world = RuntimeWorld::new(Arc::new(scene(vec![cube("a", [0.0; 3])]))).unwrap();
	let mut collision = CollisionWorld::new(&world).unwrap();
	collision
		.synchronize(
			&world,
			&WorldChange {
				full_resync: true,
				..Default::default()
			},
		)
		.unwrap();
	assert_eq!(collision.stats().rebuilds, 2);
	let before = world.document();
	let matrix_updates = world.matrix_updates();
	for (target, delta) in [("missing", [1.0; 3]), ("a", [f32::NAN; 3])] {
		assert!(
			collision
				.move_entity(&mut world, &key(target), delta)
				.is_err()
		);
		assert_eq!(world.document(), before);
		assert_eq!(world.matrix_updates(), matrix_updates);
	}
}
