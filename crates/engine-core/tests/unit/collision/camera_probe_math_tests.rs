use super::*;
use crate::scene_data::{CURRENT_SCHEMA_VERSION, CameraData, EntityData, TransformData};

fn slab(start: [f64; 3], delta: [f64; 3]) -> Result<Option<f64>, String> {
	segment_box_entry(start.into(), delta.into(), DVec3::ZERO, DVec3::ONE)
}

#[test]
fn slab_uses_the_latest_entry_and_earliest_exit_across_axes() {
	assert_eq!(slab([-2.0, -1.0, 0.5], [4.0, 4.0, 0.0]).unwrap(), Some(0.5));
	assert_eq!(slab([-2.0, -1.0, 0.5], [4.0, 5.0, 0.0]).unwrap(), None);
	assert_eq!(slab([-1.0, 1.1, 0.5], [2.0, 0.0, 0.0]).unwrap(), None);
	assert_eq!(slab([2.0, 0.5, 0.5], [-2.0, 0.0, 0.0]).unwrap(), Some(0.5));
	assert_eq!(slab([-1.0, 0.5, 0.5], [1.0, 0.0, 0.0]).unwrap(), Some(1.0));
	assert_eq!(slab([-2.0, 0.5, 0.5], [1.0, 0.0, 0.0]).unwrap(), None);
}

#[test]
fn slab_distinguishes_inward_outward_and_stationary_contact() {
	let face = [0.0, 0.5, 0.5];
	assert_eq!(slab(face, [1.0, 0.0, 0.0]).unwrap(), Some(0.0));
	assert_eq!(slab(face, [-1.0, 0.0, 0.0]).unwrap(), None);
	assert_eq!(slab(face, [0.0; 3]).unwrap(), None);
	assert_eq!(slab([-1.0; 3], [0.0; 3]).unwrap(), None);
	assert!(slab([0.5; 3], [0.0; 3]).is_err());
	assert!(slab([f64::NAN; 3], [0.0; 3]).is_err());
	assert!(segment_box_entry(DVec3::ZERO, DVec3::X, DVec3::ONE, DVec3::ZERO).is_err());
}

#[test]
fn safe_distances_round_towards_the_pivot_even_when_the_nearest_float_is_larger() {
	for value in [
		0.0,
		f64::from(f32::from_bits(1)) * 0.9,
		0.1,
		1.0 / 3.0,
		2.749,
		f64::from(f32::MAX),
	] {
		let rounded = distance_round_down(value).unwrap();
		assert!(rounded >= 0.0 && f64::from(rounded) <= value);
		if rounded < f32::MAX {
			assert!(f64::from(f32::from_bits(rounded.to_bits() + 1)) > value);
		}
	}
	for invalid in [-0.25, f64::from(f32::MAX) * 2.0, f64::INFINITY, f64::NAN] {
		assert!(distance_round_down(invalid).is_err());
	}
}

fn key(name: &str) -> EntityKey {
	EntityKey::new(name).unwrap()
}

fn wall(name: &str, position: [f32; 3], half_extents: [f32; 3]) -> EntityData {
	EntityData::new(key(name), name, "models/test.gltf")
		.with_transform(TransformData {
			position,
			..Default::default()
		})
		.with_collider(Some(ColliderData::Box {
			offset: [0.0; 3],
			half_extents,
			interaction: Default::default(),
		}))
}

struct Room {
	world: RuntimeWorld,
	collisions: CollisionWorld,
	lifecycle: RuntimeLifecycle,
	target: RuntimeEntityHandle,
	exclusions: CameraExclusions,
}

impl Room {
	fn new(mut entities: Vec<EntityData>) -> Self {
		entities.push(EntityData::new(key("target"), "Target", "models/test.gltf"));
		let template = crate::Camera::default();
		let world = RuntimeWorld::new(Arc::new(SceneData {
			schema_version: CURRENT_SCHEMA_VERSION,
			organization: Default::default(),
			default_game_camera: None,
			bindings: Default::default(),
			lights: vec![],
			entities,
			camera: CameraData {
				follow: None,
				orbit: None,
				position: template.position,
				yaw: template.yaw,
				pitch: template.pitch,
				vertical_fov: template.vertical_fov,
				near_plane: template.near,
				far_plane: template.far,
			},
		}))
		.unwrap();
		let collisions = CollisionWorld::new(&world).unwrap();
		let lifecycle = RuntimeLifecycle::new(&world);
		let target = lifecycle.handle(&key("target")).unwrap();
		let mut budget = MAX_CANDIDATES;
		let exclusions = CameraExclusions::build(&world, &lifecycle, &target, &mut budget).unwrap();
		Self {
			world,
			collisions,
			lifecycle,
			target,
			exclusions,
		}
	}

	fn probe(&self) -> CameraProbe<'_> {
		CameraProbe {
			pivot: [0.0; 3],
			forward: [0.0, 0.0, -1.0],
			distance: 5.0,
			radius: 0.25,
			obstruction_mask: 1,
			target: &self.target,
			topology_revision: self.lifecycle.topology_revision,
		}
	}

	fn cast(&self, probe: CameraProbe<'_>, budget: &mut usize) -> Result<CameraProbeLimit, String> {
		self.collisions
			.cast_camera_probe(&self.world, probe, &self.exclusions, budget)
	}
}

#[test]
fn equal_grazing_hits_choose_the_stable_key_in_either_entity_order() {
	// Neither thin wall intersects the center ray. Both intersect its radius at the same Z.
	// B has a smaller minimum X and is always scanned before A in the broad phase.
	let a = wall("a", [0.16, 0.0, 3.0], [0.05, 0.10, 0.25]);
	let b = wall("b", [-0.16, 0.0, 3.0], [0.05, 0.10, 0.25]);
	for walls in [vec![a.clone(), b.clone()], vec![b, a]] {
		let room = Room::new(walls);
		let mut budget = 5; // Query, two broad-phase visits, two rounded endpoint checks.
		let result = room.cast(room.probe(), &mut budget).unwrap();
		let hit = result.hit.unwrap();
		assert_eq!(hit.blocker, key("a"));
		assert!((0.499..0.5).contains(&hit.fraction));
		assert!((2.49..2.5).contains(&result.distance));
		assert_eq!(budget, 0);
	}
}

#[test]
fn boundary_and_zero_length_queries_check_the_physical_origin_before_the_guard() {
	// The physical lower Z face after expansion is exactly zero.
	let room = Room::new(vec![wall("wall", [0.0, 0.0, 0.5], [0.25; 3])]);
	let mut budget = 100;
	let inward = room.cast(room.probe(), &mut budget).unwrap();
	assert_eq!(inward.distance, 0.0);
	assert_eq!(inward.hit.unwrap().fraction, 0.0);
	let outward = room
		.cast(
			CameraProbe {
				forward: [0.0, 0.0, 1.0],
				..room.probe()
			},
			&mut budget,
		)
		.unwrap();
	assert_eq!(outward.distance, 5.0);
	assert!(outward.hit.is_none());
	let stationary = room
		.cast(
			CameraProbe {
				distance: 0.0,
				..room.probe()
			},
			&mut budget,
		)
		.unwrap();
	assert_eq!(stationary.distance, 0.0);
	assert!(stationary.hit.is_none());
	let penetrating = CameraProbe {
		distance: 0.0,
		pivot: [0.0, 0.0, 0.01],
		..room.probe()
	};
	let error = room.cast(penetrating, &mut budget).err().unwrap();
	assert!(error.contains("penetrating collider wall") && error.contains("pivot offset"));
}

#[test]
fn malformed_probe_is_rejected_before_visiting_colliders() {
	let room = Room::new(vec![]);
	let mut budget = 1;
	for invalid in [
		CameraProbe {
			forward: [0.0; 3],
			..room.probe()
		},
		CameraProbe {
			distance: -1.0,
			..room.probe()
		},
		CameraProbe {
			radius: f32::NAN,
			..room.probe()
		},
		CameraProbe {
			pivot: [f32::MAX; 3],
			distance: f32::MAX,
			..room.probe()
		},
	] {
		assert!(room.cast(invalid, &mut budget).is_err());
		assert_eq!(budget, 1);
	}
}

#[test]
fn a_later_penetrating_candidate_invalidates_an_earlier_hit() {
	let room = Room::new(vec![
		wall("hit", [0.0, 0.0, 3.0], [2.0, 1.0, 0.5]),
		wall("origin", [0.0; 3], [0.25; 3]),
	]);
	let error = room.cast(room.probe(), &mut 10).err().unwrap();
	assert!(error.contains("penetrating collider origin"), "{error}");
}

#[test]
fn probe_consumes_only_budget_leaving_world_and_sensor_contacts_intact() {
	let mut sensor = wall("sensor", [0.0, 0.0, 2.0], [0.5; 3]);
	if let Some(ColliderData::Box { interaction, .. }) = sensor.collider.as_mut() {
		interaction.sensor = true;
	}
	let mut room = Room::new(vec![sensor, wall("wall", [0.0, 0.0, 2.0], [0.25; 3])]);
	let contacts = room
		.collisions
		.query_sensor_overlaps_with_budget(&mut 10)
		.unwrap();
	assert_eq!(contacts.len(), 1);
	let stats = room.collisions.stats();
	let document = room.world.document();
	let matrices = room.world.world_matrices().to_vec();
	let updates = room.world.matrix_updates();
	let mut budget = 4;
	assert_eq!(
		room.cast(room.probe(), &mut budget)
			.unwrap()
			.hit
			.unwrap()
			.blocker,
		key("wall")
	);
	assert_eq!(budget, 0);
	// With a zero mask even the skipped entries are charged; endpoint checks are unnecessary.
	let mut filtered_budget = 3;
	let clear = room
		.cast(
			CameraProbe {
				obstruction_mask: 0,
				..room.probe()
			},
			&mut filtered_budget,
		)
		.unwrap();
	assert!(clear.hit.is_none());
	assert_eq!(clear.distance, 5.0);
	assert_eq!(filtered_budget, 0);
	assert_eq!(room.collisions.stats().queries, stats.queries);
	assert_eq!(
		room.collisions.stats().updated_colliders,
		stats.updated_colliders
	);
	assert_eq!(room.collisions.stats().rebuilds, stats.rebuilds);
	assert_eq!(room.world.data(), document.as_ref());
	assert_eq!(room.world.world_matrices(), matrices);
	assert_eq!(room.world.matrix_updates(), updates);
	assert_eq!(
		room.collisions
			.query_sensor_overlaps_with_budget(&mut 10)
			.unwrap(),
		contacts
	);
}

#[test]
fn local_and_shared_work_limits_charge_the_last_successful_visit_exactly_once() {
	let mut visits = 0;
	let mut remaining = CAMERA_MAX_VISITS + 10;
	for _ in 0..CAMERA_MAX_VISITS {
		charge_camera_visit(&mut visits, &mut remaining).unwrap();
	}
	assert!(
		charge_camera_visit(&mut visits, &mut remaining)
			.unwrap_err()
			.contains("8192")
	);
	assert_eq!((visits, remaining), (8192, 10));
	let room = Room::new(vec![wall("wall", [0.0, 0.0, 3.0], [0.25; 3])]);
	let mut remaining = 2; // Query and broad phase fit. The endpoint check must still charge work.
	assert!(
		room.cast(room.probe(), &mut remaining)
			.err()
			.unwrap()
			.contains("budget")
	);
	assert_eq!(remaining, 0);
}

#[test]
fn changed_topology_rebuilds_exclusions_even_with_the_same_target_and_entity_count() {
	let mut room = Room::new(vec![wall("child", [0.0, 0.0, 2.0], [0.25; 3])]);
	assert_eq!(
		room.cast(room.probe(), &mut 10)
			.unwrap()
			.hit
			.unwrap()
			.blocker,
		key("child")
	);
	let mut data = room.world.data().clone();
	data.entities[0].parent = Some(key("target"));
	room.world = RuntimeWorld::new(Arc::new(data)).unwrap();
	room.collisions = CollisionWorld::new(&room.world).unwrap();
	room.lifecycle.topology_revision += 1;
	let mut budget = 10;
	assert!(
		room.cast(room.probe(), &mut budget)
			.err()
			.unwrap()
			.contains("topology")
	);
	assert_eq!(budget, 10);
	room.exclusions =
		CameraExclusions::build(&room.world, &room.lifecycle, &room.target, &mut budget).unwrap();
	assert!(room.cast(room.probe(), &mut budget).unwrap().hit.is_none());
	let new_generation = RuntimeEntityHandle {
		generation: room.target.generation + 1,
		..room.target.clone()
	};
	let foreign = RuntimeLifecycle::new(&room.world)
		.handle(&room.target.key)
		.unwrap();
	for handle in [new_generation, foreign] {
		let mut budget = 10;
		assert!(
			room.cast(
				CameraProbe {
					target: &handle,
					..room.probe()
				},
				&mut budget
			)
			.is_err()
		);
		assert!(
			CameraExclusions::build(&room.world, &room.lifecycle, &handle, &mut budget).is_err()
		);
		assert_eq!(budget, 10);
	}
}

#[test]
fn large_world_coordinate_cannot_erase_the_probe_radius_at_a_wall_face() {
	// A local quarter metre is smaller than a single f64 ULP at this X position.
	// The cached X bounds round to the pivot plane, so its sphere overlaps the wall.
	let x = 2.0_f32.powi(58);
	let room = Room::new(vec![wall("wall", [x, 0.0, 0.0], [0.25; 3])]);
	let probe = CameraProbe {
		pivot: [x, 0.0, 0.0],
		..room.probe()
	};
	let result = room.cast(probe, &mut 10);
	assert!(
		result
			.err()
			.is_some_and(|error| error.contains("penetrating collider wall"))
	);
}

#[test]
#[ignore = "local CPU benchmark; run in release with --ignored --nocapture"]
fn measure_camera_probe_scaling() {
	use std::{hint::black_box, time::Instant};
	for (count, mask) in [(0, 1), (10, 1), (100, 1), (1000, 1), (1000, 0), (8193, 0)] {
		// Every AABB reaches the broad-phase filter. Eligible walls also require an endpoint check.
		let entities = (0..count)
			.map(|i| wall(&format!("wall-{i}"), [0.0, 0.0, 3.0], [0.25; 3]))
			.collect();
		let room = Room::new(entities);
		let cast = || {
			let mut remaining = MAX_CANDIDATES;
			let result = black_box(room.cast(
				CameraProbe {
					obstruction_mask: mask,
					..room.probe()
				},
				&mut remaining,
			));
			assert_eq!(result.is_err(), count > CAMERA_MAX_VISITS);
			MAX_CANDIDATES - remaining - 1
		};
		for _ in 0..64 {
			black_box(cast());
		}
		let mut batches = Vec::new();
		for _ in 0..31 {
			let start = Instant::now();
			for _ in 0..32 {
				black_box(cast());
			}
			batches.push(start.elapsed().as_secs_f64() * 1.0e6 / 32.0);
		}
		batches.sort_by(f64::total_cmp);
		let visits = cast();
		assert_eq!(
			visits,
			if mask == 0 {
				count.min(CAMERA_MAX_VISITS)
			} else {
				2 * count
			}
		);
		println!(
			"obstacles={count} mask={mask} visits={visits} batch_mean_us p50={:.3} p95={:.3} max={:.3}",
			batches[15], batches[29], batches[30]
		);
	}
}
