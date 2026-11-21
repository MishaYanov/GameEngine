use super::*;

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
