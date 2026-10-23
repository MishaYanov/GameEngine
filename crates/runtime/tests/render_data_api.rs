use glam::{Mat4, Quat, Vec3};
use renderer::vulkan::Vertex;
use runtime::{
	Bounds3, Transform,
	assets::{MeshData, Model},
	model_instance::ModelInstance,
	scene::Scene,
	visibility::Frustum,
};
use std::sync::Arc;

fn close(a: f32, b: f32) {
	assert!((a - b).abs() < 1e-5, "{a} != {b}");
}

#[test]
fn missing_normals_are_area_weighted_and_explicit_normals_are_preserved() {
	let vertices = vec![
		Vertex::new([0.0, 0.0, 0.0], [0.0; 2]),
		Vertex::new([2.0, 0.0, 0.0], [0.0; 2]),
		Vertex::new([0.0, 2.0, 0.0], [0.0; 2]),
		Vertex::new([0.0, 0.0, 1.0], [0.0; 2]),
		Vertex::new([9.0, 9.0, 9.0], [0.0; 2]).with_normal([1.0, 0.0, 0.0]),
	];
	// First face contributes +4Z; second face contributes +2Y at shared vertices.
	let mesh = MeshData::new(vertices, vec![0, 1, 2, 0, 3, 1]);
	let expected = Vec3::new(0.0, 2.0, 4.0).normalize();
	for index in [0, 1] {
		let normal = Vec3::from_array(mesh.vertices()[index].normal);
		close(normal.distance(expected), 0.0);
	}
	assert_eq!(mesh.vertices()[4].normal, [1.0, 0.0, 0.0]);
	assert_eq!(mesh.vertices()[2].normal, [0.0, 0.0, 1.0]);
	assert_eq!(mesh.vertices()[3].normal, [0.0, 1.0, 0.0]);
}

#[test]
fn degenerate_or_unused_vertices_receive_finite_fallback_normals() {
	let mesh = MeshData::new(
		vec![Vertex::new([0.0; 3], [0.0; 2]); 4],
		vec![0, 1, 2, 0, 99, 2],
	);
	for vertex in mesh.vertices() {
		assert_eq!(vertex.normal, [0.0, 0.0, 1.0]);
	}
}

#[test]
fn vulkan_frustum_tests_all_six_planes_and_keeps_touching_bounds() {
	let frustum = Frustum::from_view_projection(Mat4::IDENTITY).unwrap();
	assert!(frustum.intersects(Bounds3::new([-0.5, -0.5, 0.1], [0.5, 0.5, 0.9]).unwrap()));
	for (min, max) in [
		([1.01, 0.0, 0.1], [2.0, 0.1, 0.2]),
		([-2.0, 0.0, 0.1], [-1.01, 0.1, 0.2]),
		([0.0, 1.01, 0.1], [0.1, 2.0, 0.2]),
		([0.0, -2.0, 0.1], [0.1, -1.01, 0.2]),
		([0.0, 0.0, -1.0], [0.1, 0.1, -0.01]),
		([0.0, 0.0, 1.01], [0.1, 0.1, 2.0]),
	] {
		assert!(!frustum.intersects(Bounds3::new(min, max).unwrap()));
	}
	assert!(frustum.intersects(Bounds3::new([1.0, -0.1, 0.0], [2.0, 0.1, 1.0]).unwrap()));
	assert!(Frustum::from_view_projection(Mat4::ZERO).is_err());
	assert!(Frustum::from_view_projection(Mat4::from_scale(Vec3::splat(f32::NAN))).is_err());
}

#[test]
fn render_visibility_counts_entities_and_conservatively_keeps_unknown_bounds() {
	let bounds = Bounds3::new([-0.1; 3], [0.1; 3]).unwrap();
	let known = Arc::new(Model::with_bounds(vec![], Some(bounds)));
	let mut scene = Scene::default();
	for (x, scale) in [(0.0, [1.0; 3]), (5.0, [-2.0, 3.0, 0.5])] {
		scene.spawn(ModelInstance::new(
			Arc::clone(&known),
			Transform {
				position: [x, 0.0, 0.5],
				rotation: [0.0, 0.0, 0.2],
				scale,
			},
		));
	}
	scene.spawn(ModelInstance::new(
		Arc::new(Model::new(vec![])),
		Transform {
			position: [100.0; 3],
			..Transform::default()
		},
	));
	let frustum = Frustum::from_view_projection(Mat4::IDENTITY).unwrap();
	let (objects, stats) = scene.render_visible(Some(&frustum));
	assert!(objects.is_empty()); // CPU fixtures have bounds but no GPU mesh parts.
	assert_eq!(
		(
			stats.total_entities,
			stats.visible_entities,
			stats.culled_entities
		),
		(3, 2, 1)
	);
	assert_eq!((stats.draw_calls, stats.triangles), (0, 0));
	let (_, disabled) = scene.render_visible(None);
	assert_eq!(
		(
			disabled.total_entities,
			disabled.visible_entities,
			disabled.culled_entities
		),
		(3, 3, 0)
	);
	let rotated = bounds
		.transformed(Mat4::from_scale_rotation_translation(
			Vec3::new(-1.0, 2.0, 0.5),
			Quat::from_rotation_z(0.7),
			Vec3::new(0.0, 0.0, 0.5),
		))
		.unwrap();
	assert!(frustum.intersects(rotated));
}
