use super::*;
use crate::{
	EntityKey, SceneData,
	scene_data::{CURRENT_SCHEMA_VERSION, CameraData, EntityData},
};
use crate::{
	camera_input::CameraIntent,
	collision::{CameraExclusions, CameraProbeHit, CollisionWorld},
	runtime_lifecycle::{RuntimeEntityHandle, RuntimeLifecycle},
};
use std::collections::HashSet;
use std::f32::consts::{FRAC_PI_2, FRAC_PI_6, PI};
use std::sync::Arc;

fn clear_room(
	_pivot: [f32; 3],
	_forward: [f32; 3],
	distance: f32,
) -> Result<OrbitArmLimit, String> {
	Ok(OrbitArmLimit {
		distance,
		hit: None,
	})
}

fn settings() -> OrbitCameraSettings {
	OrbitCameraSettings {
		target: EntityKey::new("actor").unwrap(),
		pivot_offset: [0.0; 3],
		initial_yaw: 0.0,
		initial_pitch: 0.0,
		initial_distance: 5.0,
		min_pitch: -1.0,
		max_pitch: 1.0,
		min_distance: 1.0,
		max_distance: 10.0,
		follow_seconds: 0.1,
		recovery_seconds: 0.2,
		teleport_distance: 10.0,
		probe_radius: 0.25,
		obstruction_mask: 1,
	}
}

fn subject(settings: &OrbitCameraSettings) -> CameraTargetSnapshot {
	CameraTargetSnapshot {
		handle: RuntimeEntityHandle {
			world_id: "test".into(),
			key: settings.target.clone(),
			generation: 1,
		},
		world_position: [0.0; 3],
	}
}

fn fixed_step(target: &CameraTargetSnapshot, tick: u64) -> OrbitStepInput<'_> {
	OrbitStepInput {
		target: Some(target),
		intent: CameraIntent::default(),
		tick,
		step_seconds: 1.0 / 60.0,
		snap: false,
	}
}

fn close(actual: f32, expected: f32) {
	assert!((actual - expected).abs() < 1.0e-5, "{actual} != {expected}");
}

#[test]
fn cardinal_angles_and_zero_arm_retain_forward_orientation() {
	let template = Camera::default();
	for (yaw, pitch, expected) in [
		(0.0, 0.0, [0.0, 0.0, 5.0]),
		(FRAC_PI_2, 0.0, [-5.0, 0.0, 0.0]),
		(0.0, -FRAC_PI_6, [0.0, 2.5, 4.330127]),
	] {
		let pose = orbit_pose(&template, [0.0; 3], yaw, pitch, 5.0).unwrap();
		for (actual, expected) in pose.position.into_iter().zip(expected) {
			close(actual, expected);
		}
	}
	let pose = orbit_pose(&template, [1.0, 2.0, 3.0], 0.4, -0.2, 0.0).unwrap();
	assert_eq!(
		(pose.position, pose.yaw, pose.pitch),
		([1.0, 2.0, 3.0], 0.4, -0.2)
	);
	assert!(orbit_pose(&template, [f32::MAX; 3], 0.0, 0.0, 5.0).is_err());
	assert!(orbit_pose(&template, [0.0; 3], 0.0, 0.0, f32::NAN).is_err());
}

#[test]
fn exponential_response_settles_exactly_and_zero_elapsed_preserves_residuals() {
	assert_eq!(smoothing_alpha(0.0, 0.1).unwrap(), 0.0);
	assert_eq!(smoothing_alpha(0.1, 0.0).unwrap(), 1.0);
	assert!(smoothing_alpha(f64::NAN, 0.1).is_err());
	let alpha = smoothing_alpha(1.0 / 60.0, 0.1).unwrap();
	let mut value = smooth_scalar(0.0, 1.0, alpha);
	close(value, 0.15351827);
	assert_eq!(smooth_scalar(value, value + 0.00001, 0.0), value);
	for _ in 1..240 {
		value = smooth_scalar(value, 1.0, alpha);
	}
	assert_eq!(value, 1.0);
	assert_eq!(smooth_scalar(f32::MAX, 1.0, 1.0), 1.0);
}

#[test]
fn both_signed_pi_defaults_stay_canonical_on_empty_steps() {
	let template = Camera::default();
	for endpoint in [-PI, PI] {
		let mut settings = settings();
		settings.initial_yaw = endpoint;
		let target = subject(&settings);
		let initial = OrbitRuntimeState::admit(&settings, &target, 0).unwrap();
		assert_eq!(initial.yaw, -PI);
		let step = resolve_orbit(
			&settings,
			&initial,
			&template,
			fixed_step(&target, 1),
			clear_room,
		)
		.unwrap();
		assert_eq!(step.pose.yaw, -PI);
		assert!(initial.same_status(&step.state));
	}
	assert!(normalize_yaw(f64::INFINITY).is_err());
	close(normalize_yaw(3.0 * std::f64::consts::PI).unwrap(), -PI);
}

#[test]
fn queried_arm_shortens_immediately_and_recovers_on_safe_segment() {
	let settings = settings();
	let target = subject(&settings);
	let initial = OrbitRuntimeState::admit(&settings, &target, 0).unwrap();
	let hit = CameraProbeHit {
		blocker: EntityKey::new("wall").unwrap(),
		fraction: 0.4,
	};
	let query = |_, _, distance| {
		assert_eq!(distance, 5.0);
		Ok(OrbitArmLimit {
			distance: 2.0,
			hit: Some(hit.clone()),
		})
	};
	let blocked = resolve_orbit(
		&settings,
		&initial,
		&Camera::default(),
		fixed_step(&target, 1),
		query,
	)
	.unwrap();
	assert_eq!(blocked.state.effective_distance, 2.0);
	assert_eq!(blocked.state.user_distance, 5.0);
	assert_eq!(blocked.state.obstruction.as_ref(), Some(&hit));
	assert!(!blocked.state.needs_tick());
	let stationary = resolve_orbit(
		&settings,
		&blocked.state,
		&blocked.pose,
		fixed_step(&target, 2),
		query,
	)
	.unwrap();
	assert!(blocked.state.same_status(&stationary.state));
	let recovered = resolve_orbit(
		&settings,
		&blocked.state,
		&blocked.pose,
		fixed_step(&target, 2),
		clear_room,
	)
	.unwrap();
	assert!(!blocked.state.same_status(&recovered.state));
	assert!(recovered.state.effective_distance > 2.0 && recovered.state.effective_distance < 5.0);
	assert_eq!(recovered.state.last_allowed_distance, 5.0);
	assert!(recovered.state.needs_tick());
	for invalid in [f32::NAN, -0.1, 6.0] {
		assert!(
			resolve_orbit(
				&settings,
				&initial,
				&Camera::default(),
				fixed_step(&target, 1),
				|_, _, _| Ok(OrbitArmLimit {
					distance: invalid,
					hit: None
				})
			)
			.is_err()
		);
	}
	assert!(
		resolve_orbit(
			&settings,
			&initial,
			&Camera::default(),
			fixed_step(&target, 1),
			|_, _, _| Ok(OrbitArmLimit {
				distance: 2.0,
				hit: Some(CameraProbeHit {
					fraction: 1.2,
					..hit.clone()
				})
			})
		)
		.is_err()
	);
}

#[test]
fn teleport_threshold_snaps_before_the_query_and_identity_mismatch_freezes_pose() {
	let settings = settings();
	let mut target = subject(&settings);
	let initial = OrbitRuntimeState::admit(&settings, &target, 0).unwrap();
	target.world_position[0] = settings.teleport_distance;
	let teleported = resolve_orbit(
		&settings,
		&initial,
		&Camera::default(),
		fixed_step(&target, 1),
		|pivot, forward, distance| {
			assert_eq!(pivot, target.world_position);
			assert_eq!(forward, [0.0, 0.0, -1.0]);
			Ok(OrbitArmLimit {
				distance,
				hit: None,
			})
		},
	)
	.unwrap();
	assert_eq!(teleported.state.filtered_pivot, target.world_position);
	target.handle.generation += 1;
	let lost = resolve_orbit(
		&settings,
		&teleported.state,
		&teleported.pose,
		fixed_step(&target, 2),
		|_, _, _| panic!("lost binding must skip query"),
	)
	.unwrap();
	assert!(lost.clear_input && lost.state.target_lost && !lost.state.needs_tick());
	assert_eq!(lost.pose.position, teleported.pose.position);
}

fn orbit_document() -> Arc<SceneData> {
	let settings = settings();
	let target = settings.target.clone();
	let template = Camera::default();
	Arc::new(SceneData {
		schema_version: CURRENT_SCHEMA_VERSION,
		bindings: Default::default(),
		lights: vec![],
		entities: vec![EntityData::new(
			target.clone(),
			"Actor",
			"models/actor.gltf",
		)],
		camera: CameraData {
			follow: None,
			orbit: Some(settings),
			position: template.position,
			yaw: template.yaw,
			pitch: template.pitch,
			vertical_fov: template.vertical_fov,
			near_plane: template.near,
			far_plane: template.far,
		},
	})
}

#[test]
fn repeated_probe_reuses_exclusions_and_debits_one_shared_budget() {
	let world = RuntimeWorld::new(orbit_document()).unwrap();
	let lifecycle = RuntimeLifecycle::new(&world);
	let collisions = CollisionWorld::new(&world).unwrap();
	let settings = world.data().camera.orbit.as_ref().unwrap();
	let target = lifecycle.handle(&settings.target).unwrap();
	let cast = |cache: &mut Option<CameraExclusions>, remaining: &mut usize| {
		obstruction_query(&world, &lifecycle, &collisions, cache, remaining)(
			&target,
			settings,
			[0.0; 3],
			[0.0, 0.0, -1.0],
			5.0,
		)
	};
	let mut exclusions = None;
	let mut budget = 3;
	assert_eq!(cast(&mut exclusions, &mut budget).unwrap().distance, 5.0);
	assert_eq!(budget, 1); // One entity in the exclusion table, then one cast.
	assert_eq!(cast(&mut exclusions, &mut budget).unwrap().distance, 5.0);
	assert_eq!(budget, 0);
	assert!(cast(&mut exclusions, &mut budget).is_err());
}

#[test]
fn standalone_admission_and_updates_probe_current_blockers() {
	use crate::scene_data::{ColliderData, TransformData};
	let mut document = orbit_document();
	let wall = EntityKey::new("wall").unwrap();
	let transform = |z| TransformData {
		position: [0.0, 0.0, z],
		..Default::default()
	};
	Arc::make_mut(&mut document).entities.push(
		EntityData::new(wall.clone(), "Wall", "models/wall.gltf")
			.with_transform(transform(2.5))
			.with_collider(Some(ColliderData::Box {
				interaction: Default::default(),
				half_extents: [0.5; 3],
				offset: [0.0; 3],
			})),
	);
	let mut world = RuntimeWorld::new(document).unwrap();
	let mut camera = GameCameraController::new(&world).unwrap();
	let admitted = camera.orbit_state().unwrap();
	assert_eq!(admitted.obstruction.as_ref().unwrap().blocker, wall);
	close(admitted.effective_distance, 1.749);
	assert!(!camera.needs_tick(&world));
	let previous = admitted.clone();
	world.set_transform(&wall, transform(0.0)).unwrap();
	assert!(camera.update(&world, 1.0 / 60.0).is_err());
	assert_eq!(camera.orbit_state(), Some(&previous));
	assert!(camera.needs_tick(&world));
	world.set_transform(&wall, transform(1.5)).unwrap();
	assert!(camera.needs_tick(&world));
	camera.update(&world, 1.0 / 60.0).unwrap();
	close(camera.camera().position[2], 0.749);
	assert!(!camera.needs_tick(&world));
	world.set_collision_enabled(&wall, false).unwrap();
	assert!(camera.needs_tick(&world));
	camera.update(&world, 1.0 / 60.0).unwrap();
	let clear = camera.orbit_state().unwrap();
	assert!(clear.obstruction.is_none());
	assert_eq!(clear.last_allowed_distance, 5.0);
	assert!(clear.effective_distance > 0.749 && clear.effective_distance < 5.0);
}

#[test]
fn independent_world_with_the_same_key_cannot_take_over_a_standalone_rig() {
	let document = orbit_document();
	let mut original = RuntimeWorld::new(document.clone()).unwrap();
	let mut replacement = RuntimeWorld::new(document).unwrap();
	let target = settings().target;
	let mut controller = GameCameraController::new(&original).unwrap();
	let accepted = controller.camera().position;
	// Even identical coordinates need one step to publish terminal target loss.
	assert!(!controller.needs_tick(&original));
	assert!(controller.needs_tick(&replacement));
	let transform = crate::scene_data::TransformData {
		position: [8.0, 0.0, 0.0],
		..Default::default()
	};
	replacement
		.set_transform(&target, transform.clone())
		.unwrap();
	controller.update(&replacement, 1.0).unwrap();
	assert!(controller.orbit_state().unwrap().target_lost);
	assert_eq!(controller.camera().position, accepted);
	assert!(!controller.needs_tick(&replacement));

	let mut fresh = GameCameraController::new(&original).unwrap();
	original.set_transform(&target, transform).unwrap();
	assert!(fresh.needs_tick(&original));
	fresh.update(&original, 1.0).unwrap();
	assert!(!fresh.orbit_state().unwrap().target_lost);
	assert!(fresh.camera().position[0] > accepted[0]);
}

#[test]
fn lifecycle_removal_clears_input_freezes_pose_and_never_rebinds() {
	let document = orbit_document();
	let target = settings().target;
	let mut world = RuntimeWorld::new(document.clone()).unwrap();
	let mut lifecycle = RuntimeLifecycle::new(&world);
	let mut controller =
		GameCameraController::new_with_lifecycle(&world, &lifecycle, |_, _, p, f, d| {
			clear_room(p, f, d)
		})
		.unwrap();
	let yaw = CameraIntent {
		yaw_delta: 0.25,
		..Default::default()
	};
	assert!(
		!controller
			.step(&world, &lifecycle, 1.0 / 60.0, yaw, 1, |_, _, p, f, d| {
				clear_room(p, f, d)
			})
			.unwrap()
	);
	let accepted = controller.camera().clone();
	let removed = HashSet::from([target]);
	lifecycle.removed(&removed).unwrap();
	world.remove_runtime_entities(&removed).unwrap();
	assert!(world.data().camera.orbit.is_none());
	assert!(controller.needs_tick(&world));
	assert!(
		controller
			.step(
				&world,
				&lifecycle,
				1.0 / 60.0,
				yaw,
				2,
				|_, _, _, _, _| panic!("lost target must skip probe")
			)
			.unwrap()
	);
	assert!(controller.orbit_state().unwrap().target_lost);
	assert!(!controller.needs_tick(&world));
	let replacement = RuntimeWorld::new(document).unwrap();
	let new_lifecycle = RuntimeLifecycle::new(&replacement);
	assert!(
		controller
			.step(
				&replacement,
				&new_lifecycle,
				1.0 / 60.0,
				yaw,
				3,
				|_, _, _, _, _| panic!("lost target must skip probe")
			)
			.unwrap()
	);
	assert_eq!(controller.camera().position, accepted.position);
	assert_eq!(controller.camera().yaw, accepted.yaw);
	assert!(!controller.needs_tick(&replacement));
}
