use glam::{Mat4, Quat, Vec3};
use runtime::{
	Bounds3, Camera, Transform,
	assets::Model,
	model_instance::ModelInstance,
	picking::{CameraRay, pick},
	scene::{EntityId, Scene},
};
use std::sync::Arc;

fn cube() -> Bounds3 {
	Bounds3::new([-1.0; 3], [1.0; 3]).unwrap()
}
fn add(scene: &mut Scene, transform: Transform, bounds: Option<Bounds3>) -> EntityId {
	scene.spawn(ModelInstance::new(
		Arc::new(Model::with_bounds(vec![], bounds)),
		transform,
	))
}
fn ray() -> CameraRay {
	CameraRay {
		origin: Vec3::new(0.0, 0.0, 5.0),
		direction: -Vec3::Z,
		max_distance: 20.0,
	}
}
fn close(a: f32, b: f32) {
	assert!((a - b).abs() < 0.001, "expected {b}, got {a}");
}

#[test]
fn bounds_reject_unordered_nonfinite_points_and_support_empty_or_flat_inputs() {
	assert_eq!(Bounds3::from_points([]).unwrap(), None);
	assert!(Bounds3::new([1.0; 3], [0.0; 3]).is_err());
	for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
		assert!(Bounds3::from_points([[0.0; 3], [invalid, 1.0, 2.0]]).is_err());
	}
	let bounds = Bounds3::from_points([[2.0, -3.0, 1.0], [-1.0, 4.0, 1.0]])
		.unwrap()
		.unwrap();
	assert_eq!(bounds.min(), [-1.0, -3.0, 1.0]);
	assert_eq!(bounds.max(), [2.0, 4.0, 1.0]);
}

#[test]
fn transformed_bounds_include_all_corners_under_rotation_and_negative_nonuniform_scale() {
	let matrix = Mat4::from_scale_rotation_translation(
		Vec3::new(-2.0, 3.0, 0.5),
		Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
		Vec3::new(-3.0, 4.0, 5.0),
	);
	let bounds = cube().transformed(matrix).unwrap();
	for (actual, expected) in bounds.min().into_iter().zip([-6.0, 2.0, 4.5]) {
		close(actual, expected);
	}
	for (actual, expected) in bounds.max().into_iter().zip([0.0, 6.0, 5.5]) {
		close(actual, expected);
	}
	assert!(
		cube()
			.transformed(Mat4::from_scale(Vec3::splat(f32::NAN)))
			.is_err()
	);
}

#[test]
fn slab_intersection_handles_parallel_inside_backward_grazing_and_clip_distances() {
	let bounds = cube();
	assert_eq!(
		bounds.ray_distance(Vec3::new(0.0, 0.0, 5.0), -Vec3::Z, 10.0),
		Some(4.0)
	);
	assert_eq!(
		bounds.ray_distance(Vec3::new(2.0, 0.0, 5.0), -Vec3::Z, 10.0),
		None
	);
	assert_eq!(bounds.ray_distance(Vec3::ZERO, Vec3::X, 10.0), Some(0.0));
	assert_eq!(
		bounds.ray_distance(Vec3::new(0.0, 0.0, 5.0), Vec3::Z, 10.0),
		None
	);
	assert_eq!(
		bounds.ray_distance(Vec3::new(1.0, 1.0, 5.0), -Vec3::Z, 4.0),
		Some(4.0)
	);
	assert_eq!(
		bounds.ray_distance(Vec3::new(0.0, 0.0, 5.0), -Vec3::Z, 3.99),
		None
	);
	// A scaled direction must retain its original t parameter, not be normalized.
	assert_eq!(
		bounds.ray_distance(Vec3::new(0.0, 0.0, 5.0), -Vec3::Z * 2.0, 10.0),
		Some(2.0)
	);
	assert_eq!(
		bounds.ray_distance(Vec3::splat(f32::NAN), Vec3::Z, 10.0),
		None
	);
	assert_eq!(
		bounds.ray_distance(Vec3::ZERO, Vec3::splat(f32::INFINITY), 10.0),
		None
	);
	assert_eq!(bounds.ray_distance(Vec3::ZERO, Vec3::Z, -1.0), None);
}

#[test]
fn center_camera_ray_starts_at_near_plane_and_ends_at_far_plane() {
	let camera = Camera {
		near: 0.5,
		far: 10.0,
		..Camera::default()
	};
	let ray = CameraRay::from_screen(&camera, [400.0, 300.0], [800, 600]).unwrap();
	close(ray.origin.z, 2.5);
	close(ray.direction.length(), 1.0);
	close(ray.direction.dot(-Vec3::Z), 1.0);
	close(ray.max_distance, 9.5);
	close((ray.origin + ray.direction * ray.max_distance).z, -7.0);
}

#[test]
fn camera_ray_respects_screen_axes_aspect_and_camera_rotation() {
	let camera = Camera::default();
	let left = CameraRay::from_screen(&camera, [0.0, 300.0], [800, 600]).unwrap();
	let right = CameraRay::from_screen(&camera, [799.0, 300.0], [800, 600]).unwrap();
	let top = CameraRay::from_screen(&camera, [400.0, 0.0], [800, 600]).unwrap();
	let bottom = CameraRay::from_screen(&camera, [400.0, 599.0], [800, 600]).unwrap();
	assert!(left.direction.x < 0.0 && right.direction.x > 0.0);
	assert!(top.direction.y > 0.0 && bottom.direction.y < 0.0);
	let camera = Camera {
		yaw: std::f32::consts::FRAC_PI_2,
		..camera
	};
	let ray = CameraRay::from_screen(&camera, [400.0, 300.0], [800, 600]).unwrap();
	close(ray.direction.dot(Vec3::X), 1.0);
}

#[test]
fn screen_ray_rejects_empty_viewports_outside_pixels_and_invalid_camera_values() {
	let camera = Camera::default();
	for (pixel, extent) in [
		([0.0, 0.0], [0, 600]),
		([-1.0, 2.0], [800, 600]),
		([800.0, 2.0], [800, 600]),
		([2.0, 600.0], [800, 600]),
		([f64::NAN, 2.0], [800, 600]),
	] {
		assert!(CameraRay::from_screen(&camera, pixel, extent).is_err());
	}
	for field in 0..8 {
		let mut invalid = Camera::default();
		match field {
			0 => invalid.position[0] = f32::NAN,
			1 => invalid.yaw = f32::INFINITY,
			2 => invalid.pitch = f32::NAN,
			3 => invalid.near = 0.0,
			4 => invalid.far = invalid.near,
			5 => invalid.vertical_fov = 0.0,
			6 => invalid.vertical_fov = std::f32::consts::PI,
			_ => invalid.far = f32::INFINITY,
		}
		assert!(CameraRay::from_screen(&invalid, [400.0, 300.0], [800, 600]).is_err());
	}
}

#[test]
fn picking_chooses_nearest_hit_and_keeps_scene_order_for_exact_ties() {
	let mut scene = Scene::default();
	add(
		&mut scene,
		Transform {
			position: [0.0, 0.0, -4.0],
			..Transform::default()
		},
		Some(cube()),
	);
	let near = add(&mut scene, Transform::default(), Some(cube()));
	let tie = add(&mut scene, Transform::default(), Some(cube()));
	assert_eq!(pick(&scene, ray()), Some(near));
	scene.remove(near).unwrap();
	assert_eq!(pick(&scene, ray()), Some(tie));
}

#[test]
fn picking_handles_rotated_negative_nonuniform_scale_and_skips_degenerate_or_unbounded_models() {
	let mut scene = Scene::default();
	add(
		&mut scene,
		Transform {
			scale: [0.0, 1.0, 1.0],
			..Transform::default()
		},
		Some(cube()),
	);
	add(&mut scene, Transform::default(), None);
	let transformed = add(
		&mut scene,
		Transform {
			rotation: [0.0, 0.0, 0.7],
			scale: [-2.0, 0.25, 2.0],
			..Transform::default()
		},
		Some(cube()),
	);
	assert_eq!(pick(&scene, ray()), Some(transformed));
	// z scale makes the first hit distance three, not four or six.
	assert_eq!(
		pick(
			&scene,
			CameraRay {
				max_distance: 2.99,
				..ray()
			}
		),
		None
	);
	assert_eq!(
		pick(
			&scene,
			CameraRay {
				max_distance: 3.0,
				..ray()
			}
		),
		Some(transformed)
	);
	assert_eq!(
		pick(
			&scene,
			CameraRay {
				origin: Vec3::ZERO,
				..ray()
			}
		),
		Some(transformed)
	);
}

#[test]
fn picking_excludes_geometry_outside_camera_near_and_far_planes() {
	let camera = Camera {
		position: [0.0; 3],
		near: 1.0,
		far: 10.0,
		..Camera::default()
	};
	let ray = CameraRay::from_screen(&camera, [400.0, 300.0], [800, 600]).unwrap();
	let tiny = Bounds3::new([-0.1; 3], [0.1; 3]).unwrap();
	let mut scene = Scene::default();
	add(
		&mut scene,
		Transform {
			position: [0.0, 0.0, -0.3],
			..Transform::default()
		},
		Some(tiny),
	);
	add(
		&mut scene,
		Transform {
			position: [0.0, 0.0, -12.0],
			..Transform::default()
		},
		Some(tiny),
	);
	assert_eq!(pick(&scene, ray), None);
	let visible = add(
		&mut scene,
		Transform {
			position: [0.0, 0.0, -5.0],
			..Transform::default()
		},
		Some(tiny),
	);
	assert_eq!(pick(&scene, ray), Some(visible));
}

#[test]
fn nearest_hit_compares_world_distance_across_different_model_scales() {
	let mut scene = Scene::default();
	add(
		&mut scene,
		Transform {
			position: [0.0, 0.0, -20.0],
			scale: [1.0, 1.0, 10.0],
			..Transform::default()
		},
		Some(cube()),
	);
	let near = add(
		&mut scene,
		Transform {
			scale: [-2.0, 3.0, 0.1],
			..Transform::default()
		},
		Some(cube()),
	);
	assert_eq!(pick(&scene, ray()), Some(near));
	let asymmetric = Bounds3::new([0.0, -0.5, -1.0], [2.0, 0.5, 1.0]).unwrap();
	let mut scene = Scene::default();
	let mirrored = add(
		&mut scene,
		Transform {
			position: [3.0, 0.0, 0.0],
			scale: [-1.0, 2.0, 0.5],
			..Transform::default()
		},
		Some(asymmetric),
	);
	assert_eq!(
		pick(
			&scene,
			CameraRay {
				origin: Vec3::new(2.0, 0.0, 5.0),
				..ray()
			}
		),
		Some(mirrored)
	);
	assert_eq!(
		pick(
			&scene,
			CameraRay {
				origin: Vec3::new(4.0, 0.0, 5.0),
				..ray()
			}
		),
		None
	);
}
