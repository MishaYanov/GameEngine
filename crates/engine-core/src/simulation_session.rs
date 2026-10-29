//! One simulation pipeline for editor Play, the derived player and headless runs.
use crate::game_progress::{GameProgress, ProgressAction, ProgressState, ProgressView};
use crate::scene_catalog::SceneId;
use crate::{
	EntityKey,
	collision::{CollisionStats, CollisionWorld},
	project::{BehaviorSettings, BehaviorSpace, ProjectData, ResolvedBehavior},
	scene_data::{SceneData, TransformData},
	simulation::FixedStepper,
	world::{ChangeAccumulator, RuntimeWorld, WorldChange},
};
use glam::Vec3;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::{collections::HashMap, sync::Arc};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum MovementTarget {
	#[default]
	AllControlled,
	Entity(EntityKey),
}

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
	Progress(ProgressAction),
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
	pub fn push_progress<P: GameProgress>(&mut self, action: P::Action) -> Result<(), String> {
		self.push(SimulationAction::Progress(ProgressAction::new::<P>(
			action,
		)?))
	}
	pub fn push(&mut self, action: SimulationAction) -> Result<(), String> {
		if self.actions.len() >= Self::MAX_ACTIONS {
			return Err("Simulation action budget exceeded".into());
		}
		if matches!(&action, SimulationAction::Progress(_))
			&& self
				.actions
				.iter()
				.filter(|action| matches!(action, SimulationAction::Progress(_)))
				.count() >= crate::game_progress::MAX_PROGRESS_ACTIONS
		{
			return Err("Game progress action budget exceeded".into());
		}
		self.actions.push(action);
		Ok(())
	}
}
/// Modules plan bounded actions from a read-only world. Mutation and collision
/// policy remain in the engine, so a game module cannot bypass world validation.
pub struct GameplayContext<'a> {
	pub world: &'a RuntimeWorld,
	pub input: &'a InputFrame,
	pub step_seconds: f32,
	pub progress: ProgressView<'a>,
}

pub trait GameModule: Send {
	fn needs_tick_with_progress(
		&self,
		_world: &RuntimeWorld,
		input: &InputFrame,
		_progress: ProgressView<'_>,
	) -> bool {
		self.needs_tick(input)
	}
	fn plan_with_progress(
		&self,
		context: GameplayContext<'_>,
		output: &mut ActionWriter<'_>,
	) -> Result<(), String> {
		self.plan(context.world, context.input, context.step_seconds, output)
	}
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
	/// Called once per game run, never while preparing an ordinary scene change.
	fn create_progress(&self) -> Result<ProgressState, String> {
		Ok(ProgressState::empty())
	}
	fn create(
		&self,
		project: &ProjectData,
		scene_id: &SceneId,
	) -> Result<Box<dyn GameModule>, String>;

	/// Called after the engine validates the project, scene and behavior targets.
	/// Existing factories retain their original checked creation path by default.
	fn create_prepared(
		&self,
		config: PreparedModuleConfig<'_>,
	) -> Result<Box<dyn GameModule>, String> {
		self.create(config.project, config.scene_id)
	}
}

/// Engine-prepared module input. Only the engine can construct this validated
/// context; custom factories can inspect it or keep implementing `create` only.
pub struct PreparedModuleConfig<'a> {
	project: &'a ProjectData,
	scene_id: &'a SceneId,
	behaviors: Vec<ResolvedBehavior>,
}
impl PreparedModuleConfig<'_> {
	pub fn project(&self) -> &ProjectData {
		self.project
	}
	pub fn scene_id(&self) -> &SceneId {
		self.scene_id
	}
	pub fn behaviors(&self) -> &[ResolvedBehavior] {
		&self.behaviors
	}
}

#[derive(Default)]
pub struct ConfiguredBehaviorFactory;
impl GameModuleFactory for ConfiguredBehaviorFactory {
	fn create(
		&self,
		project: &ProjectData,
		scene_id: &SceneId,
	) -> Result<Box<dyn GameModule>, String> {
		Ok(Box::new(ProjectModule::new(
			project.resolved_behaviors_for(scene_id)?,
		)))
	}

	fn create_prepared(
		&self,
		config: PreparedModuleConfig<'_>,
	) -> Result<Box<dyn GameModule>, String> {
		Ok(Box::new(ProjectModule::new(config.behaviors)))
	}
}
struct ProjectModule {
	behaviors: Vec<ResolvedBehavior>,
	has_nonzero_spin: bool,
}
impl ProjectModule {
	fn new(behaviors: Vec<ResolvedBehavior>) -> Self {
		let has_nonzero_spin = behaviors.iter().any(|behavior| {
			matches!(&behavior.settings, BehaviorSettings::Spin { angular_velocity } if *angular_velocity != [0.0; 3])
		});
		Self {
			behaviors,
			has_nonzero_spin,
		}
	}
}
impl GameModule for ProjectModule {
	fn needs_tick(&self, input: &InputFrame) -> bool {
		if self.has_nonzero_spin {
			return true;
		}
		if input.is_idle() {
			return false;
		}
		self.behaviors
			.iter()
			.any(|behavior| match &behavior.settings {
				BehaviorSettings::Spin { .. } => false,
				BehaviorSettings::ControlledMove { speed, .. } => {
					*speed > 0.0 && input.movement(&behavior.entity) != [0.0; 3]
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
			let entity = &behavior.entity;
			let index = world
				.entity_index(entity)
				.ok_or("Configured behavior target is missing")?;
			match &behavior.settings {
				BehaviorSettings::ControlledMove { speed, space } => {
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
				BehaviorSettings::Spin { angular_velocity } => {
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
	#[serde(default)]
	pub progress_changed: bool,
	#[serde(default)]
	pub progress_revision: u64,
}
pub struct SimulationSession {
	project: Arc<ProjectData>,
	world: RuntimeWorld,
	collisions: CollisionWorld,
	clock: FixedStepper,
	module: Box<dyn GameModule>,
	inputs: InputFrame,
	paused: bool,
	tick: u64,
	actions: Vec<SimulationAction>,
	scene_id: SceneId,
	controlled_targets: Vec<EntityKey>,
	controlled_target_set: HashSet<EntityKey>,
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
		let scene_id = project.startup_scene_id.clone();
		Self::with_factory_for_scene(project, scene_id, document, factory)
	}

	pub fn with_factory_for_scene(
		project: ProjectData,
		scene_id: SceneId,
		document: Arc<SceneData>,
		factory: &dyn GameModuleFactory,
	) -> Result<Self, String> {
		Self::with_shared_project_for_scene(Arc::new(project), scene_id, document, factory)
	}

	pub(crate) fn with_shared_project_for_scene(
		project: Arc<ProjectData>,
		scene_id: SceneId,
		document: Arc<SceneData>,
		factory: &dyn GameModuleFactory,
	) -> Result<Self, String> {
		let behaviors = project.resolved_behaviors_for(&scene_id)?;
		let world = RuntimeWorld::new(document)?;
		let mut controlled_targets = Vec::new();
		let mut seen = HashSet::new();

		for (index, behavior) in behaviors.iter().enumerate() {
			if world.entity_index(&behavior.entity).is_none() {
				return Err(format!(
					"Behavior {index} targets missing entity {:?}",
					behavior.entity.as_str()
				));
			}
			if matches!(&behavior.settings, BehaviorSettings::ControlledMove { .. })
				&& seen.insert(behavior.entity.clone())
			{
				controlled_targets.push(behavior.entity.clone());
			}
		}
		let module = factory.create_prepared(PreparedModuleConfig {
			project: &project,
			scene_id: &scene_id,
			behaviors,
		})?;
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
			scene_id,
			controlled_targets,
			controlled_target_set: seen,
		})
	}
	pub fn project(&self) -> &ProjectData {
		&self.project
	}
	pub fn scene_id(&self) -> &SceneId {
		&self.scene_id
	}
	pub fn controlled_targets(&self) -> &[EntityKey] {
		&self.controlled_targets
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
	pub(crate) fn needs_tick_with_progress(&self, progress: ProgressView<'_>) -> bool {
		!self.paused
			&& self
				.module
				.needs_tick_with_progress(&self.world, &self.inputs, progress)
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
		if !self.controlled_target_set.contains(entity) {
			return Err("Input target has no configured controlled movement behavior".into());
		}

		let movement = normalized_input(movement)?;
		Ok(Self::apply_input(
			&mut self.inputs,
			&mut self.clock,
			entity,
			movement,
		))
	}

	pub fn set_targeted_input(
		&mut self,
		target: &MovementTarget,
		movement: [f32; 3],
	) -> Result<bool, String> {
		match target {
			MovementTarget::AllControlled => self.set_controlled_input(movement),
			MovementTarget::Entity(entity) => self.set_input(entity, movement),
		}
	}

	fn apply_input(
		inputs: &mut InputFrame,
		clock: &mut FixedStepper,
		entity: &EntityKey,
		movement: [f32; 3],
	) -> bool {
		match inputs.movement.get_mut(entity) {
			Some(old) if *old == movement => return false,
			Some(old) if movement != [0.0; 3] => {
				*old = movement;
				return true;
			}
			Some(_) => {
				inputs.movement.remove(entity);
			}
			None if movement == [0.0; 3] => return false,
			None => {
				inputs.movement.insert(entity.clone(), movement);
			}
		}
		clock.reset();
		true
	}

	pub fn set_controlled_input(&mut self, movement: [f32; 3]) -> Result<bool, String> {
		let movement = normalized_input(movement)?;

		if self.controlled_targets.is_empty() && movement != [0.0; 3] {
			return Err(
				"Configure a controlled movement behavior before supplying movement".into(),
			);
		}

		let mut changed = false;
		for entity in &self.controlled_targets {
			changed |= Self::apply_input(&mut self.inputs, &mut self.clock, entity, movement);
		}

		Ok(changed)
	}

	pub fn tick(&mut self, elapsed: f64) -> Result<TickOutcome, String> {
		self.tick_with_progress(elapsed, &mut ProgressState::empty())
	}

	pub(crate) fn tick_with_progress(
		&mut self,
		elapsed: f64,
		progress: &mut ProgressState,
	) -> Result<TickOutcome, String> {
		if !elapsed.is_finite() || elapsed < 0.0 {
			return Err("Elapsed time must be finite and nonnegative".into());
		}
		let mut outcome = TickOutcome {
			tick: self.tick,
			progress_revision: progress.view().revision(),
			..TickOutcome::default()
		};
		if !self.needs_tick_with_progress(progress.view()) {
			self.clock.reset();
			return Ok(outcome);
		}
		let mut clock = self.clock.clone();
		let report = clock.advance(elapsed)?;
		if self.tick.checked_add(report.steps as u64).is_none() {
			return Err("Simulation tick counter is exhausted".into());
		}
		let mut before = HashMap::<EntityKey, TransformData>::new();
		let mut changes = ChangeAccumulator::default();
		let mut actions_applied = 0;
		let initial_matrix_updates = self.world.matrix_updates();
		let mut collision_candidates_remaining = Self::MAX_COLLISION_CANDIDATES_PER_TICK;
		let mut progress_transaction = progress.transaction();
		let result = (|| -> Result<(), String> {
			for _ in 0..report.steps {
				self.actions.clear();
				self.module.plan_with_progress(
					GameplayContext {
						world: &self.world,
						input: &self.inputs,
						step_seconds: report.step_seconds,
						progress: progress_transaction.view(),
					},
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
					if let SimulationAction::Progress(action) = &action {
						progress_transaction.apply(action)?;
						continue;
					}
					let entity = match &action {
						SimulationAction::Progress(_) => {
							return Err("Progress action reached world mutation".into());
						}
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
						SimulationAction::Progress(_) => {
							return Err("Progress action reached world mutation".into());
						}
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
					changes.merge(change);
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
		outcome.progress_changed = progress_transaction.commit();
		outcome.progress_revision = progress.view().revision();
		self.clock = clock;
		self.tick += report.steps as u64;
		outcome.tick = self.tick;
		outcome.steps = report.steps;
		outcome.dropped_seconds = report.dropped_seconds;
		outcome.changes = changes.finish();
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
