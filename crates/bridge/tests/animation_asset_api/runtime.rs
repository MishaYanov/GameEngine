use std::{
	collections::BTreeMap,
	sync::{
		Arc,
		atomic::{AtomicUsize, Ordering},
	},
};

use bridge::animation_asset;
use engine_core::{
	SceneData,
	animation::{AnimationAsset, sample_local},
	project::ProjectData,
	scene_catalog::SceneId,
	simulation_session::{
		ActionWriter, GameModule, GameModuleFactory, InputFrame, SimulationAction,
		SimulationSession,
	},
	world::RuntimeWorld,
};

use super::{close, decoded, key, root, scene};

#[test]
fn animation_alone_schedules_ticks_and_finished_one_shot_reaches_idle() {
	let data = scene(true);
	let mut looping = SimulationSession::new(project(&data), Arc::new(data)).unwrap();
	assert!(
		looping.tick(0.05).is_err(),
		"unprepared animation cannot begin"
	);
	install(&mut looping, decoded());
	assert!(looping.needs_tick());
	looping.tick(0.05).unwrap();
	assert_eq!(
		looping
			.animations()
			.pose(&key("clip-demo"))
			.unwrap()
			.elapsed_ticks,
		1
	);
	looping.set_paused(true);
	assert_eq!(looping.tick(0.2).unwrap().steps, 0);
	looping.set_paused(false);
	assert!(looping.needs_tick());
	let data = scene(false);
	let mut once = SimulationSession::new(project(&data), Arc::new(data)).unwrap();
	install(&mut once, decoded());
	for _ in 0..20 {
		once.tick(0.05).unwrap();
	}
	assert!(!once.needs_tick());
	assert_eq!(
		once.animations()
			.pose(&key("clip-demo"))
			.unwrap()
			.elapsed_ticks,
		20
	);
	assert_eq!(once.tick(0.05).unwrap().steps, 0);
}

struct Failure(Arc<AtomicUsize>);
impl GameModuleFactory for Failure {
	fn create(&self, _: &ProjectData, _: &SceneId) -> Result<Box<dyn GameModule>, String> {
		Ok(Box::new(Self(self.0.clone())))
	}
}
impl GameModule for Failure {
	fn needs_tick(&self, _: &InputFrame) -> bool {
		true
	}
	fn plan(
		&self,
		_: &RuntimeWorld,
		_: &InputFrame,
		_: f32,
		out: &mut ActionWriter<'_>,
	) -> Result<(), String> {
		if self.0.fetch_add(1, Ordering::Relaxed) == 2 {
			out.push(SimulationAction::SetVisible {
				entity: key("missing"),
				visible: false,
			})?;
		}
		Ok(())
	}
}
#[test]
fn late_substep_failure_preserves_pose_clock_palette_bounds_and_revision() {
	let data = Arc::new(scene(true));
	let mut simulation = SimulationSession::with_factory(
		project(&data),
		data.clone(),
		&Failure(Arc::new(AtomicUsize::new(0))),
	)
	.unwrap();
	install(&mut simulation, decoded());
	let before = simulation
		.animations()
		.pose(&key("clip-demo"))
		.unwrap()
		.clone();
	assert!(simulation.tick(0.15).is_err());
	assert_eq!(simulation.tick_index(), 0);
	let after = simulation.animations().pose(&key("clip-demo")).unwrap();
	assert_eq!(after.elapsed_ticks, before.elapsed_ticks);
	assert_eq!(after.revision, before.revision);
	assert_eq!(after.palette, before.palette);
	assert_eq!(after.local, before.local);
	assert_eq!(after.bounds, before.bounds);
	simulation.set_paused(false);
	simulation.tick(0.15).unwrap();
	let mut clean = SimulationSession::new(project(&data), data).unwrap();
	install(&mut clean, decoded());
	clean.tick(0.15).unwrap();
	let a = simulation.animations().pose(&key("clip-demo")).unwrap();
	let b = clean.animations().pose(&key("clip-demo")).unwrap();
	assert_eq!(a.elapsed_ticks, b.elapsed_ticks);
	assert_eq!(a.palette, b.palette);
	assert_eq!(a.bounds, b.bounds);
}

#[test]
fn instances_share_assets_but_advance_independent_clip_clocks() {
	let mut data = scene(true);
	let mut second = data.entities[0].clone();
	second.key = key("copy");
	second
		.animation
		.as_mut()
		.unwrap()
		.settings
		.speed_denominator = 2;
	data.entities.push(second);
	let prepared = animation_asset::prepare_scene(&root(), &data).unwrap();
	let mut simulation = SimulationSession::new(project(&data), Arc::new(data)).unwrap();
	simulation.install_animation_bindings(prepared).unwrap();
	for _ in 0..20 {
		simulation.tick(0.05).unwrap();
	}
	let normal = simulation.animations().pose(&key("clip-demo")).unwrap();
	let slow = simulation.animations().pose(&key("copy")).unwrap();
	assert!(Arc::ptr_eq(&normal.asset, &slow.asset));
	assert_eq!(normal.elapsed_ticks, 20);
	assert_eq!(slow.elapsed_ticks, 20);
	assert_ne!(normal.local, slow.local);
	assert_eq!(normal.local, sample_local(&normal.asset, 1, 0.).unwrap());
	assert_eq!(slow.local, sample_local(&slow.asset, 1, 0.5).unwrap());
}

#[test]
fn disabled_rest_pose_bounds_follow_accepted_owner_movement() {
	let mut data = scene(true);
	data.entities[0].animation.as_mut().unwrap().enabled = false;
	let mut configured = project(&data);
	configured.add_behavior(engine_core::project::BehaviorSpec::ControlledMove {
		entity: key("clip-demo"),
		speed: 1.,
		space: engine_core::project::BehaviorSpace::World,
	});
	let mut simulation = SimulationSession::new(configured, Arc::new(data)).unwrap();
	install(&mut simulation, decoded());
	let original = simulation
		.animations()
		.pose(&key("clip-demo"))
		.unwrap()
		.clone();
	simulation.set_controlled_input([1., 0., 0.]).unwrap();
	simulation.tick(0.05).unwrap();
	let moved = simulation.animations().pose(&key("clip-demo")).unwrap();
	assert_eq!(moved.local, original.local);
	assert_eq!(moved.elapsed_ticks, 0);
	close(moved.bounds.min()[0] - original.bounds.min()[0], 0.05);
	close(moved.bounds.max()[0] - original.bounds.max()[0], 0.05);
}

fn project(scene: &SceneData) -> ProjectData {
	let mut project = ProjectData::for_scene("Animation", "scene.json", scene).unwrap();
	project.fixed_hz = 20;
	project
}
fn install(session: &mut SimulationSession, asset: Arc<AnimationAsset>) {
	session
		.install_animation_bindings(BTreeMap::from([(key("clip-demo"), asset)]))
		.unwrap();
}
