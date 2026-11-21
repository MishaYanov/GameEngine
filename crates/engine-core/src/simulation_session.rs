//! One simulation pipeline for editor Play, the derived player and headless runs.
use crate::camera_input::{CameraInputLatch, CameraIntent};
use crate::game_progress::{GameProgress, ProgressAction, ProgressState, ProgressView};
use crate::scene_catalog::SceneId;
use crate::{
	EntityKey,
	collision::{CollisionStats, CollisionWorld},
	game_camera::{GameCameraController, OrbitRuntimeState},
	input::{GameKeyboardInput, HeldGameActions},
	project::{BehaviorSettings, BehaviorSpace, ProjectData, ResolvedBehavior},
	scene_data::{EntityData, MaterialOverrideData, SceneData, TransformData},
	simulation::FixedStepper,
	triggers::{TriggerEvent, TriggerTracker, TriggerView},
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
	game_actions: HeldGameActions,
}
impl InputFrame {
	pub fn movement(&self, entity: &EntityKey) -> [f32; 3] {
		self.movement.get(entity).copied().unwrap_or([0.0; 3])
	}
	pub fn is_idle(&self) -> bool {
		self.movement.is_empty() && self.game_actions.ids.is_empty()
	}
	pub fn action_held(&self, id: &str) -> bool {
		self.game_actions.ids.contains(id)
	}
	/// Multiple held actions may feed the same parameter. Any held source sets it.
	pub fn animation_bool(&self, parameter: &str) -> bool {
		self.game_actions.animation_parameters.contains(parameter)
	}
}
pub enum SimulationAction {
	Despawn {
		handle: crate::runtime_lifecycle::RuntimeEntityHandle,
	},
	Progress(ProgressAction),
	RequestScene {
		scene_id: SceneId,
	},
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
	SetVisible {
		entity: EntityKey,
		visible: bool,
	},
	SetCollisionEnabled {
		entity: EntityKey,
		enabled: bool,
	},
	SetMaterial {
		entity: EntityKey,
		material: Option<MaterialOverrideData>,
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

/// Bounded, game-owned text for the active runtime objective.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameplayStatus {
	pub objective: String,
	#[serde(default)]
	pub detail: Option<String>,
	#[serde(default)]
	pub score: Option<i32>,
	pub completed: bool,
}

impl GameplayStatus {
	pub const MAX_TEXT_BYTES: usize = 128;

	pub(crate) fn is_valid(&self) -> bool {
		let valid_text = |text: &str| {
			text.len() <= Self::MAX_TEXT_BYTES
				&& !text.trim().is_empty()
				&& !text.chars().any(char::is_control)
		};
		valid_text(&self.objective) && self.detail.as_deref().is_none_or(valid_text)
	}
}

/// Modules plan bounded actions from a read-only world. Mutation and collision
/// policy remain in the engine, so a game module cannot bypass world validation.
pub struct GameplayContext<'a> {
	pub world: &'a RuntimeWorld,
	pub input: &'a InputFrame,
	pub step_seconds: f32,
	pub progress: ProgressView<'a>,
	/// Edges sampled before this fixed step's actions. Exit precedes Enter.
	pub triggers: &'a [TriggerEvent],
	pub trigger_overlaps: TriggerView<'a>,
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

	/// Projects optional status from committed run progress for a host to display.
	fn status(&self, _progress: ProgressView<'_>) -> Option<GameplayStatus> {
		None
	}
}

pub trait GameModuleFactory: Send + Sync {
	/// Explicit references for portable authored assemblies. Modules define these
	/// paths; core must never infer identities from arbitrary matching strings.
	fn prefab_references(
		&self,
		_scene_settings: &serde_json::Value,
	) -> Result<ModuleSettingReferences, String> {
		Err("This gameplay module does not declare portable prefab setting references".into())
	}
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

#[derive(Default)]
pub struct ModuleSettingReferences {
	pub entities: std::collections::BTreeMap<String, EntityKey>,
	pub scenes: std::collections::BTreeMap<String, SceneId>,
}

/// Resolves the project configuration once, before a new game run is created.
pub type GameModuleResolver =
	Arc<dyn Fn(&ProjectData) -> Result<Arc<dyn GameModuleFactory>, String> + Send + Sync>;

/// The engine's built-in fallback. Applications can resolve additional native modules.
pub fn resolve_default_gameplay(
	project: &ProjectData,
) -> Result<Arc<dyn GameModuleFactory>, String> {
	match project.gameplay_module.as_deref() {
		None => Ok(Arc::new(ConfiguredBehaviorFactory)),
		Some(id) => Err(format!("Unsupported gameplay module {id:?}")),
	}
}

/// Engine-prepared module input. Only the engine can construct this validated
/// context; custom factories can inspect it or keep implementing `create` only.
pub struct PreparedModuleConfig<'a> {
	project: &'a ProjectData,
	scene_id: &'a SceneId,
	document: &'a SceneData,
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
	pub fn document(&self) -> &SceneData {
		self.document
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
	/// Accepted fixed steps retained across a runtime asset-preparation boundary.
	/// Fulfill/reject pending spawns, then call tick(0.0) before new elapsed time.
	#[serde(default)]
	pub pending_steps: u32,
	pub dropped_seconds: f64,
	pub changes: WorldChange,
	#[serde(default)]
	pub progress_changed: bool,
	/// Committed orbit diagnostics or pending input changed independently of the rendered pose.
	#[serde(default)]
	pub camera_status_changed: bool,
	#[serde(default)]
	pub progress_revision: u64,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub requested_scene: Option<SceneId>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub trigger_events: Vec<TriggerEvent>,
}
pub struct SimulationSession {
	animations: crate::animation::AnimationBank,
	animation_bindings: crate::animation::PreparedAnimationBindings,
	animation_resolver: Option<crate::animation::AnimationResolver>,
	characters: crate::character_controller::CharacterRuntime,
	lifecycle: crate::runtime_lifecycle::RuntimeLifecycle,
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
	triggers: TriggerTracker,
	camera: GameCameraController,
	visibility_capabilities: Option<HashSet<EntityKey>>,
	camera_input: CameraInputLatch,
}
impl SimulationSession {
	pub const MAX_ACTIONS_PER_TICK: usize = ActionWriter::MAX_ACTIONS;
	pub const MAX_MATRIX_UPDATES_PER_TICK: u64 = 1_000_000;
	pub const MAX_COLLISION_CANDIDATES_PER_TICK: usize = 1_000_000;

	pub fn new(project: ProjectData, document: Arc<SceneData>) -> Result<Self, String> {
		let factory = resolve_default_gameplay(&project)?;
		Self::with_factory(project, document, factory.as_ref())
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
		document.validate()?;
		let runtime_document = if document
			.entities
			.iter()
			.any(|entity| entity.linked_prefab.is_some())
		{
			let mut resolved = (*document).clone();
			for entity in &mut resolved.entities {
				entity.linked_prefab = None;
			}
			Arc::new(resolved)
		} else {
			document
		};
		let world = RuntimeWorld::new(runtime_document)?;
		let characters = crate::character_controller::CharacterRuntime::new(&world, &project)?;
		let mut controlled_targets = Vec::new();
		let mut seen = HashSet::new();

		for (index, behavior) in behaviors.iter().enumerate() {
			if characters.contains(&behavior.entity) {
				return Err(
					"Character controller owns its body's motion; remove configured movement/spin"
						.into(),
				);
			}
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
			document: world.data(),
			behaviors,
		})?;
		let collisions = CollisionWorld::new(&world)?;
		let clock = FixedStepper::new(project.fixed_hz, project.max_catchup_steps)?;
		let triggers = TriggerTracker::new(&world);
		let lifecycle = crate::runtime_lifecycle::RuntimeLifecycle::new(&world);
		let camera = GameCameraController::new_with_lifecycle(&world, &lifecycle)?;
		let camera_input = CameraInputLatch::default();

		Ok(Self {
			animations: Default::default(),
			animation_bindings: Default::default(),
			animation_resolver: None,
			characters,
			lifecycle,
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
			triggers,
			camera,
			visibility_capabilities: None,
			camera_input,
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
	pub fn camera(&self) -> &crate::Camera {
		self.camera.camera()
	}
	pub fn orbit_state(&self) -> Option<&OrbitRuntimeState> {
		self.camera.orbit_state()
	}

	pub fn set_visibility_capabilities(
		&mut self,
		available: HashSet<EntityKey>,
	) -> Result<(), String> {
		if available
			.iter()
			.any(|key| self.world.entity_index(key).is_none())
		{
			return Err("Visibility capability references a missing runtime entity".into());
		}
		self.visibility_capabilities = Some(available);
		Ok(())
	}
	pub fn has_visibility_capabilities(&self) -> bool {
		self.visibility_capabilities.is_some()
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
	/// Native replay evidence excludes random world IDs and opaque module-owned
	/// state. The visible world and project remain separate host hash inputs.
	pub fn native_replay_state(&self) -> serde_json::Value {
		serde_json::json!({"schema_version":1,"lifecycle":self.lifecycle.replay_state(),"characters":self.characters.replay_state(),"animations":self.animations.replay_state()})
	}
	pub fn needs_tick(&self) -> bool {
		!self.paused
			&& (self.animations.needs_tick()
				|| self.characters.needs_tick()
				|| self.lifecycle.needs_tick()
				|| crate::native_behavior_runtime::needs_tick(&self.world)
				|| self.triggers.sample_needed
			|| self.camera_input.has_pending()
				|| self.camera.needs_tick(&self.world)
				|| self.module.needs_tick(&self.inputs))
	}
	pub(crate) fn gameplay_status(&self, progress: ProgressView<'_>) -> Option<GameplayStatus> {
		self.module
			.status(progress)
			.filter(GameplayStatus::is_valid)
	}

	pub(crate) fn needs_tick_with_progress(&self, progress: ProgressView<'_>) -> bool {
		!self.paused
			&& (self.animations.needs_tick()
				|| self.characters.needs_tick()
				|| self.lifecycle.needs_tick()
				|| crate::native_behavior_runtime::needs_tick(&self.world)
				|| self.triggers.sample_needed
			|| self.camera_input.has_pending()
				|| self.camera.needs_tick(&self.world)
				|| self
					.module
					.needs_tick_with_progress(&self.world, &self.inputs, progress))
	}
	pub fn collision_stats(&self) -> CollisionStats {
		self.collisions.stats()
	}
	pub fn animations(&self) -> &crate::animation::AnimationBank {
		&self.animations
	}
	pub fn install_animation_bindings(
		&mut self,
		bindings: crate::animation::PreparedAnimationBindings,
	) -> Result<(), String> {
		let handles = self
			.world
			.data()
			.entities
			.iter()
			.filter_map(|e| self.lifecycle.handle(&e.key).map(|h| (e.key.clone(), h)))
			.collect();
		let bank = crate::animation::AnimationBank::new(&self.world, &bindings, &handles)?;
		self.animations = bank;
		self.animation_bindings = bindings;
		Ok(())
	}
	pub fn set_animation_resolver(
		&mut self,
		resolver: crate::animation::AnimationResolver,
	) -> Result<(), String> {
		let bindings = resolver(self.world.data())?;
		self.install_animation_bindings(bindings)?;
		self.animation_resolver = Some(resolver);
		Ok(())
	}
	pub fn require_animation_ready(&self) -> Result<(), String> {
		if self
			.world
			.data()
			.entities
			.iter()
			.any(|e| e.animation.is_some() && !self.animation_bindings.contains_key(&e.key))
		{
			return Err("Prepare animation assets before starting this scene".into());
		}
		Ok(())
	}
	pub fn has_runtime_instances(&self) -> bool {
		self.lifecycle.has_instances()
	}
	pub fn runtime_handle(
		&self,
		key: &EntityKey,
	) -> Option<crate::runtime_lifecycle::RuntimeEntityHandle> {
		self.lifecycle.handle(key)
	}
	pub fn pending_runtime_spawns(&self) -> Vec<crate::runtime_lifecycle::RuntimeSpawnRequest> {
		self.lifecycle.requests()
	}
	pub fn character_motion(&self) -> Vec<crate::character_controller::CommittedMotion> {
		self.characters
			.keys()
			.filter_map(|key| {
				Some(
					self.characters
						.state(key)?
						.motion(self.lifecycle.handle(key)?),
				)
			})
			.collect()
	}
	pub fn set_character_input(
		&mut self,
		key: &EntityKey,
		movement_xz: [f32; 2],
		jump_held: bool,
	) -> Result<(), String> {
		if self.paused {
			return Ok(());
		}
		self.characters.set_input(key, movement_xz, jump_held)
	}
	pub fn reject_runtime_spawn(&mut self, id: u64, error: &str) {
		self.lifecycle.reject_request(id, error);
	}
	pub fn prepare_runtime_spawn(
		&self,
		prefab: &crate::editor::PrefabData,
		fingerprint: &str,
		position: [f32; 3],
		request: Option<u64>,
	) -> Result<crate::runtime_lifecycle::PreparedRuntimeSpawn, String> {
		self.lifecycle
			.prepare(&self.world, prefab, fingerprint, position, request)
	}
	pub fn runtime_spawn_document(
		&self,
		prepared: &crate::runtime_lifecycle::PreparedRuntimeSpawn,
	) -> Result<SceneData, String> {
		self.lifecycle.candidate(&self.world, prepared)
	}
	pub fn commit_runtime_spawn(
		&mut self,
		prepared: crate::runtime_lifecycle::PreparedRuntimeSpawn,
	) -> Result<WorldChange, String> {
		if self.paused {
			return Err("Resume Play before admitting runtime entities".into());
		}
		let data = self.runtime_spawn_document(&prepared)?;
		if self.visibility_capabilities.is_some()
			&& prepared.entities.iter().any(|entity| {
				entity.renderable
					&& prepared
						.visible
						.as_ref()
						.is_none_or(|available| !available.contains(&entity.key))
			}) {
			return Err("Prepare native render resources before admitting runtime entities".into());
		}
		let world = RuntimeWorld::new(Arc::new(data))?;
		let collisions = CollisionWorld::new(&world)?;
		let mut characters = self.characters.clone();
		characters.reconcile(&world, &self.project)?;
		let mut lifecycle = self.lifecycle.clone();
		lifecycle.admit(&prepared, self.tick)?;
		let mut animation_bindings = self.animation_bindings.clone();
		let mut missing = world.data().clone();
		missing
			.entities
			.retain(|e| e.animation.is_some() && !animation_bindings.contains_key(&e.key));
		if !missing.entities.is_empty() {
			let resolver = self
				.animation_resolver
				.as_ref()
				.ok_or("Runtime animation assets have no resolver")?;
			animation_bindings.extend(resolver(&missing)?);
		}
		animation_bindings.retain(|k, _| world.entity_index(k).is_some());
		let handles = world
			.data()
			.entities
			.iter()
			.filter_map(|e| lifecycle.handle(&e.key).map(|h| (e.key.clone(), h)))
			.collect();
		let mut animations =
			crate::animation::AnimationBank::new(&world, &animation_bindings, &handles)?;
		animations.preserve_existing(&self.animations);
		let keys = prepared
			.entities
			.iter()
			.map(|entity| entity.key.clone())
			.collect();
		if let Some(available) = &mut self.visibility_capabilities {
			available.extend(prepared.visible.into_iter().flatten());
		}
		self.world = world;
		self.animations = animations;
		self.animation_bindings = animation_bindings;
		self.collisions = collisions;
		self.characters = characters;
		self.lifecycle = lifecycle;
		self.triggers.structural_change(&self.world);
		Ok(WorldChange {
			changed_entities: keys,
			topology_changed: true,
			full_resync: true,
			..WorldChange::default()
		})
	}
	pub fn runtime_inspection(
		&self,
		play_session_id: &str,
		run_id: &str,
		offset: usize,
		limit: usize,
	) -> Result<crate::runtime_lifecycle::RuntimeInspectionDto, String> {
		if !(1..=128).contains(&limit) {
			return Err("Runtime inspection page size must be 1..=128".into());
		}
		Ok(crate::runtime_lifecycle::RuntimeInspectionDto {
			character_motion: self.character_motion(),
			animations: self.animations.inspection(self.paused),
			play_session_id: play_session_id.into(),
			run_id: run_id.into(),
			scene_id: self.scene_id.clone(),
			tick: self.tick,
			topology_revision: self.lifecycle.topology_revision,
			total_entities: self.world.data().entities.len(),
			entities: self.lifecycle.entities(&self.world, offset, limit),
			timer_count: self.lifecycle.timer_count(),
			pending_spawns: self.lifecycle.request_count(),
			pending_events: self.lifecycle.events.len(),
			event_sequence: self.lifecycle.events.sequence(),
			machine_deadlines: self.lifecycle.machine_deadlines(),
			stale_events_discarded: self.lifecycle.stale_events_discarded,
			last_error: self.lifecycle.last_error.clone(),
		})
	}
	pub fn clear_input(&mut self) {
		self.characters.clear_input();
		self.camera_input.clear();
		self.inputs.movement.clear();
		self.inputs.game_actions = HeldGameActions::default();
		self.clock.reset();
	}
	pub fn set_paused(&mut self, paused: bool) {
		if self.paused != paused {
			self.paused = paused;
			self.clear_input();
		}
	}
	pub fn set_input(&mut self, entity: &EntityKey, movement: [f32; 3]) -> Result<bool, String> {
		let movement = normalized_input(movement)?;
		if self.characters.contains(entity) {
			self.characters
				.set_movement(entity, [movement[0], movement[2]])?;
			return Ok(true);
		}
		if !self.controlled_target_set.contains(entity) {
			return Err("Input target has no configured controlled movement behavior".into());
		}

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

	pub(crate) fn set_game_keyboard_input(
		&mut self,
		target: &MovementTarget,
		input: GameKeyboardInput,
	) -> Result<(), String> {
		if self.paused {
			return Ok(());
		}
		let [x, y] = input.movement;
		// The legacy controlled mover uses XY; upright characters use XZ.
		// Route each target once so the legacy path cannot overwrite character Z.
		match target {
			MovementTarget::AllControlled => {
				self.characters.sample_keyboard(
					&self.world,
					None,
					input.movement,
					&input.actions,
				)?;
				for entity in &self.controlled_targets {
					Self::apply_input(&mut self.inputs, &mut self.clock, entity, [x, y, 0.0]);
				}
			}
			MovementTarget::Entity(entity) if self.characters.contains(entity) => {
				self.characters.sample_keyboard(
					&self.world,
					Some(entity),
					input.movement,
					&input.actions,
				)?;
			}
			MovementTarget::Entity(entity) => {
				self.set_input(entity, [x, y, 0.0])?;
			}
		}
		self.inputs.game_actions = input.actions;
		Ok(())
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
		let character_keys = self.characters.keys().cloned().collect::<Vec<_>>();
		for key in &character_keys {
			self.characters
				.set_movement(key, [movement[0], movement[2]])?;
		}

		if self.controlled_targets.is_empty() && character_keys.is_empty() && movement != [0.0; 3] {
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

	pub fn pending_camera_input(&self) -> CameraIntent {
		self.camera_input.snapshot()
	}

	pub(crate) fn submit_camera_input(&mut self, intent: CameraIntent) -> Result<(), String> {
		if self.paused {
			return Err("Resume Play before submitting camera input".into());
		}
		if !self.orbit_state().is_some_and(|rig| !rig.target_lost) {
			return Err("Camera input requires an orbit camera with a live target".into());
		}
		self.camera_input.submit(intent)
	}

	pub fn tick(&mut self, elapsed: f64) -> Result<TickOutcome, String> {
		self.tick_with_progress(elapsed, &mut ProgressState::empty(), false)
	}

	pub(crate) fn tick_with_progress(
		&mut self,
		elapsed: f64,
		progress: &mut ProgressState,
		scene_request_pending: bool,
	) -> Result<TickOutcome, String> {
		if !elapsed.is_finite() || elapsed < 0.0 {
			return Err("Elapsed time must be finite and nonnegative".into());
		}
		self.require_animation_ready()?;
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
		let mut before = HashMap::<EntityKey, EntityData>::new();
		let mut structural_before: Option<Arc<SceneData>> = None;
		let mut structural_work = 0usize;
		let mut lifecycle = self.lifecycle.clone();
		let mut characters = self.characters.clone();
		let mut animations = self.animations.clone();
		let mut triggers = self.triggers.clone();
		let mut camera = self.camera.clone();
		let mut camera_input = self.camera_input.clone();
		let mut trigger_events = Vec::new();
		let mut trigger_event_bytes = 0usize;
		let mut changes = ChangeAccumulator::default();
		let mut actions_applied = 0;
		let mut gameplay_deliveries = 0usize;
		let mut steps = 0;
		let mut pending_steps = 0;
		let mut requested_scene = None;
		let initial_matrix_updates = self.world.matrix_updates();
		let mut collision_candidates_remaining = Self::MAX_COLLISION_CANDIDATES_PER_TICK;
		let mut progress_transaction = progress.transaction();
		let result = (|| -> Result<(), String> {
			for _ in 0..report.steps {
				let event_start = trigger_events.len();
				if triggers.sample_needed {
					let contacts = self
						.collisions
						.query_sensor_overlaps_with_budget(&mut collision_candidates_remaining)?;
					triggers.sample(
						&self.world,
						&contacts,
						&self.scene_id,
						self.tick + u64::from(steps) + 1,
						&mut trigger_events,
						&mut trigger_event_bytes,
					)?;
				}
				self.actions.clear();
				let mut gameplay_emissions = crate::state_machine_runtime::plan(
					&self.world,
					&mut lifecycle,
					self.tick + u64::from(steps) + 1,
					&trigger_events[event_start..],
					&mut gameplay_deliveries,
					&mut ActionWriter {
						actions: &mut self.actions,
					},
				)?;
				crate::native_behavior_runtime::plan(
					&self.world,
					triggers.view(),
					&mut lifecycle,
					&mut ActionWriter {
						actions: &mut self.actions,
					},
				)?;
				for handle in lifecycle.due(self.tick + u64::from(steps) + 1) {
					if !self.actions.iter().any(|action| matches!(action, SimulationAction::Despawn { handle: existing } if existing == &handle)) {
						ActionWriter { actions: &mut self.actions }.push(SimulationAction::Despawn { handle })?;
					}
				}
				self.module.plan_with_progress(
					GameplayContext {
						world: &self.world,
						input: &self.inputs,
						step_seconds: report.step_seconds,
						progress: progress_transaction.view(),
						triggers: &trigger_events[event_start..],
						trigger_overlaps: triggers.view(),
					},
					&mut ActionWriter {
						actions: &mut self.actions,
					},
				)?;
				// Structural removals become visible at the end of this step, after
				let count = characters.keys().count();
				actions_applied += count;
				if actions_applied > Self::MAX_ACTIONS_PER_TICK {
					return Err("Character action budget exceeded".into());
				}
				let character_change = characters.step(
					&mut self.world,
					&mut self.collisions,
					self.tick + u64::from(steps) + 1,
					report.step_seconds,
					&mut collision_candidates_remaining,
					&mut before,
				)?;
				if !character_change.is_empty() {
					triggers.invalidate();
				}
				changes.merge(character_change);
				if self
					.world
					.matrix_updates()
					.saturating_sub(initial_matrix_updates)
					> Self::MAX_MATRIX_UPDATES_PER_TICK
				{
					return Err("Character matrix update budget exceeded".into());
				}
				// Structural removals become visible at the end of this step, after
				// all systems have planned against the same stable world.
				self.actions
					.sort_by_key(|action| matches!(action, SimulationAction::Despawn { .. }));
				let mut removed_handles = HashSet::new();
				for action in self.actions.drain(..) {
					let forbidden = match &action {
						SimulationAction::Move { entity, .. }
						| SimulationAction::Rotate { entity, .. }
						| SimulationAction::SetTransform { entity, .. } => characters.contains(entity),
						SimulationAction::SetCollisionEnabled { entity, enabled } => {
							characters.forbids_collider_change(&self.world, entity, *enabled)
						}
						_ => false,
					};
					if forbidden {
						return Err(
							"Character controller owns its body transform and collider".into()
						);
					}
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
					let action = match action {
						SimulationAction::Despawn { handle } => {
							if removed_handles.contains(&handle) {
								continue;
							}
							let keys = lifecycle.removal(&handle)?;
							structural_work =
								structural_work.saturating_add(self.world.data().entities.len());
							if structural_work > 1_000_000 {
								return Err("Runtime structural work budget exceeded".into());
							}
							for key in &keys {
								if let Some(handle) = lifecycle.handle(key) {
									removed_handles.insert(handle);
								}
							}
							structural_before.get_or_insert_with(|| self.world.document());
							let change = self.world.remove_runtime_entities(&keys)?;
							self.collisions = CollisionWorld::new(&self.world)?;
							lifecycle.removed(&keys)?;
							characters.prune(&self.world);
							lifecycle.prune_states(&self.world);
							triggers.invalidate();
							changes.merge(change);
							continue;
						}
						SimulationAction::Progress(action) => {
							progress_transaction.apply(&action)?;
							continue;
						}
						SimulationAction::RequestScene { scene_id } => {
							if scene_id == self.scene_id {
								return Err("Cannot request the current runtime scene".into());
							}
							self.project.scenes.get(&scene_id).map_err(|error| {
								format!("Cannot request scene {:?}: {error}", scene_id.as_str())
							})?;
							if scene_request_pending {
								continue;
							}
							if let Some(previous) = &requested_scene {
								if previous != &scene_id {
									return Err(
										"Conflicting runtime scene requests in one step".into()
									);
								}
							} else {
								requested_scene = Some(scene_id);
							}
							continue;
						}
						action => action,
					};
					let entity = match &action {
						SimulationAction::Progress(_)
						| SimulationAction::RequestScene { .. }
						| SimulationAction::Despawn { .. } => {
							unreachable!("Non-world actions were handled above");
						}
						SimulationAction::Move { entity, .. }
						| SimulationAction::Rotate { entity, .. }
						| SimulationAction::SetVisible { entity, .. }
						| SimulationAction::SetCollisionEnabled { entity, .. }
						| SimulationAction::SetMaterial { entity, .. }
						| SimulationAction::SetTransform { entity, .. } => entity,
					};
					let index = self
						.world
						.entity_index(entity)
						.ok_or("Simulation action target is missing")?;
					before
						.entry(entity.clone())
						.or_insert_with(|| self.world.data().entities[index].clone());
					let affects_collision = !matches!(
						&action,
						SimulationAction::SetVisible { .. } | SimulationAction::SetMaterial { .. }
					);
					let change = match action {
						SimulationAction::Progress(_)
						| SimulationAction::RequestScene { .. }
						| SimulationAction::Despawn { .. } => {
							unreachable!("Non-world actions were handled above");
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
						SimulationAction::SetVisible { entity, visible } => {
							if visible
								&& self
									.visibility_capabilities
									.as_ref()
									.is_some_and(|available| !available.contains(&entity))
							{
								return Err(format!(
									"Cannot show entity {:?}: its render model was not prepared",
									entity.as_str()
								));
							}
							self.world.set_visible(&entity, visible)?
						}
						SimulationAction::SetCollisionEnabled { entity, enabled } => {
							let change = self.world.set_collision_enabled(&entity, enabled)?;
							self.collisions.synchronize(&self.world, &change)?;
							change
						}
						SimulationAction::SetMaterial { entity, material } => {
							self.world.set_material(&entity, material)?
						}
					};
					if affects_collision && !change.is_empty() {
						triggers.invalidate();
					}
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
				// Removal or disabling of a support must be visible to later staged systems.
				characters.prune(&self.world);
				crate::state_machine_runtime::route_motion(
					&self.world,
					&mut lifecycle,
					&characters,
					self.tick + u64::from(steps) + 1,
					&mut gameplay_emissions,
				)?;
				let motions = characters
					.keys()
					.filter_map(|key| {
						let state = characters.state(key)?;
						Some((
							key.clone(),
							crate::animation::AnimationMotion {
								entity: lifecycle.handle(key)?,
								tick: state.tick,
								grounded: state.grounded,
								planar_speed: state.planar_speed,
								vertical_speed: state.velocity[1],
							},
						))
					})
					.collect();
				if animations.step(
					&self.world,
					&motions,
					self.tick + u64::from(steps) + 1,
					self.project.fixed_hz,
				)? {
					changes.merge(WorldChange {
						changed_entities: animations.poses().map(|(k, _)| k.clone()).collect(),
						..WorldChange::default()
					});
				}
				let before_camera = camera.camera().clone();
				let clear_camera_input = camera.step(
					&self.world,
					&lifecycle,
					report.step_seconds,
					camera_input.take_for_step(),
					self.tick + u64::from(steps) + 1,
				)?;
				if clear_camera_input {
					camera_input.clear();
				}
				if before_camera.position != camera.camera().position
					|| before_camera.yaw != camera.camera().yaw
					|| before_camera.pitch != camera.camera().pitch
				{
					changes.merge(WorldChange {
						camera_changed: true,
						..WorldChange::default()
					});
				}
				steps += 1;
				if requested_scene.is_some() {
					// A new scene starts at the next step, after the host commits it.
					break;
				}
				if lifecycle.request_count() > 0 {
					// Publish this completed step so every host can prepare/admit the
					// same request before planning the following fixed step. Retain
					// accepted catch-up work rather than discarding it as frame time.
					pending_steps = report.steps - steps;
					clock.retain_steps(pending_steps)?;
					break;
				}
			}
			Ok(())
		})();
		if let Err(error) = result {
			// Cold failure path restores only entities touched by planned
			// actions. A full document copy is never retained per steady tick.
			let mut restored = structural_before
				.as_deref()
				.unwrap_or(self.world.data())
				.clone();
			for entity in &mut restored.entities {
				if let Some(original) = before.remove(&entity.key) {
					*entity = original;
				}
			}
			self.world = RuntimeWorld::new(Arc::new(restored))?;
			self.collisions = CollisionWorld::new(&self.world)?;
			self.paused = true;
			self.clear_input();
			self.actions.clear();
			// The application reports the returned failure. Record the recovery here.
			engine_logger::debug!(target: "engine_core::simulation", "Simulation rolled back and paused: tick={}", self.tick);
			return Err(format!("Simulation paused after rejected actions: {error}"));
		}
		outcome.progress_changed = progress_transaction.commit();
		outcome.progress_revision = progress.view().revision();
		self.clock = clock;
		self.tick += u64::from(steps);
		outcome.tick = self.tick;
		outcome.steps = steps;
		outcome.pending_steps = pending_steps;
		outcome.dropped_seconds = report.dropped_seconds
			+ f64::from(report.steps - steps - pending_steps) / f64::from(self.project.fixed_hz);
		outcome.changes = changes.finish();
		outcome.requested_scene = requested_scene;
		self.triggers = triggers;
		self.inputs
			.movement
			.retain(|key, _| self.world.entity_index(key).is_some());
		if let Some(available) = &mut self.visibility_capabilities {
			available.retain(|key| self.world.entity_index(key).is_some());
		}
		self.lifecycle = lifecycle;
		self.characters = characters;
		self.animations = animations;
		self.animation_bindings
			.retain(|key, _| self.world.entity_index(key).is_some());
		outcome.camera_status_changed = self.camera_input != camera_input
			|| self
			.camera
			.orbit_state()
			.zip(camera.orbit_state())
			.is_some_and(|(previous, next)| !previous.same_status(next));
		self.camera = camera;
		self.camera_input = camera_input;
		outcome.trigger_events = trigger_events;
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
