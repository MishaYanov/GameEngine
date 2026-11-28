mod support;

use engine_core::{
	Camera,
	collision::CollisionWorld,
	scene_data::{ColliderData, ColliderInteraction},
	world::RuntimeWorld,
};
use glam::{Vec2, Vec4};
use std::sync::Arc;
use support::{entity, key, scene};

#[derive(Debug)]
struct PrecisionSample {
	camera_ndc: Option<Vec2>,
	stop_x: Option<f32>,
	hit_wall: bool,
}

fn probe(origin: f32) -> PrecisionSample {
	let camera = Camera {
		position: [origin, 0.25, origin],
		yaw: 0.36,
		pitch: 0.08,
		..Camera::default()
	};
	let camera_ndc = camera.view_projection(16.0 / 9.0).ok().and_then(|matrix| {
		let clip = matrix * Vec4::new(origin + 2.0, 0.7, origin - 6.0, 1.0);
		let ndc = Vec2::new(clip.x, clip.y) / clip.w;
		ndc.is_finite().then_some(ndc)
	});

	let actor = entity("actor", [origin, 0.0, origin]).with_collider(Some(ColliderData::Sphere {
		radius: 0.2,
		offset: [0.0; 3],
		interaction: ColliderInteraction::default(),
	}));
	let wall = entity("wall", [origin + 0.6, 0.0, origin]).with_collider(Some(ColliderData::Box {
		half_extents: [0.1, 1.0, 1.0],
		offset: [0.0; 3],
		interaction: ColliderInteraction::default(),
	}));
	let mut world = RuntimeWorld::new(Arc::new(scene(vec![actor, wall]))).unwrap();
	let mut collision = CollisionWorld::new(&world).unwrap();
	let movement = collision.move_entity(&mut world, &key("actor"), [0.8, 0.0, 0.0]);
	PrecisionSample {
		camera_ndc,
		stop_x: movement
			.as_ref()
			.ok()
			.map(|_| world.data().entities[0].transform.position[0] - origin),
		hit_wall: movement.is_ok_and(|movement| movement.blocked_by == Some(key("wall"))),
	}
}

#[test]
fn world_coordinate_precision_reports_a_cpu_acceptance_envelope() {
	const CAMERA_NDC_ERROR: f32 = 0.02;
	const STOP_DISTANCE_ERROR: f32 = 0.02;
	let baseline = probe(0.0);
	assert!(baseline.hit_wall);
	let baseline_ndc = baseline.camera_ndc.unwrap();
	let baseline_stop = baseline.stop_x.unwrap();
	println!("coordinate,camera_ndc_error,stop_distance_error,hit_wall,accepted");
	for origin in [
		0.0,
		1_000.0,
		10_000.0,
		100_000.0,
		1_000_000.0,
		10_000_000.0,
		100_000_000.0,
	] {
		let sample = probe(origin);
		let camera_error = sample
			.camera_ndc
			.map_or(f32::INFINITY, |ndc| ndc.distance(baseline_ndc));
		let stop_error = sample
			.stop_x
			.map_or(f32::INFINITY, |stop| (stop - baseline_stop).abs());
		let accepted = sample.hit_wall
			&& camera_error <= CAMERA_NDC_ERROR
			&& stop_error <= STOP_DISTANCE_ERROR;
		println!(
			"{origin},{camera_error},{stop_error},{},{accepted}",
			sample.hit_wall
		);
		if origin <= 100_000.0 {
			assert!(
				accepted,
				"camera or collision regression at {origin}: {sample:?}"
			);
		}
	}
}
