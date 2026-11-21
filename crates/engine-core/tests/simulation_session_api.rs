#[path = "simulation_session_api/configuration.rs"]
mod configuration;
#[path = "simulation_session_api/input.rs"]
mod input;
#[path = "simulation_session_api/movement.rs"]
mod movement;
mod support;
use engine_core::{
	EngineBuilder,
	command::{EditorCommandDto as Command, EditorCommandRequest},
	project::{BehaviorSpace, BehaviorSpec, ProjectData, SceneBehavior},
	scene_catalog::{SceneDescriptor, SceneId},
	scene_data::{ColliderData, TransformData},
	simulation_session::{
		ActionWriter, ConfiguredBehaviorFactory, GameModule, GameModuleFactory, InputFrame,
		PreparedModuleConfig, SimulationAction, SimulationSession,
	},
	world::RuntimeWorld,
};
use std::sync::{Arc, Mutex};
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

fn scene_context_fixture() -> (
	ProjectData,
	SceneId,
	Arc<engine_core::SceneData>,
	Arc<engine_core::SceneData>,
) {
	let startup = Arc::new(scene(vec![
		entity("actor", [0.0; 3]),
		entity("startup-only", [0.0; 3]),
	]));
	let secondary = Arc::new(scene(vec![
		entity("actor", [10.0, 0.0, 0.0]),
		entity("secondary-only", [20.0, 0.0, 0.0]),
	]));
	let mut project = project(
		&startup,
		vec![
			controlled("actor", 1.0, BehaviorSpace::World),
			controlled("startup-only", 3.0, BehaviorSpace::World),
		],
	);
	let secondary_id = SceneId::new("secondary").unwrap();
	project
		.scenes
		.insert(SceneDescriptor::new(secondary_id.clone(), "scenes/secondary.json").unwrap())
		.unwrap();
	for (target, speed) in [("actor", 5.0), ("secondary-only", 2.0)] {
		project.behaviors.push(SceneBehavior {
			scene_id: secondary_id.clone(),
			behavior: controlled(target, speed, BehaviorSpace::World),
		});
	}
	(project, secondary_id, startup, secondary)
}

#[derive(Default)]
struct RecordingFactory {
	calls: Mutex<Vec<(ProjectData, SceneId)>>,
}
impl GameModuleFactory for RecordingFactory {
	fn create(
		&self,
		project: &ProjectData,
		scene_id: &SceneId,
	) -> Result<Box<dyn GameModule>, String> {
		self.calls
			.lock()
			.unwrap()
			.push((project.clone(), scene_id.clone()));
		ConfiguredBehaviorFactory.create(project, scene_id)
	}
}

struct RejectingFactory;
impl GameModuleFactory for RejectingFactory {
	fn create(
		&self,
		_project: &ProjectData,
		_scene_id: &SceneId,
	) -> Result<Box<dyn GameModule>, String> {
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

struct RepeatedActions(usize);
impl GameModuleFactory for RepeatedActions {
	fn create(&self, _: &ProjectData, _: &SceneId) -> Result<Box<dyn GameModule>, String> {
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
					interaction: Default::default(),
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

struct PreparedFactory {
	project: ProjectData,
	scene_id: SceneId,
	document: Arc<engine_core::SceneData>,
	calls: Mutex<usize>,
}
impl GameModuleFactory for PreparedFactory {
	fn create(&self, _: &ProjectData, _: &SceneId) -> Result<Box<dyn GameModule>, String> {
		panic!("The opted-in prepared path should receive validated constructor data")
	}
	fn create_prepared(
		&self,
		config: PreparedModuleConfig<'_>,
	) -> Result<Box<dyn GameModule>, String> {
		assert_eq!(config.project(), &self.project);
		assert_eq!(config.scene_id(), &self.scene_id);
		assert_eq!(config.document(), self.document.as_ref());
		assert_eq!(
			config.behaviors(),
			self.project.resolved_behaviors_for(&self.scene_id).unwrap()
		);
		*self.calls.lock().unwrap() += 1;
		ConfiguredBehaviorFactory.create_prepared(config)
	}
}

#[test]
fn catchup_changes_preserve_first_action_order_and_deduplicate_across_steps() {
	let data = Arc::new(scene(
		(0..32)
			.map(|index| entity(&format!("actor-{index}"), [0.0; 3]))
			.collect(),
	));
	let targets: Vec<_> = data
		.entities
		.iter()
		.rev()
		.map(|entity| entity.key.clone())
		.collect();
	let behaviors = targets
		.iter()
		.map(|entity| BehaviorSpec::Spin {
			entity: entity.clone(),
			angular_velocity: [0.0, 0.0, 1.0],
		})
		.collect();
	let mut session = SimulationSession::new(project(&data, behaviors), data).unwrap();
	let outcome = session.tick(3.0 / 60.0).unwrap();
	assert_eq!(outcome.steps, 3);
	assert_eq!(outcome.changes.changed_entities, targets);
	assert!(!outcome.changes.full_resync);
}
