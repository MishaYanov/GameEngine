use super::*;

#[test]
fn explicit_behavior_target_moves_without_selection_demo_bindings_or_implicit_animation() {
	let data = Arc::new(scene(vec![
		entity("uncontrolled", [2.0, 0.0, 0.0]),
		entity("actor", [0.0; 3]),
	]));
	let mut session = SimulationSession::new(
		project(&data, vec![controlled("actor", 2.0, BehaviorSpace::World)]),
		Arc::clone(&data),
	)
	.unwrap();
	assert!(!session.needs_tick());
	assert!(session.set_input(&key("actor"), [3.0, 4.0, 0.0]).unwrap());
	assert!(!session.set_input(&key("actor"), [3.0, 4.0, 0.0]).unwrap());
	let outcome = session.tick(0.1).unwrap();
	assert_eq!(outcome.steps, 6);
	assert_eq!(outcome.tick, 6);
	close(session.document().entities[1].transform.position[0], 0.12);
	close(session.document().entities[1].transform.position[1], 0.16);
	assert_eq!(session.document().entities[0], data.entities[0]);
	assert_eq!(data.entities[1].transform.position, [0.0; 3]);
	assert_eq!(outcome.changes.changed_entities, vec![key("actor")]);
	assert!(
		session
			.set_input(&key("uncontrolled"), [1.0, 0.0, 0.0])
			.is_err()
	);
	let mut empty = SimulationSession::new(project(&data, vec![]), Arc::clone(&data)).unwrap();
	assert!(!empty.needs_tick());
	assert!(empty.set_controlled_input([1.0, 0.0, 0.0]).is_err());
	assert_eq!(empty.tick(1.0).unwrap().steps, 0);
	assert_eq!(empty.document(), data);
}

#[test]
fn local_and_world_movement_respect_rotated_scaled_parent_without_changing_speed() {
	let mut parent = entity("parent", [0.0; 3]);
	parent.transform.rotation[2] = std::f32::consts::FRAC_PI_2;
	parent.transform.scale = [2.0; 3];
	let data = Arc::new(scene(vec![
		parent,
		entity("actor", [0.0; 3]).with_parent(Some(key("parent"))),
	]));
	for space in [BehaviorSpace::Local, BehaviorSpace::World] {
		let mut session = SimulationSession::new(
			project(&data, vec![controlled("actor", 2.0, space)]),
			Arc::clone(&data),
		)
		.unwrap();
		session.set_controlled_input([1.0, 0.0, 0.0]).unwrap();
		session.tick(0.1).unwrap();
		let world = session.world().world_matrices()[1].w_axis.truncate();
		close(world.length(), 0.2);
		match space {
			BehaviorSpace::Local => {
				close(world.x, 0.0);
				close(world.y, 0.2);
			}
			BehaviorSpace::World => {
				close(world.x, 0.2);
				close(world.y, 0.0);
			}
		}
	}
}

#[test]
fn configured_move_and_spin_compose_on_the_same_entity_instead_of_overwriting_motion() {
	let data = Arc::new(scene(vec![entity("actor", [0.0; 3])]));
	for reversed in [false, true] {
		let mut behaviors = vec![
			controlled("actor", 2.0, BehaviorSpace::World),
			BehaviorSpec::Spin {
				entity: key("actor"),
				angular_velocity: [0.0, 0.0, 1.0],
			},
		];
		if reversed {
			behaviors.reverse();
		}
		let mut session =
			SimulationSession::new(project(&data, behaviors), Arc::clone(&data)).unwrap();
		session.set_controlled_input([1.0, 0.0, 0.0]).unwrap();
		session.tick(0.1).unwrap();
		close(session.document().entities[0].transform.position[0], 0.2);
		close(session.document().entities[0].transform.rotation[2], 0.1);
	}
}

#[test]
fn shared_simulation_uses_persistent_swept_collision_and_advances_to_contact() {
	let collider = Some(ColliderData::Box {
		interaction: Default::default(),
		half_extents: [0.5; 3],
		offset: [0.0; 3],
	});
	let data = Arc::new(scene(vec![
		entity("actor", [-2.0, 0.0, 0.0]).with_collider(collider.clone()),
		entity("wall", [0.0; 3]).with_collider(collider),
	]));
	let mut session = SimulationSession::new(
		project(
			&data,
			vec![controlled("actor", 240.0, BehaviorSpace::World)],
		),
		Arc::clone(&data),
	)
	.unwrap();
	session.set_controlled_input([1.0, 0.0, 0.0]).unwrap();
	let stats = session.collision_stats();
	let outcome = session.tick(1.0 / 60.0).unwrap();
	assert!(!outcome.changes.is_empty());
	close(
		session.document().entities[0].transform.position[0],
		-1.0001,
	);
	assert_eq!(data.entities[0].transform.position[0], -2.0);
	assert_eq!(session.collision_stats().rebuilds, stats.rebuilds);
	assert_eq!(
		session.collision_stats().updated_colliders,
		stats.updated_colliders + 1
	);
}
