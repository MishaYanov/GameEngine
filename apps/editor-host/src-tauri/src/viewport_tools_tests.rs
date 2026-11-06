use super::*;

fn approx(a: Vec3, b: Vec3) {
	assert!(a.distance(b) < 0.005, "{a:?} differs from {b:?}");
}
fn pixel(point: Vec2) -> [f64; 2] {
	[point.x as f64, point.y as f64]
}
fn bounds(min: [f32; 3], max: [f32; 3]) -> Bounds3 {
	Bounds3::new(min, max).unwrap()
}
fn camera() -> EditorCamera {
	EditorCamera::default()
}
fn identity_target(key: &str) -> TransformTarget {
	TransformTarget {
		key: key.into(),
		world: Mat4::IDENTITY,
	}
}
fn transform_matrix(edit: &TransformEditDto) -> Mat4 {
	runtime::scene_data::TransformData {
		position: edit.transform.position,
		rotation: edit.transform.rotation,
		scale: edit.transform.scale,
	}
	.matrix()
}
fn axis_drag(
	settings: ViewportSettingsDto,
	tool: ViewportTool,
	targets: Vec<TransformTarget>,
) -> (GizmoDrag, Vec2, Vec2) {
	let camera = camera();
	let extent = [1200, 800];
	let gizmo = Gizmo::new(Vec3::ZERO, Quat::IDENTITY, 2.0);
	let start = camera
		.project(Vec3::ZERO, extent, settings.projection)
		.unwrap();
	let axis = camera
		.project(Vec3::X * 2.0, extent, settings.projection)
		.unwrap()
		- start;
	let drag = GizmoDrag::begin(
		&camera,
		extent,
		settings.projection,
		pixel(start),
		gizmo,
		tool,
		Handle::Axis(0),
		&settings,
		targets,
	)
	.unwrap();
	(drag, start, axis)
}

#[test]
fn initialization_preserves_authored_pose_but_navigation_only_changes_the_editor_value() {
	let authored = runtime::Camera {
		position: [4.0, 2.0, 8.0],
		yaw: -0.3,
		pitch: -0.2,
		..Default::default()
	};
	let before = authored.view_projection(1.5).unwrap();
	let mut editor = EditorCamera::from_camera(&authored);
	approx(editor.position(), Vec3::from_array(authored.position));
	editor.orbit(Vec2::new(80.0, 30.0));
	editor.pan(
		Vec2::new(30.0, -20.0),
		[1200, 800],
		ViewportProjection::Perspective,
	);
	editor.zoom(0.7);
	assert_eq!(authored.view_projection(1.5).unwrap(), before);
	assert!(
		editor
			.position()
			.distance(Vec3::from_array(authored.position))
			> 1.0
	);
}

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
fn v3_distance_to_line(point: Vec3, origin: Vec3, direction: Vec3) -> f32 {
	(point - origin - direction * (point - origin).dot(direction)).length()
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
fn world_bounds_picking_chooses_the_nearest_and_has_stable_ties() {
	let ray = Ray {
		origin: Vec3::new(0.0, 0.0, 8.0),
		direction: -Vec3::Z,
		max_distance: 100.0,
	};
	let targets = vec![
		PickTarget {
			key: "far".into(),
			bounds: bounds([-1.0, -1.0, -3.0], [1.0, 1.0, -2.0]),
		},
		PickTarget {
			key: "near".into(),
			bounds: bounds([-1.0, -1.0, 1.0], [1.0, 1.0, 2.0]),
		},
		PickTarget {
			key: "tie".into(),
			bounds: bounds([-1.0, -1.0, 1.0], [1.0, 1.0, 2.0]),
		},
	];
	let hit = pick_nearest(ray, &targets).unwrap();
	assert_eq!(hit.key, "near");
	assert_eq!(hit.distance, 6.0);
	approx(hit.position, Vec3::new(0.0, 0.0, 2.0));
	approx(hit.normal, Vec3::Z);
}

#[test]
fn placement_uses_surface_height_and_snaps_only_its_tangent_coordinates() {
	let ray = Ray {
		origin: Vec3::new(0.37, 5.0, 0.61),
		direction: -Vec3::Y,
		max_distance: 100.0,
	};
	let targets = [PickTarget {
		key: "platform".into(),
		bounds: bounds([-2.0, 0.0, -2.0], [2.0, 1.2, 2.0]),
	}];
	let point = placement_point(ray, &targets, ViewportPlane::Xz, Some(1.0)).unwrap();
	approx(point, Vec3::new(0.0, 1.2, 1.0));
	approx(
		placement_point(ray, &[], ViewportPlane::Xz, None).unwrap(),
		Vec3::new(0.37, 0.0, 0.61),
	);
}

#[test]
fn parallel_or_invalid_plane_rays_fail_quietly() {
	let ray = Ray {
		origin: Vec3::Y,
		direction: Vec3::X,
		max_distance: 100.0,
	};
	assert!(placement_point(ray, &[], ViewportPlane::Xz, None).is_none());
	let invalid = Ray {
		origin: Vec3::splat(f32::NAN),
		..ray
	};
	assert!(invalid.plane(Vec3::ZERO, Vec3::Y).is_none());
}

#[test]
fn adaptive_grid_uses_tens_of_pixels_and_does_not_change_snap_settings() {
	let settings = ViewportSettingsDto::default();
	for units in [0.0001, 0.002, 0.02, 0.1, 0.5, 5.0] {
		let spacing = adaptive_grid_spacing(1.0, units);
		assert!(spacing / units >= 31.9 && spacing / units <= 160.1);
	}
	assert_eq!(settings.move_snap, 1.0);
	assert_eq!(settings.grid_spacing, 1.0);
	assert_eq!(adaptive_grid_spacing(f32::NAN, 1.0), 1.0);
}

#[test]
fn gizmo_projected_size_stays_constant_across_dolly_and_orthographic_zoom() {
	for projection in [
		ViewportProjection::Perspective,
		ViewportProjection::Orthographic,
	] {
		let mut camera = camera();
		let measure = |camera: &EditorCamera| {
			let size = camera.gizmo_size(Vec3::ZERO, [1200, 800], projection, 90.0);
			let center = camera.project(Vec3::ZERO, [1200, 800], projection).unwrap();
			camera
				.project(Vec3::X * size, [1200, 800], projection)
				.unwrap()
				.distance(center)
		};
		let before = measure(&camera);
		camera.zoom(3.0);
		let after = measure(&camera);
		assert!((before - after).abs() < 0.02, "{before} != {after}");
	}
}

#[test]
fn handles_hit_visible_axes_and_uniform_scale_center() {
	let camera = camera();
	let extent = [1200, 800];
	let projection = ViewportProjection::Perspective;
	let gizmo = Gizmo::new(Vec3::ZERO, Quat::IDENTITY, 2.0);
	let point = camera.project(Vec3::X * 1.8, extent, projection).unwrap();
	assert_eq!(
		gizmo.hit_test(
			&camera,
			extent,
			projection,
			ViewportTool::Move,
			pixel(point),
			9.0
		),
		Some(Handle::Axis(0))
	);
	let center = camera.project(Vec3::ZERO, extent, projection).unwrap();
	assert_eq!(
		gizmo.hit_test(
			&camera,
			extent,
			projection,
			ViewportTool::Scale,
			pixel(center),
			9.0
		),
		Some(Handle::UniformScale)
	);
	assert!(gizmo
		.hit_test(
			&camera,
			extent,
			projection,
			ViewportTool::Hand,
			pixel(center),
			9.0
		)
		.is_none());
}

#[test]
fn local_gizmo_basis_rotates_the_handle_geometry() {
	let gizmo = Gizmo::new(
		Vec3::ZERO,
		Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
		2.0,
	);
	approx(gizmo.axes[0], -Vec3::Z);
	let line = gizmo
		.segments(ViewportTool::Move)
		.into_iter()
		.find(|s| s.handle == Handle::Axis(0))
		.unwrap();
	approx(line.b, -Vec3::Z * 2.0);
}

#[test]
fn repeated_move_previews_derive_from_the_original_instead_of_accumulating() {
	let (mut drag, start, axis) = axis_drag(
		ViewportSettingsDto::default(),
		ViewportTool::Move,
		vec![identity_target("a")],
	);
	let point = pixel(start + axis * 0.6);
	let first = drag.preview(point).unwrap();
	let second = drag.preview(point).unwrap();
	assert_eq!(first[0].transform, second[0].transform);
	approx(Vec3::from_array(first[0].transform.position), Vec3::X * 1.2);
}

#[test]
fn pressing_a_handle_without_moving_does_not_canonicalize_authored_values() {
	let (mut drag, start, axis) = axis_drag(
		ViewportSettingsDto::default(),
		ViewportTool::Scale,
		vec![TransformTarget {
			key: "a".into(),
			world: Mat4::from_rotation_y(7.0),
		}],
	);
	assert!(drag.preview(pixel(start)).unwrap().is_empty());
	// A perpendicular movement also produces an identity scale operation.
	let perpendicular = Vec2::new(-axis.y, axis.x).normalize() * 10.0;
	assert!(drag
		.preview(pixel(start + perpendicular))
		.unwrap()
		.is_empty());
}

#[test]
fn move_snapping_uses_total_constrained_displacement() {
	let settings = ViewportSettingsDto {
		snap: true,
		move_snap: 1.0,
		..Default::default()
	};
	let (mut drag, start, axis) =
		axis_drag(settings, ViewportTool::Move, vec![identity_target("a")]);
	let result = drag.preview(pixel(start + axis * 0.4)).unwrap();
	approx(Vec3::from_array(result[0].transform.position), Vec3::X);
}

#[test]
fn world_translation_preserves_a_rotated_parent_derived_world_pose() {
	let parent =
		Mat4::from_rotation_translation(Quat::from_rotation_y(0.7), Vec3::new(4.0, 2.0, 1.0));
	let before = parent * Mat4::from_translation(Vec3::new(2.0, 0.0, 0.0));
	let (mut drag, start, axis) = axis_drag(
		ViewportSettingsDto::default(),
		ViewportTool::Move,
		vec![TransformTarget {
			key: "child".into(),
			world: before,
		}],
	);
	let result = drag.preview(pixel(start + axis * 0.5)).unwrap();
	let expected = Mat4::from_translation(Vec3::X) * before;
	let actual = transform_matrix(&result[0]);
	assert!(actual.abs_diff_eq(expected, 0.001));
}

#[test]
fn move_delta_supports_a_sheared_world_baseline_without_decomposing_it() {
	let parent = Mat4::from_scale(Vec3::new(2.0, 1.0, 1.0));
	let local =
		Mat4::from_rotation_translation(Quat::from_rotation_y(0.7), Vec3::new(1.0, 0.0, 0.0));
	let world = parent * local;
	assert!(runtime::scene_data::TransformData::from_matrix(world).is_err());
	let (drag, start, axis) = axis_drag(
		ViewportSettingsDto::default(),
		ViewportTool::Move,
		vec![TransformTarget {
			key: "child".into(),
			world,
		}],
	);
	let delta = drag.translation_delta(pixel(start + axis * 0.5)).unwrap();
	approx(delta, Vec3::X);
	let new_local_position = local.w_axis.truncate() + parent.inverse().transform_vector3(delta);
	approx(
		parent.transform_point3(new_local_position),
		world.w_axis.truncate() + Vec3::X,
	);
	approx(
		drag.translation_delta(pixel(start + axis * 0.5)).unwrap(),
		delta,
	);
}

#[test]
fn group_move_keeps_relative_world_offsets() {
	let (mut drag, start, axis) = axis_drag(
		ViewportSettingsDto::default(),
		ViewportTool::Move,
		vec![
			TransformTarget {
				key: "a".into(),
				world: Mat4::from_translation(Vec3::new(1.0, 0.0, 0.0)),
			},
			TransformTarget {
				key: "b".into(),
				world: Mat4::from_translation(Vec3::new(3.0, 0.0, 0.0)),
			},
		],
	);
	let result = drag.preview(pixel(start + axis * 0.5)).unwrap();
	approx(
		Vec3::from_array(result[1].transform.position)
			- Vec3::from_array(result[0].transform.position),
		Vec3::X * 2.0,
	);
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

#[test]
fn axis_scale_keeps_mirrored_transform_sign_and_rejects_unrepresentable_shear() {
	let mirrored = Mat4::from_scale(Vec3::new(-2.0, 1.0, 1.0));
	let (mut drag, start, axis) = axis_drag(
		ViewportSettingsDto::default(),
		ViewportTool::Scale,
		vec![TransformTarget {
			key: "mirror".into(),
			world: mirrored,
		}],
	);
	let result = drag.preview(pixel(start + axis)).unwrap();
	approx(
		transform_matrix(&result[0]).transform_vector3(Vec3::X),
		Vec3::X * -4.0,
	);
	let rotated = Mat4::from_quat(Quat::from_rotation_y(0.6));
	let (mut drag, start, axis) = axis_drag(
		ViewportSettingsDto::default(),
		ViewportTool::Scale,
		vec![TransformTarget {
			key: "rotated".into(),
			world: rotated,
		}],
	);
	assert!(drag
		.preview(pixel(start + axis))
		.unwrap_err()
		.contains("shear"));
}

#[test]
fn rotate_and_scale_accept_invertible_small_world_transforms() {
	let small = Mat4::from_scale(Vec3::splat(0.0001));
	assert!(small.determinant() > 0.0 && small.determinant() < 1.0e-10);
	let (mut drag, start, axis) = axis_drag(
		ViewportSettingsDto::default(),
		ViewportTool::Scale,
		vec![TransformTarget {
			key: "small".into(),
			world: small,
		}],
	);
	let result = drag.preview(pixel(start + axis * 0.5)).unwrap();
	let after = transform_matrix(&result[0]);
	assert!((after.x_axis.x - 0.00015).abs() < 1.0e-8);
	assert!((after.y_axis.y - 0.0001).abs() < 1.0e-8);
	let camera = camera();
	let extent = [1200, 800];
	let projection = ViewportProjection::Perspective;
	let start = camera.project(Vec3::X * 2.0, extent, projection).unwrap();
	let finish = camera.project(Vec3::Z * 2.0, extent, projection).unwrap();
	let mut rotation = GizmoDrag::begin(
		&camera,
		extent,
		projection,
		pixel(start),
		Gizmo::new(Vec3::ZERO, Quat::IDENTITY, 2.0),
		ViewportTool::Rotate,
		Handle::Rotate(1),
		&ViewportSettingsDto::default(),
		vec![TransformTarget {
			key: "small".into(),
			world: small,
		}],
	)
	.unwrap();
	let result = rotation.preview(pixel(finish)).unwrap();
	let vector = transform_matrix(&result[0]).transform_vector3(Vec3::X);
	assert!(vector.distance(Vec3::Z * 0.0001) < 1.0e-7);
}

#[test]
fn uniform_scale_acts_about_the_shared_pivot_and_stays_positive() {
	let camera = camera();
	let extent = [1200, 800];
	let projection = ViewportProjection::Perspective;
	let center = camera.project(Vec3::ZERO, extent, projection).unwrap();
	let mut drag = GizmoDrag::begin(
		&camera,
		extent,
		projection,
		pixel(center),
		Gizmo::new(Vec3::ZERO, Quat::IDENTITY, 2.0),
		ViewportTool::Scale,
		Handle::UniformScale,
		&ViewportSettingsDto::default(),
		vec![
			TransformTarget {
				key: "a".into(),
				world: Mat4::from_translation(-Vec3::X),
			},
			TransformTarget {
				key: "b".into(),
				world: Mat4::from_translation(Vec3::X),
			},
		],
	)
	.unwrap();
	let result = drag.preview(pixel(center + Vec2::new(120.0, 0.0))).unwrap();
	approx(
		Vec3::from_array(result[0].transform.position),
		-Vec3::X * 2.0,
	);
	approx(
		Vec3::from_array(result[1].transform.position),
		Vec3::X * 2.0,
	);
	let result = drag
		.preview(pixel(center - Vec2::new(10000.0, 0.0)))
		.unwrap();
	assert!(result[0].transform.scale.iter().all(|v| *v > 0.0));
}

#[test]
fn captured_screen_drag_can_leave_the_viewport_and_bad_axis_is_rejected() {
	let camera = camera();
	let extent = [1200, 800];
	let projection = ViewportProjection::Perspective;
	let center = camera.project(Vec3::ZERO, extent, projection).unwrap();
	let gizmo = Gizmo::new(Vec3::ZERO, Quat::IDENTITY, 2.0);
	let mut drag = GizmoDrag::begin(
		&camera,
		extent,
		projection,
		pixel(center),
		gizmo,
		ViewportTool::Move,
		Handle::Screen,
		&ViewportSettingsDto::default(),
		vec![identity_target("a")],
	)
	.unwrap();
	assert!(drag.preview([-100.0, 400.0]).is_ok());
	assert!(drag.preview([f64::NAN, 400.0]).is_err());
	assert!(GizmoDrag::begin(
		&camera,
		extent,
		projection,
		pixel(center),
		gizmo,
		ViewportTool::Move,
		Handle::Axis(4),
		&ViewportSettingsDto::default(),
		vec![identity_target("a")]
	)
	.is_err());
}
