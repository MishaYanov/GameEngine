mod support;
use engine_core::{
	SceneData,
	game_progress::{GameProgress, ProgressState},
	game_session::GameSession,
	project::{BehaviorSpace, BehaviorSpec, ProjectData},
	scene_catalog::{SceneDescriptor, SceneId},
	scene_data::{ColliderData, ColliderInteraction, MaterialOverrideData},
	simulation_session::{
		ActionWriter, GameModule, GameModuleFactory, GameplayContext, InputFrame, SimulationAction,
		SimulationSession,
	},
	triggers::TriggerPhase,
	world::RuntimeWorld,
};
use std::sync::{
	Arc,
	atomic::{AtomicBool, Ordering},
};
use support::*;

fn fixture() -> (ProjectData, Arc<SceneData>) {
	let mut actor = entity("actor", [0.0; 3]);
	actor.collider = Some(ColliderData::Sphere {
		radius: 0.25,
		offset: [0.0; 3],
		interaction: ColliderInteraction::default(),
	});
	let mut sensor = entity("sensor", [0.0; 3]);
	sensor.collider = Some(ColliderData::Box {
		half_extents: [0.5; 3],
		offset: [0.0; 3],
		interaction: ColliderInteraction {
			sensor: true,
			..Default::default()
		},
	});
	let data = Arc::new(scene(vec![actor, sensor]));
	let mut project = ProjectData::for_scene_with_id(
		"Triggers",
		SceneId::new("first").unwrap(),
		"scenes/first.json",
		&data,
	)
	.unwrap();
	project.fixed_hz = 10;
	project.add_behavior(BehaviorSpec::ControlledMove {
		entity: key("actor"),
		speed: 10.0,
		space: BehaviorSpace::World,
	});
	(project, data)
}

#[test]
fn idle_sensor_samples_once_and_pause_preserves_history_while_restart_starts_fresh() {
	let (project, data) = fixture();
	let mut simulation = SimulationSession::new(project.clone(), data.clone()).unwrap();
	assert!(simulation.needs_tick());
	assert!(simulation.tick(0.05).unwrap().trigger_events.is_empty());
	let first = simulation.tick(0.05).unwrap();
	assert_eq!(first.trigger_events.len(), 1);
	let event = &first.trigger_events[0];
	assert_eq!(
		(
			event.phase,
			event.sensor.as_str(),
			event.other.as_str(),
			event.scene_id.as_str(),
			event.step
		),
		(TriggerPhase::Enter, "sensor", "actor", "first", 1)
	);
	assert!(!simulation.needs_tick());
	assert_eq!(simulation.tick(30.0).unwrap().steps, 0);
	simulation.set_paused(true);
	assert!(simulation.tick(1.0).unwrap().trigger_events.is_empty());
	simulation.set_paused(false);
	assert!(!simulation.needs_tick());
	assert_eq!(
		SimulationSession::new(project, data)
			.unwrap()
			.tick(0.1)
			.unwrap()
			.trigger_events
			.len(),
		1
	);
}

#[test]
fn final_movement_is_sampled_after_release_and_exit_is_emitted_once() {
	let (project, data) = fixture();
	let mut simulation = SimulationSession::new(project, data).unwrap();
	simulation.tick(0.1).unwrap();
	simulation.set_controlled_input([1.0, 0.0, 0.0]).unwrap();
	assert!(simulation.tick(0.1).unwrap().trigger_events.is_empty());
	simulation.clear_input();
	assert!(simulation.needs_tick());
	let outcome = simulation.tick(0.1).unwrap();
	assert_eq!(outcome.trigger_events.len(), 1);
	assert_eq!(outcome.trigger_events[0].phase, TriggerPhase::Exit);
	assert!(!simulation.needs_tick());
	assert_eq!(simulation.tick(10.0).unwrap().steps, 0);
}

#[test]
fn ordering_and_fixed_step_results_ignore_entity_order_and_elapsed_partition() {
	let (project, mut data) = fixture();
	let mut second = data.entities[1].clone();
	second.key = key("a-sensor");
	Arc::make_mut(&mut data).entities.push(second);
	let mut reordered = (*data).clone();
	reordered.entities.reverse();
	let mut first = SimulationSession::new(project.clone(), data).unwrap();
	let mut second = SimulationSession::new(project, Arc::new(reordered)).unwrap();
	first.set_controlled_input([1.0, 0.0, 0.0]).unwrap();
	second.set_controlled_input([1.0, 0.0, 0.0]).unwrap();
	let all = first.tick(0.3).unwrap().trigger_events;
	let mut split = Vec::new();
	for _ in 0..3 {
		split.extend(second.tick(0.1).unwrap().trigger_events);
	}
	assert_eq!(all, split);
	assert_eq!(all[0].sensor.as_str(), "a-sensor");
	// Sensor/sensor contributes one directed Enter for each sensor.
	assert_eq!(
		all.iter()
			.filter(|event| event.phase == TriggerPhase::Enter)
			.count(),
		4
	);
	assert_eq!(
		all.iter()
			.filter(|event| event.phase == TriggerPhase::Exit)
			.count(),
		2
	);
}

#[derive(Clone, Default, PartialEq)]
struct Count(u32);
impl GameProgress for Count {
	type Action = ();
	fn validate(&self) -> Result<(), String> {
		Ok(())
	}
	fn retained_bytes(&self) -> usize {
		4
	}
	fn action_bytes(_: &()) -> usize {
		0
	}
	fn apply(&mut self, _: &()) -> Result<(), String> {
		self.0 += 1;
		Ok(())
	}
}
struct Actions(Arc<AtomicBool>);
impl GameModuleFactory for Actions {
	fn create_progress(&self) -> Result<ProgressState, String> {
		ProgressState::new(Count::default())
	}
	fn create(&self, _: &ProjectData, _: &SceneId) -> Result<Box<dyn GameModule>, String> {
		Ok(Box::new(Self(self.0.clone())))
	}
}
impl GameModule for Actions {
	fn needs_tick(&self, _: &InputFrame) -> bool {
		false
	}
	fn plan(
		&self,
		_: &RuntimeWorld,
		_: &InputFrame,
		_: f32,
		_: &mut ActionWriter<'_>,
	) -> Result<(), String> {
		Ok(())
	}
	fn plan_with_progress(
		&self,
		context: GameplayContext<'_>,
		output: &mut ActionWriter<'_>,
	) -> Result<(), String> {
		for event in context.triggers {
			output.push_progress::<Count>(())?;
			if event.phase == TriggerPhase::Enter {
				output.push(SimulationAction::SetVisible {
					entity: key("sensor"),
					visible: false,
				})?;
				output.push(SimulationAction::SetCollisionEnabled {
					entity: key("sensor"),
					enabled: false,
				})?;
				output.push(SimulationAction::SetMaterial {
					entity: key("sensor"),
					material: Some(MaterialOverrideData {
						base_color_factor: [0.0, 1.0, 0.0, 1.0],
					}),
				})?;
			} else if self.0.load(Ordering::Relaxed) {
				output.push(SimulationAction::SetVisible {
					entity: key("missing"),
					visible: false,
				})?;
			}
		}
		Ok(())
	}
}

#[test]
fn later_catchup_failure_restores_properties_progress_and_trigger_history_then_retry_emits_enter() {
	let (project, data) = fixture();
	let reject = Arc::new(AtomicBool::new(true));
	let mut game = GameSession::with_factory_for_scene(
		Arc::new(project),
		SceneId::new("first").unwrap(),
		data.clone(),
		Arc::new(Actions(reject.clone())),
	)
	.unwrap();
	assert!(game.tick(0.2).unwrap_err().contains("target is missing"));
	assert_eq!(game.active().document().as_ref(), data.as_ref());
	assert_eq!(game.progress::<Count>().unwrap().0, 0);
	assert_eq!(game.active().tick_index(), 0);
	assert!(game.active().paused());
	reject.store(false, Ordering::Relaxed);
	game.active_mut().set_paused(false);
	let accepted = game.tick(0.2).unwrap();
	assert_eq!(
		accepted
			.trigger_events
			.iter()
			.map(|event| event.phase)
			.collect::<Vec<_>>(),
		[TriggerPhase::Enter, TriggerPhase::Exit]
	);
	assert_eq!(game.progress::<Count>().unwrap().0, 2);
	let runtime = game.active().document();
	assert!(!runtime.entities[1].renderable);
	assert!(!runtime.entities[1].collision_enabled);
	assert_eq!(
		runtime.entities[1]
			.material
			.as_ref()
			.unwrap()
			.base_color_factor,
		[0.0, 1.0, 0.0, 1.0]
	);
	assert!(data.entities[1].renderable && data.entities[1].collision_enabled);
	let restart = game
		.prepare_new_game(SceneId::new("first").unwrap(), data.clone())
		.unwrap();
	game.commit_scene(restart).unwrap();
	assert_eq!(game.active().document().as_ref(), data.as_ref());
	assert_eq!(game.progress::<Count>().unwrap().0, 0);
	assert_eq!(
		game.tick(0.1).unwrap().trigger_events[0].phase,
		TriggerPhase::Enter
	);
}

#[test]
fn prepared_scene_and_failed_scene_never_change_active_overlap_history() {
	let (mut project, data) = fixture();
	project
		.scenes
		.insert(
			SceneDescriptor::new(SceneId::new("second").unwrap(), "scenes/second.json").unwrap(),
		)
		.unwrap();
	let mut game = GameSession::with_factory_for_scene(
		Arc::new(project),
		SceneId::new("first").unwrap(),
		data.clone(),
		Arc::new(engine_core::simulation_session::ConfiguredBehaviorFactory),
	)
	.unwrap();
	assert_eq!(game.tick(0.1).unwrap().trigger_events.len(), 1);
	let mut invalid = (*data).clone();
	invalid.entities[1].key = invalid.entities[0].key.clone();
	assert!(
		game.prepare_scene(SceneId::new("second").unwrap(), Arc::new(invalid))
			.is_err()
	);
	let prepared = game
		.prepare_scene(SceneId::new("second").unwrap(), data)
		.unwrap();
	assert_eq!(game.tick(0.1).unwrap().steps, 0);
	game.commit_scene(prepared).unwrap();
	let event = game.tick(0.1).unwrap().trigger_events.remove(0);
	assert_eq!(
		(event.phase, event.scene_id.as_str(), event.step),
		(TriggerPhase::Enter, "second", 1)
	);
}

#[test]
fn collision_filters_and_disabled_sensors_do_not_generate_edges() {
	for disabled in [false, true] {
		let (project, mut data) = fixture();
		let sensor = &mut Arc::make_mut(&mut data).entities[1];
		if disabled {
			sensor.collision_enabled = false;
		} else {
			sensor.collider = Some(ColliderData::Sphere {
				radius: 1.0,
				offset: [0.0; 3],
				interaction: ColliderInteraction {
					sensor: true,
					mask: 0,
					..Default::default()
				},
			});
		}
		let mut game = SimulationSession::new(project, data).unwrap();
		assert!(game.tick(0.1).unwrap().trigger_events.is_empty());
		assert!(!game.needs_tick());
	}
}

#[test]
fn sensor_sampling_charges_filtered_solid_candidates_and_rejects_without_partial_commit() {
	let (project, mut data) = fixture();
	let data_mut = Arc::make_mut(&mut data);
	data_mut.entities.clear();
	for index in 0..1500 {
		let mut value = entity(&format!("candidate-{index}"), [0.0; 3]);
		value.collider = Some(ColliderData::Sphere {
			radius: 1.0,
			offset: [0.0; 3],
			interaction: ColliderInteraction {
				sensor: index == 0,
				mask: 0,
				..Default::default()
			},
		});
		data_mut.entities.push(value);
	}
	let mut project = project;
	project.behaviors.clear();
	let mut game = SimulationSession::new(project, data.clone()).unwrap();
	let error = game.tick(0.1).unwrap_err();
	assert!(error.contains("broadphase candidates"), "{error}");
	assert_eq!(game.tick_index(), 0);
	assert!(game.paused());
	assert_eq!(game.document(), data);
}

#[test]
fn runtime_material_validation_is_atomic_and_visibility_does_not_disable_collision() {
	let (_, data) = fixture();
	let mut world = RuntimeWorld::new(data.clone()).unwrap();
	let before = world.document();
	assert!(
		world
			.set_material(
				&key("actor"),
				Some(MaterialOverrideData {
					base_color_factor: [f32::NAN, 0.0, 0.0, 1.0]
				})
			)
			.is_err()
	);
	assert_eq!(world.document(), before);
	let change = world.set_visible(&key("actor"), false).unwrap();
	assert!(change.topology_changed);
	assert!(!world.data().entities[0].renderable);
	assert!(world.data().entities[0].collision_enabled);
	assert_eq!(
		engine_core::collision::CollisionWorld::new(&world)
			.unwrap()
			.query_overlaps()
			.unwrap()
			.len(),
		1
	);
	assert!(world.set_visible(&key("actor"), false).unwrap().is_empty());
}

#[test]
fn exits_precede_enters_even_when_the_new_sensor_sorts_first() {
	let (mut project, mut data) = fixture();
	let data_mut = Arc::make_mut(&mut data);
	data_mut.entities[1].key = key("z-sensor");
	let mut next = data_mut.entities[1].clone();
	next.key = key("a-sensor");
	next.transform.position = [2.0, 0.0, 0.0];
	data_mut.entities.push(next);
	project.behaviors.clear();
	project.add_behavior(BehaviorSpec::ControlledMove {
		entity: key("actor"),
		speed: 20.0,
		space: BehaviorSpace::World,
	});
	let mut game = SimulationSession::new(project, data).unwrap();
	game.set_controlled_input([1.0, 0.0, 0.0]).unwrap();
	game.tick(0.1).unwrap();
	game.clear_input();
	let events = game.tick(0.1).unwrap().trigger_events;
	assert_eq!(
		events
			.iter()
			.map(|event| (event.phase, event.sensor.as_str()))
			.collect::<Vec<_>>(),
		[
			(TriggerPhase::Exit, "z-sensor"),
			(TriggerPhase::Enter, "a-sensor")
		]
	);
}

struct ShowHidden;
impl GameModuleFactory for ShowHidden {
	fn create_progress(&self) -> Result<ProgressState, String> {
		ProgressState::new(Count::default())
	}
	fn create(&self, _: &ProjectData, _: &SceneId) -> Result<Box<dyn GameModule>, String> {
		Ok(Box::new(Self))
	}
}
impl GameModule for ShowHidden {
	fn needs_tick(&self, _: &InputFrame) -> bool {
		true
	}
	fn plan(
		&self,
		_: &RuntimeWorld,
		_: &InputFrame,
		_: f32,
		output: &mut ActionWriter<'_>,
	) -> Result<(), String> {
		output.push_progress::<Count>(())?;
		output.push(SimulationAction::Move {
			entity: key("actor"),
			world_delta: [1.0, 0.0, 0.0],
		})?;
		output.push(SimulationAction::SetVisible {
			entity: key("sensor"),
			visible: true,
		})
	}
}

#[test]
fn native_visibility_capability_rejects_unprepared_models_before_committing_the_tick() {
	let (project, mut data) = fixture();
	Arc::make_mut(&mut data).entities[1].renderable = false;
	let mut game = GameSession::with_factory_for_scene(
		Arc::new(project),
		SceneId::new("first").unwrap(),
		data.clone(),
		Arc::new(ShowHidden),
	)
	.unwrap();
	assert!(!game.active().has_visibility_capabilities());
	game.active_mut()
		.set_visibility_capabilities([key("actor")].into_iter().collect())
		.unwrap();
	assert!(
		game.tick(0.1)
			.unwrap_err()
			.contains("render model was not prepared")
	);
	assert_eq!(game.active().document(), data);
	assert_eq!(game.progress::<Count>().unwrap().0, 0);
	assert_eq!(game.active().tick_index(), 0);
	game.active_mut()
		.set_visibility_capabilities([key("actor"), key("sensor")].into_iter().collect())
		.unwrap();
	assert!(
		game.active_mut()
			.set_visibility_capabilities([key("missing")].into_iter().collect())
			.is_err()
	);
	game.active_mut().set_paused(false);
	game.tick(0.1).unwrap();
	assert!(game.active().world().data().entities[1].renderable);
	assert_eq!(game.progress::<Count>().unwrap().0, 1);
	let restart = game
		.prepare_new_game(SceneId::new("first").unwrap(), data)
		.unwrap();
	game.commit_scene(restart).unwrap();
	assert!(!game.active().has_visibility_capabilities());
}
