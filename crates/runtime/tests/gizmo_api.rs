use glam::{EulerRot, Quat, Vec3};
use runtime::{
	Camera, Transform,
	assets::Model,
	gizmo::{self, GizmoDrag, GizmoMode},
	model_instance::ModelInstance,
	scene::{EntityId, Scene},
};
use std::sync::Arc;

fn entity() -> EntityId {
	Scene::default().spawn(ModelInstance::new(
		Arc::new(Model::new(vec![])),
		Transform::default(),
	))
}
fn close(a: f32, b: f32) {
	assert!((a - b).abs() < 0.001, "{a} != {b}");
}

#[test]
fn projection_respects_vulkan_clip_limits_and_screen_center() {
	let camera = Camera::default();
	let center = gizmo::project(&camera, [800, 600], Vec3::ZERO).unwrap();
	close(center.x, 400.0);
	close(center.y, 300.0);
	assert!(gizmo::project(&camera, [0, 600], Vec3::ZERO).is_none());
	assert!(gizmo::project(&camera, [800, 600], Vec3::new(0.0, 0.0, 4.0)).is_none());
	assert!(gizmo::project(&camera, [800, 600], Vec3::new(0.0, 0.0, -200.0)).is_none());
	assert!(gizmo::project(&camera, [800, 600], Vec3::splat(f32::NAN)).is_none());
}

#[test]
fn parented_translation_and_scale_use_world_handles_and_local_results() {
	let camera = Camera::default();
	let parent = Transform {
		rotation: [0.0, 0.0, std::f32::consts::FRAC_PI_2],
		scale: [2.0, 3.0, 1.0],
		..Default::default()
	}
	.matrix();
	let pose = gizmo::GizmoPose {
		local: Transform::default(),
		parent,
	};
	let drag = GizmoDrag::begin_parented(
		entity(),
		pose,
		&camera,
		[800, 600],
		[400.0, 300.0],
		GizmoMode::Translate,
		0,
	)
	.unwrap();
	let after = drag.preview([490.0, 300.0]).unwrap();
	let world = parent.transform_point3(Vec3::from_array(after.position));
	close(world.x, gizmo::length(&camera, [800, 600], [0.0; 3]));
	close(world.y, 0.0);
	close(after.position[0], 0.0);
	assert!(after.position[1] < 0.0);
	let drag = GizmoDrag::begin_parented(
		entity(),
		pose,
		&camera,
		[800, 600],
		[400.0, 300.0],
		GizmoMode::Scale,
		0,
	)
	.unwrap();
	let after = drag.preview([400.0, 210.0]).unwrap();
	close(after.scale[0], 2.0);
	assert_eq!(after.position, pose.local.position);
	assert_eq!(after.rotation, pose.local.rotation);
}

#[test]
fn parent_translation_places_rendered_and_pickable_handles_at_world_position() {
	let camera = Camera::default();
	let pose = gizmo::GizmoPose {
		local: Transform::default(),
		parent: glam::Mat4::from_translation(Vec3::new(0.5, 0.0, 0.0)),
	};
	let segments = gizmo::segments_parented(&camera, [800, 600], pose, GizmoMode::Translate);
	let x = segments.iter().find(|segment| segment.axis == 0).unwrap();
	assert!(x.a.x > 0.5);
	let midpoint = gizmo::project(&camera, [800, 600], (x.a + x.b) * 0.5).unwrap();
	assert_eq!(
		gizmo::hit_test_parented(
			&camera,
			[800, 600],
			pose,
			GizmoMode::Translate,
			[midpoint.x as f64, midpoint.y as f64]
		),
		Some(0)
	);
}

#[test]
fn rotation_hit_test_preserves_parent_offset_and_clips_unavailable_handles() {
	let camera = Camera::default();
	let pose = gizmo::GizmoPose {
		local: Transform::default(),
		parent: glam::Mat4::from_translation(Vec3::new(0.5, 0.0, 0.0)),
	};
	// The world-Z ring has a 90-pixel radius and the parent's X translation
	// moves its center to approximately (486.6, 300).
	let pixel = [550.0, 237.0];
	assert_eq!(
		gizmo::hit_test_parented(&camera, [800, 600], pose, GizmoMode::Rotate, pixel),
		Some(2)
	);
	for extent in [[0, 600], [800, 0]] {
		assert_eq!(
			gizmo::hit_test_parented(&camera, extent, pose, GizmoMode::Rotate, pixel),
			None
		);
		assert!(gizmo::segments_parented(&camera, extent, pose, GizmoMode::Rotate).is_empty());
	}
	let behind_camera = gizmo::GizmoPose {
		parent: glam::Mat4::from_translation(Vec3::new(0.5, 0.0, 4.0)),
		..pose
	};
	assert_eq!(
		gizmo::hit_test_parented(&camera, [800, 600], behind_camera, GizmoMode::Rotate, pixel),
		None
	);
}

#[test]
fn parented_world_rotation_converts_back_to_local_and_rejects_shear() {
	let camera = Camera::default();
	let before = Transform {
		rotation: [0.2, 0.3, 0.4],
		..Default::default()
	};
	let parent = glam::Mat4::from_scale_rotation_translation(
		Vec3::splat(2.0),
		Quat::from_rotation_x(0.3),
		Vec3::ZERO,
	);
	let drag = GizmoDrag::begin_parented(
		entity(),
		gizmo::GizmoPose {
			local: before,
			parent,
		},
		&camera,
		[800, 600],
		[490.0, 300.0],
		GizmoMode::Rotate,
		2,
	)
	.unwrap();
	let after = drag.preview([400.0, 210.0]).unwrap();
	let expected =
		glam::Mat4::from_rotation_z(std::f32::consts::FRAC_PI_2) * parent * before.matrix();
	for (actual, expected) in (parent * after.matrix())
		.to_cols_array()
		.into_iter()
		.zip(expected.to_cols_array())
	{
		close(actual, expected);
	}
	let pose = gizmo::GizmoPose {
		local: Transform::default(),
		parent: glam::Mat4::from_scale(Vec3::new(2.0, 1.0, 1.0)),
	};
	let drag = GizmoDrag::begin_parented(
		entity(),
		pose,
		&camera,
		[800, 600],
		[490.0, 300.0],
		GizmoMode::Rotate,
		2,
	)
	.unwrap();
	assert!(drag.preview([460.0, 240.0]).unwrap_err().contains("shear"));
	let pose = gizmo::GizmoPose {
		parent: glam::Mat4::from_scale(Vec3::new(0.0, 1.0, 1.0)),
		..pose
	};
	assert!(
		GizmoDrag::begin_parented(
			entity(),
			pose,
			&camera,
			[800, 600],
			[400.0, 300.0],
			GizmoMode::Translate,
			0
		)
		.is_err()
	);
}

#[test]
fn translate_uses_world_axes_and_drag_preview_is_absolute_from_start() {
	let before = Transform {
		rotation: [0.0, 0.0, 0.7],
		..Transform::default()
	};
	let camera = Camera::default();
	let drag = GizmoDrag::begin(
		entity(),
		before,
		&camera,
		[800, 600],
		[400.0, 300.0],
		GizmoMode::Translate,
		0,
	)
	.unwrap();
	let middle = drag.preview([445.0, 300.0]).unwrap();
	let after = drag.preview([490.0, 300.0]).unwrap();
	close(after.position[0], middle.position[0] * 2.0);
	assert_eq!(after.position[1..], [0.0, 0.0]);
	assert_eq!(after.rotation, before.rotation);
	assert_eq!(after.scale, before.scale);
	close(
		gizmo::project(&camera, [800, 600], Vec3::from_array(after.position))
			.unwrap()
			.x,
		490.0,
	);
	assert_eq!(drag.preview([400.0, 300.0]).unwrap(), before);
}

#[test]
fn scale_follows_rotated_local_axis_and_preserves_other_components_and_sign() {
	let before = Transform {
		rotation: [0.0, 0.0, std::f32::consts::FRAC_PI_2],
		scale: [-2.0, 3.0, 4.0],
		..Transform::default()
	};
	let drag = GizmoDrag::begin(
		entity(),
		before,
		&Camera::default(),
		[800, 600],
		[400.0, 300.0],
		GizmoMode::Scale,
		0,
	)
	.unwrap();
	let after = drag.preview([400.0, 210.0]).unwrap();
	close(after.scale[0], -4.0);
	assert_eq!(&after.scale[1..], &before.scale[1..]);
	assert_eq!(after.position, before.position);
	assert_eq!(after.rotation, before.rotation);
	let clamped = drag.preview([400.0, 1000.0]).unwrap();
	assert!(clamped.scale[0].is_finite() && clamped.scale[0] < 0.0);
}

#[test]
fn rotation_composes_about_world_axis_using_quaternions() {
	let before = Transform {
		rotation: [0.2, 0.3, 0.4],
		..Transform::default()
	};
	let drag = GizmoDrag::begin(
		entity(),
		before,
		&Camera::default(),
		[800, 600],
		[490.0, 300.0],
		GizmoMode::Rotate,
		2,
	)
	.unwrap();
	let after = drag.preview([400.0, 210.0]).unwrap();
	let actual = Quat::from_euler(
		EulerRot::XYZ,
		after.rotation[0],
		after.rotation[1],
		after.rotation[2],
	);
	let initial = Quat::from_euler(
		EulerRot::XYZ,
		before.rotation[0],
		before.rotation[1],
		before.rotation[2],
	);
	let expected = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2) * initial;
	close(actual.dot(expected).abs(), 1.0);
	assert_eq!(after.position, before.position);
	assert_eq!(after.scale, before.scale);
}

#[test]
fn handles_hit_test_screen_segments_and_skip_degenerate_drag_axes() {
	let camera = Camera::default();
	let transform = Transform::default();
	assert_eq!(
		gizmo::segments(&camera, [800, 600], transform, GizmoMode::Translate).len(),
		3
	);
	assert_eq!(
		gizmo::segments(&camera, [800, 600], transform, GizmoMode::Rotate).len(),
		144
	);
	assert_eq!(
		gizmo::hit_test(
			&camera,
			[800, 600],
			transform,
			GizmoMode::Translate,
			[450.0, 300.0]
		),
		Some(0)
	);
	assert_eq!(
		gizmo::hit_test(
			&camera,
			[800, 600],
			transform,
			GizmoMode::Translate,
			[400.0, 250.0]
		),
		Some(1)
	);
	assert_eq!(
		gizmo::hit_test(
			&camera,
			[800, 600],
			transform,
			GizmoMode::Translate,
			[0.0, 0.0]
		),
		None
	);
	assert!(
		GizmoDrag::begin(
			entity(),
			transform,
			&camera,
			[800, 600],
			[400.0, 300.0],
			GizmoMode::Translate,
			2
		)
		.is_err()
	);
	assert!(
		GizmoDrag::begin(
			entity(),
			transform,
			&camera,
			[800, 600],
			[400.0, 300.0],
			GizmoMode::Rotate,
			0
		)
		.is_err()
	);
	assert!(
		GizmoDrag::begin(
			entity(),
			transform,
			&camera,
			[800, 600],
			[400.0, 300.0],
			GizmoMode::Translate,
			3
		)
		.is_err()
	);
	let drag = GizmoDrag::begin(
		entity(),
		transform,
		&camera,
		[800, 600],
		[400.0, 300.0],
		GizmoMode::Translate,
		0,
	)
	.unwrap();
	assert!(drag.preview([f64::NAN, 300.0]).is_err());
}

#[test]
fn drag_begin_rejects_nonfinite_cursor_coordinates() {
	for (mode, axis) in [
		(GizmoMode::Translate, 0),
		(GizmoMode::Scale, 0),
		(GizmoMode::Rotate, 2),
	] {
		for pixel in [
			[f64::NAN, 300.0],
			[400.0, f64::INFINITY],
			[f64::NEG_INFINITY, 300.0],
		] {
			assert!(
				GizmoDrag::begin(
					entity(),
					Transform::default(),
					&Camera::default(),
					[800, 600],
					pixel,
					mode,
					axis
				)
				.is_err(),
				"{mode:?} accepted {pixel:?}"
			);
		}
	}
}

#[test]
fn every_drag_mode_validates_viewport_camera_and_transform_before_starting() {
	for (mode, axis) in [
		(GizmoMode::Translate, 0),
		(GizmoMode::Scale, 0),
		(GizmoMode::Rotate, 2),
	] {
		let begin = |camera: &Camera, extent, pixel, transform| {
			GizmoDrag::begin(entity(), transform, camera, extent, pixel, mode, axis)
		};
		let camera = Camera::default();
		let before = Transform::default();
		let pixel = [490.0, 300.0];
		assert!(
			begin(&camera, [800, 600], pixel, before).is_ok(),
			"{mode:?} valid start failed"
		);
		for (extent, pixel) in [
			([0, 600], pixel),
			([800, 0], pixel),
			([800, 600], [-1.0, 300.0]),
			([800, 600], [800.0, 300.0]),
			([800, 600], [490.0, 600.0]),
		] {
			assert!(
				begin(&camera, extent, pixel, before).is_err(),
				"{mode:?} accepted invalid viewport/pixel"
			);
		}
		for invalid in [
			Camera {
				near: 0.0,
				..camera.clone()
			},
			Camera {
				far: camera.near,
				..camera.clone()
			},
			Camera {
				vertical_fov: std::f32::consts::PI,
				..camera.clone()
			},
			Camera {
				yaw: f32::NAN,
				..camera.clone()
			},
		] {
			assert!(
				begin(&invalid, [800, 600], pixel, before).is_err(),
				"{mode:?} accepted invalid camera"
			);
		}
		let invalid = Transform {
			scale: [f32::NAN, 1.0, 1.0],
			..before
		};
		assert!(begin(&camera, [800, 600], pixel, invalid).is_err());
	}
}
