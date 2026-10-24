mod support;
use runtime::{
	collision::{self, DebugShape},
	scene_data::ColliderData,
};
use support::*;

fn cube(name: &str, position: [f32; 3]) -> runtime::scene_data::EntityData {
	entity(name, position).with_collider(Some(ColliderData::Box {
		half_extents: [0.5; 3],
		offset: [0.0; 3],
	}))
}
fn sphere(name: &str, position: [f32; 3]) -> runtime::scene_data::EntityData {
	entity(name, position).with_collider(Some(ColliderData::Sphere {
		radius: 0.5,
		offset: [0.0; 3],
	}))
}

#[test]
fn overlap_contacts_are_deterministic_sorted_and_normals_point_from_a_to_b() {
	let mut data = scene(vec![
		cube("z", [0.75, 0.0, 0.0]),
		cube("a", [0.0; 3]),
		sphere("b", [0.0, 1.0, 0.0]),
	]);
	let contacts = collision::query_overlaps(&data).unwrap();
	assert_eq!(contacts.len(), 2);
	assert_eq!((contacts[0].a.as_str(), contacts[0].b.as_str()), ("a", "b"));
	assert_eq!((contacts[1].a.as_str(), contacts[1].b.as_str()), ("a", "z"));
	assert_eq!(contacts[1].normal, [1.0, 0.0, 0.0]);
	close(contacts[1].penetration, 0.25);
	data.entities.reverse();
	assert_eq!(collision::query_overlaps(&data).unwrap(), contacts);
}

#[test]
fn curved_narrow_phase_excludes_aabb_corner_false_positives_and_reports_containment() {
	let data = scene(vec![sphere("a", [0.0; 3]), sphere("b", [0.9, 0.9, 0.0])]);
	assert!(collision::query_overlaps(&data).unwrap().is_empty());
	let mut large = cube("box", [0.0; 3]);
	large.collider = Some(ColliderData::Box {
		half_extents: [2.0; 3],
		offset: [0.0; 3],
	});
	let data = scene(vec![large, sphere("sphere", [0.0; 3])]);
	let contacts = collision::query_overlaps(&data).unwrap();
	assert_eq!(contacts.len(), 1);
	close(contacts[0].penetration, 2.5);
	assert!(contacts[0].normal.iter().all(|v| v.is_finite()));
}

#[test]
fn swept_motion_cannot_tunnel_through_box_or_sphere_and_free_motion_moves_exactly() {
	for moving in [
		cube("moving", [-2.0, 0.0, 0.0]),
		sphere("moving", [-2.0, 0.0, 0.0]),
	] {
		for obstacle in [cube("obstacle", [0.0; 3]), sphere("obstacle", [0.0; 3])] {
			let mut data = scene(vec![moving.clone(), obstacle]);
			let before = data.clone();
			collision::move_entity(&mut data, &key("moving"), [4.0, 0.0, 0.0]).unwrap();
			assert_eq!(data, before);
			collision::move_entity(&mut data, &key("moving"), [0.0, 2.0, 0.0]).unwrap();
			assert_eq!(data.entities[0].transform.position, [-2.0, 2.0, 0.0]);
		}
	}
}

#[test]
fn touching_and_overlapping_shapes_can_separate_but_cannot_move_deeper() {
	for distance in [0.75, 1.0] {
		let mut data = scene(vec![cube("a", [0.0; 3]), cube("b", [distance, 0.0, 0.0])]);
		let before = data.clone();
		collision::move_entity(&mut data, &key("a"), [0.1, 0.0, 0.0]).unwrap();
		assert_eq!(data, before);
		collision::move_entity(&mut data, &key("a"), [-0.5, 0.0, 0.0]).unwrap();
		assert_eq!(data.entities[0].transform.position, [-0.5, 0.0, 0.0]);
	}
}

#[test]
fn moving_noncollider_parent_sweeps_its_children_and_ignores_contacts_within_its_subtree() {
	let mut data = scene(vec![
		entity("parent", [-2.0, 0.0, 0.0]),
		cube("child", [0.0; 3]).with_parent(Some(key("parent"))),
		cube("sibling", [0.2, 0.0, 0.0]).with_parent(Some(key("parent"))),
		cube("wall", [0.0; 3]),
	]);
	let before = data.clone();
	collision::move_entity(&mut data, &key("parent"), [4.0, 0.0, 0.0]).unwrap();
	assert_eq!(data, before);
	collision::move_entity(&mut data, &key("parent"), [0.0, 2.0, 0.0]).unwrap();
	assert_eq!(data.entities[0].transform.position, [-2.0, 2.0, 0.0]);
	assert_eq!(data.entities[1].transform.position, [0.0; 3]);
}

#[test]
fn world_movement_under_rotated_scaled_parent_converts_to_local_and_errors_are_atomic() {
	let mut parent = entity("parent", [0.0; 3]);
	parent.transform.rotation[2] = std::f32::consts::FRAC_PI_2;
	parent.transform.scale = [2.0; 3];
	let mut data = scene(vec![
		parent,
		cube("child", [0.0; 3]).with_parent(Some(key("parent"))),
	]);
	collision::move_entity(&mut data, &key("child"), [2.0, 0.0, 0.0]).unwrap();
	close(data.entities[1].transform.position[0], 0.0);
	close(data.entities[1].transform.position[1], -1.0);
	let before = data.clone();
	for (target, delta) in [("missing", [1.0; 3]), ("child", [f32::NAN; 3])] {
		assert!(collision::move_entity(&mut data, &key(target), delta).is_err());
		assert_eq!(data, before);
	}
	data.entities[0].transform.scale = [0.0, 1.0, 1.0];
	let before = data.clone();
	assert!(collision::move_entity(&mut data, &key("child"), [1.0; 3]).is_err());
	assert_eq!(data, before);
}

#[test]
fn sphere_debug_bounds_conservatively_enclose_hierarchy_shear_and_offset() {
	let mut parent = entity("parent", [1.0, 2.0, 3.0]);
	parent.transform.scale = [3.0, 1.0, 1.0];
	let mut child = sphere("child", [0.0; 3]).with_parent(Some(key("parent")));
	child.transform.rotation[2] = 0.7;
	child.collider = Some(ColliderData::Sphere {
		radius: 1.0,
		offset: [1.0, 0.0, 0.0],
	});
	let data = scene(vec![parent, child]);
	let world = data.world_matrices().unwrap()[1];
	let shapes = collision::debug_shapes(&data).unwrap();
	let DebugShape::Sphere {
		center,
		radius,
		min,
		max,
		..
	} = &shapes[0]
	else {
		panic!("expected sphere")
	};
	let expected = world.transform_point3(glam::Vec3::X);
	for axis in 0..3 {
		close(center[axis], expected[axis]);
	}
	for i in 0..360 {
		let angle = (i as f32).to_radians();
		let point =
			world.transform_point3(glam::Vec3::X + glam::Vec3::new(angle.cos(), angle.sin(), 0.0));
		assert!(point.distance(expected) <= *radius + 1e-5);
		for axis in 0..3 {
			assert!(point[axis] >= min[axis] - 1e-5 && point[axis] <= max[axis] + 1e-5);
		}
	}
}

#[test]
fn dense_contact_budget_fails_explicitly_without_mutating_input() {
	let data = scene(
		(0..100)
			.map(|i| cube(&format!("box-{i}"), [0.0; 3]))
			.collect(),
	);
	let before = data.clone();
	let error = collision::query_overlaps(&data).unwrap_err();
	assert!(error.contains("4096"), "{error}");
	assert_eq!(data, before);
}
