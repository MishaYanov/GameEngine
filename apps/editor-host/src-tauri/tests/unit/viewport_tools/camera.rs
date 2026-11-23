use super::*;

#[test]
fn project_and_ray_round_trip_in_both_projection_modes() {
	let camera = camera();
	for projection in [
		ViewportProjection::Perspective,
		ViewportProjection::Orthographic,
	] {
		for point in [
			Vec3::ZERO,
			Vec3::new(1.0, 0.4, -0.8),
			Vec3::new(-2.0, 1.5, 1.0),
		] {
			let projected = camera.project(point, [1280, 720], projection).unwrap();
			let ray = camera
				.ray(pixel(projected), [1280, 720], projection)
				.unwrap();
			let t = (point - ray.origin).dot(ray.direction);
			approx(ray.origin + ray.direction * t, point);
		}
	}
}

#[test]
fn projection_matches_vulkan_y_down_and_depth_zero_to_one() {
	let mut camera = camera();
	camera.yaw = 0.0;
	camera.pitch = 0.0;
	for projection in [
		ViewportProjection::Perspective,
		ViewportProjection::Orthographic,
	] {
		let center = camera.project(Vec3::ZERO, [800, 600], projection).unwrap();
		let up = camera.project(Vec3::Y, [800, 600], projection).unwrap();
		let right = camera.project(Vec3::X, [800, 600], projection).unwrap();
		assert!(up.y < center.y);
		assert!(right.x > center.x);
		assert!(camera
			.project(
				camera.position() + camera.forward() * camera.near * 0.1,
				[800, 600],
				projection
			)
			.is_none());
	}
}

#[test]
fn orthographic_rays_are_parallel_with_distinct_origins() {
	let camera = camera();
	let a = camera
		.ray([200.0, 300.0], [800, 600], ViewportProjection::Orthographic)
		.unwrap();
	let b = camera
		.ray([600.0, 300.0], [800, 600], ViewportProjection::Orthographic)
		.unwrap();
	approx(a.direction, b.direction);
	assert!(a.origin.distance(b.origin) > 1.0);
	assert!(v3_distance_to_line(a.origin, b.origin, b.direction) > 1.0);
}

#[test]
fn pan_is_invariant_when_physical_pixels_and_extent_scale_together() {
	for projection in [
		ViewportProjection::Perspective,
		ViewportProjection::Orthographic,
	] {
		let mut a = camera();
		let mut b = a.clone();
		a.pan(Vec2::new(40.0, -20.0), [800, 600], projection);
		b.pan(Vec2::new(80.0, -40.0), [1600, 1200], projection);
		approx(a.pivot, b.pivot);
	}
}

#[test]
fn zoom_and_fly_remain_finite_after_extreme_input() {
	let mut camera = camera();
	for _ in 0..100 {
		camera.zoom(100.0);
	}
	assert_eq!(camera.distance, MAX_DISTANCE);
	for _ in 0..100 {
		camera.zoom(0.01);
	}
	assert_eq!(camera.distance, MIN_DISTANCE);
	camera.fly(Vec3::splat(f32::NAN), 1.0, 1.0);
	camera.orbit(Vec2::splat(f32::NAN));
	camera.pan(
		Vec2::splat(f32::NAN),
		[0, 0],
		ViewportProjection::Perspective,
	);
	assert!(camera.position().is_finite());
	let before = camera.pivot;
	camera.fly(Vec3::Z, 0.05, 2.0);
	assert!(camera.pivot.distance(before) > 0.09);
}

#[test]
fn frame_selection_fits_its_bounds_at_wide_and_portrait_aspects() {
	let selected = bounds([-4.0, -1.0, -2.0], [4.0, 3.0, 2.0]);
	for extent in [[1400, 700], [600, 1200]] {
		let mut camera = camera();
		camera.frame(selected, extent[0] as f32 / extent[1] as f32);
		for projection in [
			ViewportProjection::Perspective,
			ViewportProjection::Orthographic,
		] {
			for x in [-4.0, 4.0] {
				for y in [-1.0, 3.0] {
					for z in [-2.0, 2.0] {
						let point = camera
							.project(Vec3::new(x, y, z), extent, projection)
							.unwrap();
						assert!(
							point.x >= 0.0
								&& point.x <= extent[0] as f32
								&& point.y >= 0.0 && point.y <= extent[1] as f32
						);
					}
				}
			}
		}
	}
}

#[test]
fn orientation_views_have_finite_matrices_including_poles() {
	for orientation in [
		ViewportOrientation::Top,
		ViewportOrientation::Bottom,
		ViewportOrientation::Front,
		ViewportOrientation::Back,
		ViewportOrientation::Left,
		ViewportOrientation::Right,
		ViewportOrientation::Free,
	] {
		let mut camera = camera();
		camera.orient(orientation);
		assert!(camera
			.view_projection([1000, 800], ViewportProjection::Orthographic)
			.unwrap()
			.is_finite());
		if orientation == ViewportOrientation::Top {
			assert!(camera.forward().dot(-Vec3::Y) > 0.999);
		}
		if orientation == ViewportOrientation::Right {
			assert!(camera.forward().dot(-Vec3::X) > 0.999);
		}
	}
}

#[test]
fn invalid_input_does_not_reach_matrix_or_ray_operations() {
	let camera = camera();
	assert!(camera
		.ray([f64::NAN, 0.0], [800, 600], ViewportProjection::Perspective)
		.is_err());
	assert!(camera
		.ray([-1.0, 0.0], [800, 600], ViewportProjection::Perspective)
		.is_err());
	assert!(camera
		.ray([801.0, 0.0], [800, 600], ViewportProjection::Perspective)
		.is_err());
	assert!(camera
		.view_projection([0, 600], ViewportProjection::Perspective)
		.is_err());
	assert!(camera
		.view_projection([20000, 600], ViewportProjection::Perspective)
		.is_err());
	let mut invalid = camera.clone();
	invalid.pivot.x = f32::INFINITY;
	assert!(invalid
		.view_projection([800, 600], ViewportProjection::Perspective)
		.is_err());
}

#[test]
fn rotation_changes_group_positions_and_orientation_about_one_pivot() {
	let camera = camera();
	let extent = [1200, 800];
	let projection = ViewportProjection::Perspective;
	let gizmo = Gizmo::new(Vec3::ZERO, Quat::IDENTITY, 2.0);
	let start = camera.project(Vec3::X * 2.0, extent, projection).unwrap();
	let finish = camera.project(Vec3::Z * 2.0, extent, projection).unwrap();
	let mut drag = GizmoDrag::begin(
		&camera,
		extent,
		projection,
		pixel(start),
		gizmo,
		ViewportTool::Rotate,
		Handle::Rotate(1),
		&ViewportSettingsDto::default(),
		vec![TransformTarget {
			key: "a".into(),
			world: Mat4::from_translation(Vec3::X * 2.0),
		}],
	)
	.unwrap();
	let result = drag.preview(pixel(finish)).unwrap();
	approx(
		Vec3::from_array(result[0].transform.position),
		Vec3::Z * 2.0,
	);
	approx(
		transform_matrix(&result[0]).transform_vector3(Vec3::X),
		Vec3::Z,
	);
}
