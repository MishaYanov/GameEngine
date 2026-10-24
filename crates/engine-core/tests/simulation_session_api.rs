mod support;
use engine_core::{
	project::{BehaviorSpace, BehaviorSpec, ProjectData},
	scene_data::{ColliderData, TransformData},
	simulation_session::{
		ActionWriter, GameModule, GameModuleFactory, InputFrame, SimulationAction,
		SimulationSession,
	},
	world::RuntimeWorld,
};
use std::sync::Arc;
use support::*;
fn project(data: &engine_core::SceneData, behaviors: Vec<BehaviorSpec>) -> ProjectData {
	let mut p = ProjectData::for_scene("Project", "scenes/main.json", data).unwrap();
	for behavior in behaviors {
		p.add_behavior(behavior);
	}
	p
}
fn controlled(target: &str, speed: f32, space: BehaviorSpace) -> BehaviorSpec {
	BehaviorSpec::ControlledMove {
		entity: key(target),
		speed,
		space,
	}
}

#[test]
fn explicit_behavior_target_moves_without_selection_demo_bindings_or_implicit_animation() {
	let data = Arc::new(scene(vec![
		entity("uncontrolled", [2.0, 0.0, 0.0]),
		entity("actor", [0.0; 3]),
	]));
	let mut session = SimulationSession::new(
		project(&data, vec![controlled("actor", 2.0, BehaviorSpace::World)]),
		Arc::clone(&data),
	)
	.unwrap();
	assert!(!session.needs_tick());
	assert!(session.set_input(&key("actor"), [3.0, 4.0, 0.0]).unwrap());
	assert!(!session.set_input(&key("actor"), [3.0, 4.0, 0.0]).unwrap());
	let outcome = session.tick(0.1).unwrap();
	assert_eq!(outcome.steps, 6);
	assert_eq!(outcome.tick, 6);
	close(session.document().entities[1].transform.position[0], 0.12);
	close(session.document().entities[1].transform.position[1], 0.16);
	assert_eq!(session.document().entities[0], data.entities[0]);
	assert_eq!(data.entities[1].transform.position, [0.0; 3]);
	assert_eq!(outcome.changes.changed_entities, vec![key("actor")]);
	assert!(
		session
			.set_input(&key("uncontrolled"), [1.0, 0.0, 0.0])
			.is_err()
	);
	let mut empty = SimulationSession::new(project(&data, vec![]), Arc::clone(&data)).unwrap();
	assert!(!empty.needs_tick());
	assert!(empty.set_controlled_input([1.0, 0.0, 0.0]).is_err());
	assert_eq!(empty.tick(1.0).unwrap().steps, 0);
	assert_eq!(empty.document(), data);
}

#[test]
fn idle_pause_and_input_release_discard_partial_ticks_and_do_not_replay_elapsed_time() {
	let data = Arc::new(scene(vec![entity("actor", [0.0; 3])]));
	let mut session = SimulationSession::new(
		project(&data, vec![controlled("actor", 1.0, BehaviorSpace::World)]),
		data,
	)
	.unwrap();
	session.tick(60.0).unwrap();
	session.set_controlled_input([1.0, 0.0, 0.0]).unwrap();
	assert_eq!(session.tick(1.0 / 120.0).unwrap().steps, 0);
	session.set_paused(true);
	assert!(!session.needs_tick());
	assert_eq!(session.tick(60.0).unwrap().steps, 0);
	session.set_paused(false);
	assert!(!session.needs_tick());
	session.set_controlled_input([1.0, 0.0, 0.0]).unwrap();
	assert_eq!(session.tick(1.0 / 120.0).unwrap().steps, 0);
	assert_eq!(session.tick(1.0 / 120.0).unwrap().steps, 1);
	close(
		session.document().entities[0].transform.position[0],
		1.0 / 60.0,
	);
	session.set_controlled_input([0.0; 3]).unwrap();
	session.tick(60.0).unwrap();
	session.set_controlled_input([1.0, 0.0, 0.0]).unwrap();
	assert_eq!(session.tick(1.0 / 120.0).unwrap().steps, 0);
	let outcome = session.tick(1.0).unwrap();
	assert_eq!(outcome.steps, 8);
	assert!(outcome.dropped_seconds > 0.8);
}

#[test]
fn local_and_world_movement_respect_rotated_scaled_parent_without_changing_speed() {
	let mut parent = entity("parent", [0.0; 3]);
	parent.transform.rotation[2] = std::f32::consts::FRAC_PI_2;
	parent.transform.scale = [2.0; 3];
	let data = Arc::new(scene(vec![
		parent,
		entity("actor", [0.0; 3]).with_parent(Some(key("parent"))),
	]));
	for space in [BehaviorSpace::Local, BehaviorSpace::World] {
		let mut session = SimulationSession::new(
			project(&data, vec![controlled("actor", 2.0, space)]),
			Arc::clone(&data),
		)
		.unwrap();
		session.set_controlled_input([1.0, 0.0, 0.0]).unwrap();
		session.tick(0.1).unwrap();
		let world = session.world().world_matrices()[1].w_axis.truncate();
		close(world.length(), 0.2);
		match space {
			BehaviorSpace::Local => {
				close(world.x, 0.0);
				close(world.y, 0.2);
			}
			BehaviorSpace::World => {
				close(world.x, 0.2);
				close(world.y, 0.0);
			}
		}
	}
}

#[test]
fn configured_move_and_spin_compose_on_the_same_entity_instead_of_overwriting_motion() {
	let data = Arc::new(scene(vec![entity("actor", [0.0; 3])]));
	for reversed in [false, true] {
		let mut behaviors = vec![
			controlled("actor", 2.0, BehaviorSpace::World),
			BehaviorSpec::Spin {
				entity: key("actor"),
				angular_velocity: [0.0, 0.0, 1.0],
			},
		];
		if reversed {
			behaviors.reverse();
		}
		let mut session =
			SimulationSession::new(project(&data, behaviors), Arc::clone(&data)).unwrap();
		session.set_controlled_input([1.0, 0.0, 0.0]).unwrap();
		session.tick(0.1).unwrap();
		close(session.document().entities[0].transform.position[0], 0.2);
		close(session.document().entities[0].transform.rotation[2], 0.1);
	}
}

#[test]
fn shared_simulation_uses_persistent_swept_collision_and_keeps_blocked_actor_stationary() {
	let collider = Some(ColliderData::Box {
		half_extents: [0.5; 3],
		offset: [0.0; 3],
	});
	let data = Arc::new(scene(vec![
		entity("actor", [-2.0, 0.0, 0.0]).with_collider(collider.clone()),
		entity("wall", [0.0; 3]).with_collider(collider),
	]));
	let mut session = SimulationSession::new(
		project(
			&data,
			vec![controlled("actor", 240.0, BehaviorSpace::World)],
		),
		Arc::clone(&data),
	)
	.unwrap();
	session.set_controlled_input([1.0, 0.0, 0.0]).unwrap();
	let stats = session.collision_stats();
	let outcome = session.tick(1.0 / 60.0).unwrap();
	assert!(outcome.changes.is_empty());
	assert_eq!(session.document(), data);
	assert_eq!(session.collision_stats().rebuilds, stats.rebuilds);
	assert_eq!(
		session.collision_stats().updated_colliders,
		stats.updated_colliders
	);
}

struct RejectingFactory;
impl GameModuleFactory for RejectingFactory {
	fn create(&self, _project: &ProjectData) -> Result<Box<dyn GameModule>, String> {
		Ok(Box::new(RejectingModule))
	}
}
struct RejectingModule;
impl GameModule for RejectingModule {
	fn needs_tick(&self, _input: &InputFrame) -> bool {
		true
	}
	fn plan(
		&self,
		_world: &RuntimeWorld,
		_input: &InputFrame,
		_step: f32,
		out: &mut ActionWriter<'_>,
	) -> Result<(), String> {
		out.push(SimulationAction::Move {
			entity: key("actor"),
			world_delta: [1.0, 0.0, 0.0],
		})?;
		out.push(SimulationAction::SetTransform {
			entity: key("actor"),
			transform: TransformData {
				position: [f32::NAN; 3],
				..Default::default()
			},
		})
	}
}
#[test]
fn rejected_module_action_rolls_back_earlier_actions_and_pauses_without_advancing_tick() {
	let data = Arc::new(scene(vec![entity("actor", [0.0; 3])]));
	let mut session = SimulationSession::with_factory(
		project(&data, vec![]),
		Arc::clone(&data),
		&RejectingFactory,
	)
	.unwrap();
	assert!(session.tick(0.1).is_err());
	assert!(session.paused());
	assert_eq!(session.document(), data);
	assert_eq!(session.tick_index(), 0);
	assert!(!session.needs_tick());
	assert_eq!(session.tick(1.0).unwrap().steps, 0);
}

#[test]
fn invalid_input_and_elapsed_are_atomic_and_extreme_finite_input_is_normalized() {
	let data = Arc::new(scene(vec![entity("actor", [0.0; 3])]));
	let mut session = SimulationSession::new(
		project(&data, vec![controlled("actor", 1.0, BehaviorSpace::World)]),
		Arc::clone(&data),
	)
	.unwrap();
	assert!(session.set_input(&key("actor"), [f32::NAN; 3]).is_err());
	assert!(!session.needs_tick());
	for elapsed in [-1.0, f64::NAN, f64::INFINITY] {
		assert!(session.tick(elapsed).is_err());
		assert_eq!(session.document(), data);
		assert_eq!(session.tick_index(), 0);
	}
	session
		.set_input(&key("actor"), [f32::MAX, f32::MAX, 0.0])
		.unwrap();
	session.tick(0.1).unwrap();
	let position = glam::Vec3::from_array(session.document().entities[0].transform.position);
	close(position.length(), 0.1);
}

struct RepeatedActions(usize);
impl GameModuleFactory for RepeatedActions {
	fn create(&self, _: &ProjectData) -> Result<Box<dyn GameModule>, String> {
		Ok(Box::new(Self(self.0)))
	}
}
impl GameModule for RepeatedActions {
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
		for _ in 0..self.0 {
			out.push(SimulationAction::Move {
				entity: key("actor"),
				world_delta: [0.001, 0.0, 0.0],
			})?;
		}
		Ok(())
	}
}

#[test]
fn action_budget_is_shared_across_catchup_steps_and_overflow_restores_the_entire_tick() {
	for (count, elapsed) in [(4097, 1.0 / 60.0), (3000, 2.0 / 60.0)] {
		let data = Arc::new(scene(vec![entity("actor", [0.0; 3])]));
		let mut session = SimulationSession::with_factory(
			project(&data, vec![]),
			Arc::clone(&data),
			&RepeatedActions(count),
		)
		.unwrap();
		let error = session.tick(elapsed).unwrap_err();
		assert!(error.contains("action budget"), "{error}");
		assert!(session.paused());
		assert_eq!(session.tick_index(), 0);
		assert_eq!(session.document(), data);
		assert!(!session.needs_tick());
	}
}

#[test]
fn aggregate_subtree_matrix_and_collision_work_limits_restore_all_earlier_moves() {
	for collision_work in [false, true] {
		let mut entities = vec![entity("actor", [0.0; 3])];
		for i in 0..1000 {
			let mut child = entity(&format!("child-{i}"), [0.0, i as f32 * 2.0, 0.0]);
			child.parent = Some(key("actor"));
			entities.push(child);
		}
		if collision_work {
			// One unrelated collider makes subtree lookups consume the shared
			// collision allowance, without causing an early blocked move.
			entities.push(
				entity("distant", [10000.0; 3]).with_collider(Some(ColliderData::Box {
					half_extents: [0.5; 3],
					offset: [0.0; 3],
				})),
			);
		}
		let data = Arc::new(scene(entities));
		let mut session = SimulationSession::with_factory(
			project(&data, vec![]),
			Arc::clone(&data),
			&RepeatedActions(1001),
		)
		.unwrap();
		let error = session.tick(1.0 / 60.0).unwrap_err();
		let expected = if collision_work {
			"Collision movement work budget"
		} else {
			"matrix update budget"
		};
		assert!(error.contains(expected), "{error}");
		assert!(session.paused());
		assert_eq!(session.tick_index(), 0);
		assert_eq!(session.document(), data);
	}
}
