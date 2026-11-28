use super::*;

#[test]
fn world_bounds_picking_chooses_the_nearest_and_has_stable_ties() {
	let ray = Ray {
		origin: Vec3::new(0.0, 0.0, 8.0),
		direction: -Vec3::Z,
		max_distance: 100.0,
	};
	let targets = vec![
		PickTarget::new("far".into(), bounds([-1.0, -1.0, -3.0], [1.0, 1.0, -2.0])),
		PickTarget::new("near".into(), bounds([-1.0, -1.0, 1.0], [1.0, 1.0, 2.0])),
		PickTarget::new("tie".into(), bounds([-1.0, -1.0, 1.0], [1.0, 1.0, 2.0])),
	];
	assert_eq!(pick_nearest(ray, &targets), Some("near"));
	approx(
		placement_point(ray, &targets, ViewportPlane::Xz, None).unwrap(),
		Vec3::new(0.0, 0.0, 2.0),
	);
}

#[test]
fn placement_uses_surface_height_and_snaps_only_its_tangent_coordinates() {
	let ray = Ray {
		origin: Vec3::new(0.37, 5.0, 0.61),
		direction: -Vec3::Y,
		max_distance: 100.0,
	};
	let targets = [PickTarget::new("platform".into(), bounds([-2.0, 0.0, -2.0], [2.0, 1.2, 2.0]))];
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
