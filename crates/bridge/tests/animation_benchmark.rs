//! Named, opt-in CPU measurement. This reports elapsed work, not a frame-rate claim.
use engine_core::{
	EntityKey, SceneData, animation::*, runtime_lifecycle::RuntimeEntityHandle, world::RuntimeWorld,
};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc, time::Instant};

#[test]
#[ignore = "opt-in 64-instance CPU pose and palette measurement"]
fn animation_64_instances_300_ticks_cpu_and_palette_bytes() {
	let mut scene = SceneData::from_json(include_str!(
		"../../../apps/game/assets/scenes/animation-room.json"
	))
	.unwrap();
	let template = scene
		.entities
		.iter()
		.find(|e| e.key.as_str() == "clip-demo")
		.unwrap()
		.clone();
	scene.entities = (0..64)
		.map(|n| {
			let mut entity = template.clone();
			entity.key = EntityKey::new(format!("animation-{n:02}")).unwrap();
			entity.transform.position = [n as f32 * 2., 0., 0.];
			entity
		})
		.collect();
	scene.bindings.clear();
	scene.default_game_camera = None;
	scene.camera.follow = None;
	scene.lights.clear();
	let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../apps/game/assets");
	let bindings = bridge::animation_asset::prepare_scene(&root, &scene).unwrap();
	assert_eq!(bindings.len(), 64);
	let first = bindings.values().next().unwrap();
	assert!(bindings.values().all(|asset| Arc::ptr_eq(first, asset)));
	let asset_bytes = first.retained_bytes();
	let world = RuntimeWorld::new(Arc::new(scene)).unwrap();
	let handles = world
		.data()
		.entities
		.iter()
		.map(|e| {
			(
				e.key.clone(),
				RuntimeEntityHandle {
					world_id: "benchmark".into(),
					key: e.key.clone(),
					generation: 1,
				},
			)
		})
		.collect();
	let mut bank = AnimationBank::new(&world, &bindings, &handles).unwrap();
	let motions = BTreeMap::new();
	let start = Instant::now();
	for tick in 1..=300 {
		bank.step(&world, &motions, tick, 60).unwrap();
		std::hint::black_box(&bank);
	}
	let elapsed = start.elapsed();
	let palette_bytes: usize = bank.poses().map(|(_, pose)| pose.palette.len() * 64).sum();
	assert_eq!(palette_bytes, 64 * 2 * 64);
	assert!(bank.poses().all(|(_, p)| p.elapsed_ticks == 300));
	println!(
		"ANIMATION_CPU_MEASUREMENT {}",
		serde_json::json!({"fixture":"two-joint.glb","instances":64,"ticks":300,"fixed_hz":60,"elapsed_ms":elapsed.as_secs_f64()*1000.,"us_per_tick":elapsed.as_secs_f64()*1e6/300.,"asset_bytes_shared":asset_bytes,"palette_bytes_per_frame":palette_bytes,"profile":if cfg!(debug_assertions){"debug"}else{"release"},"machine":std::env::var("COMPUTERNAME").unwrap_or_default(),"scope":"CPU pose evaluation only; no GPU/FPS claim"})
	);
}
