mod support;
use engine_core::{
	collision::{self, CollisionWorld},
	scene_data::ColliderData,
	world::RuntimeWorld,
};
use std::sync::Arc;
use support::{close, entity, key, scene};

fn box_at(name: &str, position: [f32; 3], half: [f32; 3]) -> engine_core::scene_data::EntityData {
	entity(name, position).with_collider(Some(ColliderData::Box {
		half_extents: half,
		offset: [0.0; 3],
		interaction: Default::default(),
	}))
}

#[test]
fn diagonal_motion_reaches_a_wall_and_preserves_the_tangent_component() {
	let data = scene(vec![
		box_at("actor", [-2.0, 0.0, 0.0], [0.5; 3]),
		box_at("wall", [0.0; 3], [0.5, 10.0, 10.0]),
	]);
	let mut world = RuntimeWorld::new(Arc::new(data.clone())).unwrap();
	let mut collision = CollisionWorld::new(&world).unwrap();
	let result = collision
		.move_entity(&mut world, &key("actor"), [4.0, 2.0, 0.0])
		.unwrap();
	let position = world.data().entities[0].transform.position;
	assert!(position[0] <= -1.0 && position[0] > -1.001);
	close(position[1], 2.0);
	assert_eq!(result.blocked_by, Some(key("wall")));
	assert_eq!(result.contacts.len(), 1);
	assert_eq!(result.contacts[0].entity, key("wall"));
	assert_eq!(result.contacts[0].normal, [1.0, 0.0, 0.0]);
	assert!((result.contacts[0].fraction - 0.25).abs() < 1e-6);
	assert!(
		collision
			.query_overlaps()
			.unwrap()
			.iter()
			.all(|contact| contact.a != key("actor") && contact.b != key("actor"))
	);
	let mut stateless = data;
	collision::move_entity(&mut stateless, &key("actor"), [4.0, 2.0, 0.0]).unwrap();
	assert_eq!(world.data(), &stateless);
}

#[test]
fn corner_contacts_stop_without_tunneling_and_work_failure_keeps_all_transforms() {
	let data = scene(vec![
		box_at("actor", [-2.0, -2.0, 0.0], [0.5; 3]),
		box_at("x-wall", [0.0; 3], [0.5, 10.0, 10.0]),
		box_at("y-wall", [0.0; 3], [10.0, 0.5, 10.0]),
	]);
	let mut world = RuntimeWorld::new(Arc::new(data.clone())).unwrap();
	let mut collision = CollisionWorld::new(&world).unwrap();
	let mut budget = 2;
	assert!(
		collision
			.move_entity_with_budget(&mut world, &key("actor"), [20.0, 20.0, 0.0], &mut budget)
			.is_err()
	);
	assert_eq!(world.data(), &data);
	let result = collision
		.move_entity(&mut world, &key("actor"), [20.0, 20.0, 0.0])
		.unwrap();
	assert_eq!(result.contacts.len(), 2);
	assert_eq!(result.contacts[0].normal, [1.0, 0.0, 0.0]);
	assert_eq!(result.contacts[1].normal, [0.0, 1.0, 0.0]);
	let position = world.data().entities[0].transform.position;
	assert!(position[0] <= -1.0 && position[1] <= -1.0);
	assert!(position[0] > -1.001 && position[1] > -1.001);
	assert!(
		collision
			.query_overlaps()
			.unwrap()
			.iter()
			.all(|contact| contact.a != key("actor") && contact.b != key("actor"))
	);
}

#[test]
fn sensor_query_charges_filtered_solid_pairs_without_spending_the_contact_limit() {
	let mut data = scene(vec![
		box_at("a", [0.0; 3], [0.5; 3]),
		box_at("b", [0.0; 3], [0.5; 3]),
	]);
	let world = RuntimeWorld::new(Arc::new(data.clone())).unwrap();
	let mut cache = CollisionWorld::new(&world).unwrap();
	let mut budget = 1;
	assert!(
		cache
			.query_sensor_overlaps_with_budget(&mut budget)
			.unwrap()
			.is_empty()
	);
	assert_eq!(budget, 0);
	data.entities[0].collision_enabled = false;
	let changed = engine_core::world::WorldChange::between(world.data(), &data);
	let world = RuntimeWorld::new(Arc::new(data)).unwrap();
	cache.synchronize(&world, &changed).unwrap();
	assert_eq!(cache.collider_count(), 1);
}
