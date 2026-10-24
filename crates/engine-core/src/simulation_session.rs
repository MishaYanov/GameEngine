//! One simulation pipeline for editor Play, the derived player and headless runs.
use crate::{
	EntityKey,
	collision::{CollisionStats, CollisionWorld},
	project::{BehaviorSpace, BehaviorSpec, ProjectData},
	scene_data::{SceneData, TransformData},
	simulation::FixedStepper,
	world::{RuntimeWorld, WorldChange},
};
use glam::Vec3;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};

#[derive(Default)]
pub struct InputFrame {
	movement: HashMap<EntityKey, [f32; 3]>,
}
impl InputFrame {
	pub fn movement(&self, entity: &EntityKey) -> [f32; 3] {
		self.movement.get(entity).copied().unwrap_or([0.0; 3])
	}
	pub fn is_idle(&self) -> bool {
		self.movement.is_empty()
	}
}
pub enum SimulationAction {
	Move {
		entity: EntityKey,
		world_delta: [f32; 3],
	},
	Rotate {
		entity: EntityKey,
		radians: [f32; 3],
	},
	SetTransform {
		entity: EntityKey,
		transform: TransformData,
	},
}
pub struct ActionWriter<'a> {
	actions: &'a mut Vec<SimulationAction>,
}
impl ActionWriter<'_> {
	pub const MAX_ACTIONS: usize = 4096;
	pub fn push(&mut self, action: SimulationAction) -> Result<(), String> {
		if self.actions.len() >= Self::MAX_ACTIONS {
			return Err("Simulation action budget exceeded".into());
		}
		self.actions.push(action);
		Ok(())
	}
}
/// Modules plan bounded actions from a read-only world. Mutation and collision
/// policy remain in the engine, so a game module cannot bypass world validation.
pub trait GameModule: Send {
	fn needs_tick(&self, input: &InputFrame) -> bool;
	fn plan(
		&self,
		world: &RuntimeWorld,
		input: &InputFrame,
		step: f32,
		output: &mut ActionWriter<'_>,
	) -> Result<(), String>;
}
pub trait GameModuleFactory: Send + Sync {
	fn create(&self, project: &ProjectData) -> Result<Box<dyn GameModule>, String>;
}
#[derive(Default)]
pub struct ConfiguredBehaviorFactory;
impl GameModuleFactory for ConfiguredBehaviorFactory {
	fn create(&self, project: &ProjectData) -> Result<Box<dyn GameModule>, String> {
		project.validate()?;
		Ok(Box::new(ProjectModule {
			behaviors: project.behaviors.clone(),
		}))
	}
}
struct ProjectModule {
	behaviors: Vec<BehaviorSpec>,
}
impl GameModule for ProjectModule {
	fn needs_tick(&self, input: &InputFrame) -> bool {
		self.behaviors.iter().any(|behavior| match behavior {
			BehaviorSpec::Spin {
				angular_velocity, ..
			} => *angular_velocity != [0.0; 3],
			BehaviorSpec::ControlledMove { entity, speed, .. } => {
				*speed > 0.0 && input.movement(entity) != [0.0; 3]
			}
		})
	}
	fn plan(
		&self,
		world: &RuntimeWorld,
		input: &InputFrame,
		step: f32,
		output: &mut ActionWriter<'_>,
	) -> Result<(), String> {
		for behavior in &self.behaviors {
			let entity = behavior.entity();
			let index = world
				.entity_index(entity)
				.ok_or("Configured behavior target is missing")?;
			match behavior {
				BehaviorSpec::ControlledMove {
					entity,
					speed,
					space,
				} => {
					let mut direction = Vec3::from_array(input.movement(entity));
					if direction == Vec3::ZERO || *speed == 0.0 {
						continue;
					}
					if *space == BehaviorSpace::Local {
						let length = direction.length();
						direction = world.world_matrices()[index]
							.transform_vector3(direction)
							.try_normalize()
							.ok_or("Local movement orientation is singular")?
							* length;
					}
					output.push(SimulationAction::Move {
						entity: entity.clone(),
						world_delta: (direction * *speed * step).to_array(),
					})?;
				}
				BehaviorSpec::Spin {
					entity,
					angular_velocity,
				} => {
					if *angular_velocity == [0.0; 3] {
						continue;
					}
					output.push(SimulationAction::Rotate {
						entity: entity.clone(),
						radians: angular_velocity.map(|velocity| velocity * step),
					})?;
				}
			}
		}
		Ok(())
	}
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TickOutcome {
	pub tick: u64,
	pub steps: u32,
	pub dropped_seconds: f64,
	pub changes: WorldChange,
}
pub struct SimulationSession {
	project: ProjectData,
	world: RuntimeWorld,
	collisions: CollisionWorld,
	clock: FixedStepper,
	module: Box<dyn GameModule>,
	inputs: InputFrame,
	paused: bool,
	tick: u64,
	actions: Vec<SimulationAction>,
}
impl SimulationSession {
	pub const MAX_ACTIONS_PER_TICK: usize = ActionWriter::MAX_ACTIONS;
	pub const MAX_MATRIX_UPDATES_PER_TICK: u64 = 1_000_000;
	pub const MAX_COLLISION_CANDIDATES_PER_TICK: usize = 1_000_000;

	pub fn new(project: ProjectData, document: Arc<SceneData>) -> Result<Self, String> {
		Self::with_factory(project, document, &ConfiguredBehaviorFactory)
	}
	pub fn with_factory(
		project: ProjectData,
		document: Arc<SceneData>,
		factory: &dyn GameModuleFactory,
	) -> Result<Self, String> {
		project.validate_scene(&document)?;
		let module = factory.create(&project)?;
		let world = RuntimeWorld::new(document)?;
		let collisions = CollisionWorld::new(&world)?;
		let clock = FixedStepper::new(project.fixed_hz, project.max_catchup_steps)?;
		Ok(Self {
			project,
			world,
			collisions,
			clock,
			module,
			inputs: InputFrame::default(),
			paused: false,
			tick: 0,
			actions: Vec::new(),
		})
	}
	pub fn project(&self) -> &ProjectData {
		&self.project
	}
	pub fn world(&self) -> &RuntimeWorld {
		&self.world
	}
	pub fn document(&self) -> Arc<SceneData> {
		self.world.document()
	}
	pub fn paused(&self) -> bool {
		self.paused
	}
	pub fn tick_index(&self) -> u64 {
		self.tick
	}
	pub fn needs_tick(&self) -> bool {
		!self.paused && self.module.needs_tick(&self.inputs)
	}
	pub fn collision_stats(&self) -> CollisionStats {
		self.collisions.stats()
	}
	pub fn clear_input(&mut self) {
		self.inputs.movement.clear();
		self.clock.reset();
	}
	pub fn set_paused(&mut self, paused: bool) {
		if self.paused != paused {
			self.paused = paused;
			self.clear_input();
		}
	}
	pub fn set_input(&mut self, entity: &EntityKey, movement: [f32; 3]) -> Result<bool, String> {
		if !self
			.project
			.behaviors
			.iter()
			.any(|b| matches!(b,BehaviorSpec::ControlledMove{entity:target,..}if target==entity))
		{
			return Err("Input target has no configured controlled movement behavior".into());
		}
		let movement = normalized_input(movement)?;
		let old = self.inputs.movement(entity);
		if old == movement {
			return Ok(false);
		}
		if old == [0.0; 3] || movement == [0.0; 3] {
			self.clock.reset();
		}
		if movement == [0.0; 3] {
			self.inputs.movement.remove(entity);
		} else {
			self.inputs.movement.insert(entity.clone(), movement);
		}
		Ok(true)
	}
	pub fn set_controlled_input(&mut self, movement: [f32; 3]) -> Result<bool, String> {
		let movement = normalized_input(movement)?;
		let targets = self
			.project
			.behaviors
			.iter()
			.filter_map(|b| match b {
				BehaviorSpec::ControlledMove { entity, .. } => Some(entity.clone()),
				_ => None,
			})
			.collect::<Vec<_>>();
		if targets.is_empty() && movement != [0.0; 3] {
			return Err(
				"Configure a controlled movement behavior before supplying movement".into(),
			);
		}
		let mut changed = false;
		for entity in targets {
			changed |= self.set_input(&entity, movement)?;
		}
		Ok(changed)
	}
	pub fn tick(&mut self, elapsed: f64) -> Result<TickOutcome, String> {
		if !elapsed.is_finite() || elapsed < 0.0 {
			return Err("Elapsed time must be finite and nonnegative".into());
		}
		let mut outcome = TickOutcome {
			tick: self.tick,
			..TickOutcome::default()
		};
		if self.paused || !self.needs_tick() {
			self.clock.reset();
			return Ok(outcome);
		}
		let mut clock = self.clock.clone();
		let report = clock.advance(elapsed)?;
		if self.tick.checked_add(report.steps as u64).is_none() {
			return Err("Simulation tick counter is exhausted".into());
		}
		let mut before = HashMap::<EntityKey, TransformData>::new();
		let mut actions_applied = 0;
		let initial_matrix_updates = self.world.matrix_updates();
		let mut collision_candidates_remaining = Self::MAX_COLLISION_CANDIDATES_PER_TICK;
		let result = (|| -> Result<(), String> {
			for _ in 0..report.steps {
				self.actions.clear();
				self.module.plan(
					&self.world,
					&self.inputs,
					report.step_seconds,
					&mut ActionWriter {
						actions: &mut self.actions,
					},
				)?;
				for action in self.actions.drain(..) {
					if actions_applied == Self::MAX_ACTIONS_PER_TICK {
						return Err("Simulation tick action budget exceeded".into());
					}
					if self
						.world
						.matrix_updates()
						.saturating_sub(initial_matrix_updates)
						>= Self::MAX_MATRIX_UPDATES_PER_TICK
					{
						return Err("Simulation tick matrix update budget exceeded".into());
					}
					// Count attempted actions too: blocked moves still perform
					// collision work and must not reset their allowance per step.
					actions_applied += 1;
					let entity = match &action {
						SimulationAction::Move { entity, .. }
						| SimulationAction::Rotate { entity, .. }
						| SimulationAction::SetTransform { entity, .. } => entity,
					};
					let index = self
						.world
						.entity_index(entity)
						.ok_or("Simulation action target is missing")?;
					before
						.entry(entity.clone())
						.or_insert_with(|| self.world.data().entities[index].transform.clone());
					let change = match action {
						SimulationAction::Move {
							entity,
							world_delta,
						} => {
							self.collisions
								.move_entity_with_budget(
									&mut self.world,
									&entity,
									world_delta,
									&mut collision_candidates_remaining,
								)?
								.changes
						}
						SimulationAction::Rotate { entity, radians } => {
							let mut transform = self.world.data().entities[index].transform.clone();
							for (angle, delta) in transform.rotation.iter_mut().zip(radians) {
								*angle = (*angle + delta).rem_euclid(std::f32::consts::TAU);
							}
							let changes = self.world.set_transform(&entity, transform)?;
							self.collisions.synchronize(&self.world, &changes)?;
							changes
						}
						SimulationAction::SetTransform { entity, transform } => {
							let changes = self.world.set_transform(&entity, transform)?;
							self.collisions.synchronize(&self.world, &changes)?;
							changes
						}
					};
					// A single validated action touches at most the scene's
					// 100k entities. Detect that final bounded overshoot before
					// applying another action, then restore the whole tick.
					if self
						.world
						.matrix_updates()
						.saturating_sub(initial_matrix_updates)
						> Self::MAX_MATRIX_UPDATES_PER_TICK
					{
						return Err("Simulation tick matrix update budget exceeded".into());
					}
					outcome.changes.merge(change);
				}
			}
			Ok(())
		})();
		if let Err(error) = result {
			// Cold failure path restores only transforms touched by planned
			// actions. A full document copy is never retained per steady tick.
			let mut restored = self.world.data().clone();
			for entity in &mut restored.entities {
				if let Some(transform) = before.remove(&entity.key) {
					entity.transform = transform;
				}
			}
			self.world = RuntimeWorld::new(Arc::new(restored))?;
			self.collisions = CollisionWorld::new(&self.world)?;
			self.paused = true;
			self.clear_input();
			self.actions.clear();
			return Err(format!("Simulation paused after rejected actions: {error}"));
		}
		self.clock = clock;
		self.tick += report.steps as u64;
		outcome.tick = self.tick;
		outcome.steps = report.steps;
		outcome.dropped_seconds = report.dropped_seconds;
		Ok(outcome)
	}
}
fn normalized_input(movement: [f32; 3]) -> Result<[f32; 3], String> {
	if !movement.iter().all(|v| v.is_finite()) {
		return Err("Movement input must be finite".into());
	}
	let length = movement
		.iter()
		.map(|v| (*v as f64).powi(2))
		.sum::<f64>()
		.sqrt()
		.max(1.0);
	Ok(movement.map(|v| (v as f64 / length) as f32))
}
