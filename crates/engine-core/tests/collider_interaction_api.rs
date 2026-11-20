mod support;

use engine_core::{
	collider::{ColliderData, ColliderInteraction},
	collision::{self, CollisionWorld, Contact},
	scene_data::EntityData,
	world::{RuntimeWorld, WorldChange},
};
use serde_json::json;
use std::sync::Arc;
use support::{entity, key, scene};

fn interaction(sensor: bool, layers: u32, mask: u32) -> ColliderInteraction {
	ColliderInteraction {
		sensor,
		layers,
		mask,
	}
}

fn cube(name: &str, position: [f32; 3], interaction: ColliderInteraction) -> EntityData {
	entity(name, position).with_collider(Some(ColliderData::Box {
		half_extents: [0.5; 3],
		offset: [0.0; 3],
		interaction,
	}))
}

fn sphere(name: &str, position: [f32; 3], interaction: ColliderInteraction) -> EntityData {
	entity(name, position).with_collider(Some(ColliderData::Sphere {
		radius: 0.5,
		offset: [0.0; 3],
		interaction,
	}))
}

#[test]
fn legacy_and_partial_interactions_preserve_geometry_and_default_to_solid_detection() {
	for legacy in [
		json!({"kind":"box","half_extents":[0.5,0.5,0.5],"offset":[0.0,0.0,0.0]}),
		json!({"kind":"sphere","radius":0.5,"offset":[0.0,0.0,0.0]}),
	] {
		let shape: ColliderData = serde_json::from_value(legacy.clone()).unwrap();
		assert_eq!(shape.interaction(), ColliderInteraction::default());
		assert_eq!(serde_json::to_value(&shape).unwrap(), legacy);
		let mut explicit_default = legacy.clone();
		explicit_default["interaction"] = json!({});
		let defaulted: ColliderData = serde_json::from_value(explicit_default).unwrap();
		assert_eq!(shape, defaulted);
		let mut partial = legacy;
		partial["interaction"] = json!({"sensor":true});
		let parsed: ColliderData = serde_json::from_value(partial).unwrap();
		assert_eq!(parsed.interaction(), interaction(true, 1, u32::MAX));
		let document = scene(vec![entity("sensor", [0.0; 3]).with_collider(Some(parsed))]);
		let saved = document.to_json().unwrap();
		assert_eq!(engine_core::SceneData::from_json(&saved).unwrap(), document);
	}
	let old_contact: Contact = serde_json::from_value(json!({
		"a":"a","b":"b","normal":[1,0,0],"penetration":0.25
	}))
	.unwrap();
	assert!(!old_contact.sensor);
}

#[test]
fn interaction_transport_rejects_invalid_fields_and_accepts_every_unsigned_bit() {
	let shape =
		|settings| json!({"kind":"sphere","radius":1,"offset":[0,0,0],"interaction":settings});
	for settings in [
		json!({"sensor":1}),
		json!({"sensor":null}),
		json!({"layers":-1}),
		json!({"layers":4294967296_u64}),
		json!({"layers":1.5}),
		json!({"mask":-1}),
		json!({"mask":"1"}),
		json!({"mask":null}),
		json!({"mask":4294967296_u64}),
		json!({"unknown":true}),
		json!(null),
		json!([]),
		json!([true]),
		json!([true, 1, 4294967295_u32]),
	] {
		assert!(
			serde_json::from_value::<ColliderData>(shape(settings.clone())).is_err(),
			"accepted {settings}"
		);
	}
	for settings in [
		json!({"layers":0,"mask":0}),
		json!({"layers":2147483648_u32,"mask":u32::MAX}),
	] {
		let parsed: ColliderData = serde_json::from_value(shape(settings)).unwrap();
		parsed.validate().unwrap();
		assert_eq!(
			serde_json::from_value::<ColliderData>(serde_json::to_value(&parsed).unwrap()).unwrap(),
			parsed
		);
	}
	for settings in [
		r#"{"sensor":false,"sensor":true}"#,
		r#"{"layers":1,"layers":2}"#,
		r#"{"mask":1,"mask":2}"#,
	] {
		let source =
			format!(r#"{{"kind":"sphere","radius":1,"offset":[0,0,0],"interaction":{settings}}}"#);
		assert!(
			serde_json::from_str::<ColliderData>(&source)
				.unwrap_err()
				.to_string()
				.contains("duplicate field")
		);
	}
}

#[test]
fn cached_and_stateless_overlaps_apply_both_masks_and_identify_sensor_pairs() {
	let solid = ColliderInteraction::default();
	for (first, second, detected, sensor) in [
		(solid, solid, true, false),
		(interaction(true, 1, u32::MAX), solid, true, true),
		(solid, interaction(true, 1, u32::MAX), true, true),
		(
			interaction(true, 1, u32::MAX),
			interaction(true, 1, u32::MAX),
			true,
			true,
		),
		(
			interaction(false, 1, 2),
			interaction(false, 2, 1),
			true,
			false,
		),
		(
			interaction(false, 1, 2),
			interaction(false, 2, 0),
			false,
			false,
		),
		(
			interaction(false, 1, 0),
			interaction(false, 2, 1),
			false,
			false,
		),
		(interaction(true, 0, u32::MAX), solid, false, false),
		(solid, interaction(true, 1, 0), false, false),
		(
			interaction(false, 1 << 31, 1),
			interaction(false, 1, 1 << 31),
			true,
			false,
		),
		(
			interaction(false, 1 << 31, 1),
			interaction(false, 1, 1 << 30),
			false,
			false,
		),
	] {
		let mut document = scene(vec![
			cube("a", [0.0; 3], first),
			sphere("b", [0.75, 0.0, 0.0], second),
		]);
		let expected = collision::query_overlaps(&document).unwrap();
		assert_eq!(
			expected.len(),
			usize::from(detected),
			"{first:?}, {second:?}"
		);
		if detected {
			assert_eq!(expected[0].sensor, sensor);
		}
		let world = RuntimeWorld::new(Arc::new(document.clone())).unwrap();
		assert_eq!(
			CollisionWorld::new(&world)
				.unwrap()
				.query_overlaps()
				.unwrap(),
			expected
		);
		document.entities.reverse();
		assert_eq!(collision::query_overlaps(&document).unwrap(), expected);
	}
}

#[test]
fn sensors_and_filtered_pairs_never_block_cached_or_stateless_swept_movement() {
	let solid = ColliderInteraction::default();
	for (mover, obstacle, blocked) in [
		(solid, solid, true),
		(interaction(true, 1, u32::MAX), solid, false),
		(solid, interaction(true, 1, u32::MAX), false),
		(
			interaction(true, 1, u32::MAX),
			interaction(true, 1, u32::MAX),
			false,
		),
		(interaction(false, 1, 0), solid, false),
		(solid, interaction(false, 0, u32::MAX), false),
		(interaction(false, 1, 2), interaction(false, 2, 0), false),
		(
			interaction(false, 1 << 31, 1),
			interaction(false, 1, 1 << 31),
			true,
		),
	] {
		let original = scene(vec![
			cube("actor", [-2.0, 0.0, 0.0], mover),
			sphere("wall", [0.0; 3], obstacle),
		]);
		let mut authored = original.clone();
		collision::move_entity(&mut authored, &key("actor"), [4.0, 0.0, 0.0]).unwrap();
		let mut world = RuntimeWorld::new(Arc::new(original)).unwrap();
		let mut cache = CollisionWorld::new(&world).unwrap();
		let moved = cache
			.move_entity(&mut world, &key("actor"), [4.0, 0.0, 0.0])
			.unwrap();
		assert_eq!(world.data(), &authored);
		assert_eq!(
			moved.blocked_by.is_some(),
			blocked,
			"{mover:?}, {obstacle:?}"
		);
		assert_eq!(
			authored.entities[0].transform.position[0],
			if blocked { -2.0 } else { 2.0 }
		);
		assert_eq!(
			cache.query_overlaps().unwrap(),
			collision::query_overlaps(&authored).unwrap()
		);
	}
}

#[test]
fn moving_a_parent_checks_solid_descendants_and_ignores_sensor_descendants() {
	let solid = ColliderInteraction::default();
	let sensor = interaction(true, 1, u32::MAX);
	for (solid_y, sensor_y, blocked) in [(2.0, 0.0, false), (0.0, 2.0, true)] {
		let mut document = scene(vec![
			cube("parent", [-2.0, 0.0, 0.0], sensor),
			cube("solid-child", [0.0, solid_y, 0.0], solid).with_parent(Some(key("parent"))),
			cube("sensor-child", [0.0, sensor_y, 0.0], sensor).with_parent(Some(key("parent"))),
			cube("wall", [0.0; 3], solid),
		]);
		let mut world = RuntimeWorld::new(Arc::new(document.clone())).unwrap();
		let mut cache = CollisionWorld::new(&world).unwrap();
		let result = cache
			.move_entity(&mut world, &key("parent"), [4.0, 0.0, 0.0])
			.unwrap();
		collision::move_entity(&mut document, &key("parent"), [4.0, 0.0, 0.0]).unwrap();
		assert_eq!(world.data(), &document);
		assert_eq!(result.blocked_by.is_some(), blocked);
		assert_eq!(
			document.entities[0].transform.position[0],
			if blocked { -2.0 } else { 2.0 }
		);
		assert_eq!(document.entities[1].transform.position[1], solid_y);
		assert_eq!(document.entities[2].transform.position[1], sensor_y);
	}
}

#[test]
fn interaction_only_updates_refresh_cached_queries_and_movement_without_rebuilding() {
	let mut previous = scene(vec![
		cube("a", [0.0; 3], Default::default()),
		cube("b", [0.75, 0.0, 0.0], Default::default()),
	]);
	let mut world = RuntimeWorld::new(Arc::new(previous.clone())).unwrap();
	let mut cache = CollisionWorld::new(&world).unwrap();
	for settings in [
		interaction(true, 1, u32::MAX),
		interaction(false, 1, 0),
		interaction(false, 0, u32::MAX),
		ColliderInteraction::default(),
		interaction(true, 1, u32::MAX),
	] {
		let mut next = previous.clone();
		next.entities[1] = cube("b", [0.75, 0.0, 0.0], settings);
		let change = WorldChange::between(&previous, &next);
		assert!(!change.topology_changed && !change.full_resync);
		assert_eq!(change.changed_entities, vec![key("b")]);
		world = RuntimeWorld::new(Arc::new(next.clone())).unwrap();
		let stats = cache.stats();
		cache.synchronize(&world, &change).unwrap();
		assert_eq!(cache.stats().rebuilds, stats.rebuilds);
		assert_eq!(cache.stats().updated_colliders, stats.updated_colliders + 1);
		assert_eq!(
			cache.query_overlaps().unwrap(),
			collision::query_overlaps(&next).unwrap()
		);
		previous = next;
	}
	assert!(cache.query_overlaps().unwrap()[0].sensor);
	let moved = cache
		.move_entity(&mut world, &key("a"), [2.0, 0.0, 0.0])
		.unwrap();
	assert_eq!(moved.applied_delta, [2.0, 0.0, 0.0]);
}

#[test]
fn filtered_and_sensor_candidates_still_consume_the_movement_work_budget() {
	for settings in [interaction(true, 1, u32::MAX), interaction(false, 1, 0)] {
		let document = scene(vec![
			cube("actor", [-2.0, 0.0, 0.0], settings),
			cube("wall", [0.0; 3], Default::default()),
		]);
		let mut world = RuntimeWorld::new(Arc::new(document.clone())).unwrap();
		let mut cache = CollisionWorld::new(&world).unwrap();
		let mut remaining = 2;
		assert!(
			cache
				.move_entity_with_budget(&mut world, &key("actor"), [4.0, 0.0, 0.0], &mut remaining)
				.unwrap_err()
				.contains("budget")
		);
		assert_eq!(remaining, 0);
		assert_eq!(world.data(), &document);
		let mut remaining = 3;
		assert_eq!(
			cache
				.move_entity_with_budget(&mut world, &key("actor"), [4.0, 0.0, 0.0], &mut remaining)
				.unwrap()
				.applied_delta,
			[4.0, 0.0, 0.0]
		);
		assert_eq!(remaining, 0);
	}
}

#[test]
fn disabled_detection_still_bounds_dense_overlap_query_work() {
	let document = scene(
		(0..1415)
			.map(|index| {
				cube(
					&format!("entity-{index}"),
					[0.0; 3],
					interaction(false, 1, 0),
				)
			})
			.collect(),
	);
	assert!(
		collision::query_overlaps(&document)
			.unwrap_err()
			.contains("broadphase candidates")
	);
	let world = RuntimeWorld::new(Arc::new(document)).unwrap();
	assert!(
		CollisionWorld::new(&world)
			.unwrap()
			.query_overlaps()
			.unwrap_err()
			.contains("broadphase candidates")
	);
}
