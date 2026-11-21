use super::*;
#[test]
fn speculative_casts_share_budget_and_never_mutate_the_world() {
	let mut data = crate::SceneData::from_json(include_str!(
		"../../../../../apps/game/assets/scenes/controller-room.json"
	))
	.unwrap();
	// Keep the actual authored controller fixture, including its visual child.
	data.camera.follow = None;
	let world = crate::world::RuntimeWorld::new(std::sync::Arc::new(data)).unwrap();
	let collisions = CollisionWorld::new(&world).unwrap();
	let original = world.data().clone();
	let matrices = world.world_matrices().to_vec();
	let key = EntityKey::new("player").unwrap();
	let mut budget = 1000;
	let floor = collisions
		.cast_body(&world, &key, Vec3::ZERO, -Vec3::Y * 0.02, &mut budget)
		.unwrap();
	assert!(floor.flat_support);
	assert!(floor.contact.unwrap().normal[1] < -0.999);
	assert!(budget < 1000);
	let mut exhausted = 1;
	assert!(
		collisions
			.cast_body(&world, &key, Vec3::ZERO, Vec3::X, &mut exhausted)
			.is_err()
	);
	assert_eq!(exhausted, 0);
	assert_eq!(world.data(), &original);
	assert_eq!(world.world_matrices(), matrices);
}
