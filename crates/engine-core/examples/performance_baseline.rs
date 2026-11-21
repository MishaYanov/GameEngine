//! Repeatable CPU benchmark and the shared scene fixture for the hidden native benchmark.
//! Run in release mode; wall times are local regression evidence, not universal budgets.
use engine_core::{
	EntityKey, SceneData,
	project::{BehaviorSpace, BehaviorSpec, ProjectData},
	scene_data::{
		CURRENT_SCHEMA_VERSION, CameraData, ColliderData, ColliderInteraction, EntityData,
		TransformData,
	},
	simulation_session::SimulationSession,
};
use std::{path::PathBuf, sync::Arc, time::Instant};

fn key(value: impl Into<String>) -> EntityKey {
	EntityKey::new(value).unwrap()
}
fn fixture() -> SceneData {
	let mut entities = Vec::new();
	for row in 0..24 {
		for column in 0..24 {
			let mut value = EntityData::new(
				key(format!("prop-{row}-{column}")),
				"Repeated prop",
				match (row + column) % 3 {
					0 => "props/cube.glb",
					1 => "props/sphere.glb",
					_ => "props/capsule.glb",
				},
			)
			.with_transform(TransformData {
				position: [column as f32 * 1.5 - 18.0, row as f32 * 1.5 - 18.0, 0.0],
				rotation: [0.0, (row + column) as f32 * 0.1, 0.0],
				scale: [0.4; 3],
			});
			if row % 4 == 0 && column % 4 == 0 {
				value.collider = Some(ColliderData::Box {
					half_extents: [0.5; 3],
					offset: [0.0; 3],
					interaction: ColliderInteraction::default(),
				});
			}
			entities.push(value);
		}
	}
	for index in 0..16 {
		let mut mover = EntityData::new(
			key(format!("mover-{index}")),
			"Moving actor",
			"props/capsule.glb",
		)
		.with_transform(TransformData {
			position: [-10.0 + index as f32, -20.0, 0.0],
			scale: [0.4; 3],
			..Default::default()
		});
		mover.collider = Some(ColliderData::Sphere {
			radius: 0.5,
			offset: [0.0; 3],
			interaction: ColliderInteraction::default(),
		});
		entities.push(mover);
		let mut sensor = EntityData::new(
			key(format!("sensor-{index}")),
			"Trigger",
			"props/sphere.glb",
		)
		.with_transform(TransformData {
			position: [-9.0 + index as f32, -20.0, 0.0],
			..Default::default()
		})
		.with_renderable(false);
		sensor.collider = Some(ColliderData::Sphere {
			radius: 0.4,
			offset: [0.0; 3],
			interaction: ColliderInteraction {
				sensor: true,
				layers: 2,
				mask: 1,
			},
		});
		entities.push(sensor);
	}
	SceneData {
		schema_version: CURRENT_SCHEMA_VERSION,
		bindings: Default::default(),
		entities,
		camera: CameraData {
			position: [0.0, 0.0, 38.0],
			yaw: 0.0,
			pitch: 0.0,
			vertical_fov: 60f32.to_radians(),
			near_plane: 0.1,
			far_plane: 100.0,
			follow: None,
			orbit: None,
		},
		lights: vec![],
	}
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
	let mut output = PathBuf::from("target/performance/core.json");
	let mut scene_output = PathBuf::from("target/performance/representative-scene.json");
	let mut args = std::env::args().skip(1);
	while let Some(flag) = args.next() {
		let value = args.next().ok_or("Expected a file path after the option")?;
		match flag.as_str() {
			"--output" => output = value.into(),
			"--scene-output" => scene_output = value.into(),
			_ => return Err(format!("Unknown option {flag}").into()),
		}
	}
	let scene = fixture();
	let encoded = serde_json::to_vec_pretty(&scene)?;
	let text = std::str::from_utf8(&encoded)?;
	let mut project = ProjectData::for_scene_with_id(
		"Performance baseline",
		engine_core::scene_catalog::SceneId::new("benchmark")?,
		"scenes/representative.json",
		&scene,
	)?;
	for index in 0..16 {
		project.add_behavior(BehaviorSpec::ControlledMove {
			entity: key(format!("mover-{index}")),
			speed: 0.4,
			space: BehaviorSpace::World,
		});
	}
	let mut load_samples = Vec::with_capacity(20);
	for _ in 0..20 {
		let started = Instant::now();
		let parsed = SceneData::from_json(text)?;
		let session = SimulationSession::new(project.clone(), Arc::new(parsed))?;
		std::hint::black_box(&session);
		load_samples.push(started.elapsed().as_secs_f64() * 1000.0);
	}
	let mut session = SimulationSession::new(project, Arc::new(scene))?;
	session.set_controlled_input([1.0, 0.0, 0.0])?;
	for _ in 0..60 {
		session.tick(1.0 / 60.0)?;
	}
	let start_updates = session.world().matrix_updates();
	let start_stats = session.collision_stats();
	let mut samples = Vec::with_capacity(600);
	let mut edges = 0usize;
	for _ in 0..600 {
		let started = Instant::now();
		let result = session.tick(1.0 / 60.0)?;
		samples.push(started.elapsed().as_secs_f64() * 1000.0);
		edges += result.trigger_events.len();
	}
	let final_stats = session.collision_stats();
	let entities = session.world().data().entities.len();
	let metrics = serde_json::json!({
		"schema_version": 1, "fixture": "representative-608-v1", "profile": if cfg!(debug_assertions) {"debug"} else {"release"},
		"entities": entities, "renderable_entities": session.world().data().entities.iter().filter(|entity| entity.renderable).count(),
		"colliders": 68, "sensors": 16, "moving_actors": 16, "source_models": 3,
		"load_samples": 20, "load_median_ms": percentile(&mut load_samples, 0.5), "load_p95_ms": percentile(&mut load_samples, 0.95),
		"warmup_ticks": 60, "measured_ticks": 600, "tick_median_ms": percentile(&mut samples, 0.5), "tick_p95_ms": percentile(&mut samples, 0.95), "tick_max_ms": percentile(&mut samples, 1.0),
		"trigger_edges": edges, "matrix_updates": session.world().matrix_updates()-start_updates,
		"collision_rebuilds": final_stats.rebuilds-start_stats.rebuilds, "collision_updates": final_stats.updated_colliders-start_stats.updated_colliders,
		"collision_queries": final_stats.queries-start_stats.queries,
		"serialized_scene_bytes": encoded.len(), "world_matrix_bytes": std::mem::size_of_val(session.world().world_matrices()),
		"memory_scope": "Scene serialization and matrix payload only; process working set is measured by the runner.",
		"load_scope": "JSON parse, validation, module and collision-cache construction; excludes model decoding, disk cache and GPU uploads.",
	});
	for path in [&output, &scene_output] {
		if let Some(parent) = path.parent() {
			std::fs::create_dir_all(parent)?;
		}
	}
	std::fs::write(scene_output, &encoded)?;
	std::fs::write(&output, serde_json::to_vec_pretty(&metrics)?)?;
	println!("{}", serde_json::to_string_pretty(&metrics)?);
	Ok(())
}
fn percentile(samples: &mut [f64], fraction: f64) -> f64 {
	samples.sort_unstable_by(f64::total_cmp);
	samples[((samples.len() as f64 * fraction).ceil() as usize)
		.saturating_sub(1)
		.min(samples.len() - 1)]
}
