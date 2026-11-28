//! One simulation pipeline for editor Play, the derived player and headless runs.
use crate::camera_input::{CameraInputLatch, CameraIntent};
use crate::game_progress::{ProgressState, ProgressView};
use crate::scene_catalog::SceneId;
use crate::{
	EntityKey,
	collision::{CameraExclusions, CollisionStats, CollisionWorld, InteractionMatrix},
	game_camera::{GameCameraController, OrbitRuntimeState, obstruction_query},
	particles::{ParticleBank, ParticleSample},
	project::{BehaviorSettings, ProjectData},
	scene_data::{EntityData, SceneData},
	simulation::FixedStepper,
	triggers::{TriggerEvent, TriggerTracker},
	world::{ChangeAccumulator, RuntimeWorld, WorldChange},
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::{collections::HashMap, sync::Arc};

mod animator_graphs;
mod camera_objects;
mod conditional_input;
mod control_graph;
mod gameplay;
mod input;
mod object_scripts;
mod player_control;
mod possession;
mod scene_flow;
mod sequence;
mod world_regions;
pub(crate) use world_regions::RegionMovementGuard;
pub use world_regions::{PreparedRegionChange, WorldRegionState};
pub use crate::world::RuntimeReparentMode;
pub use control_graph::ControlGraphResolver;
pub use player_control::InputActionsResolver;
pub use possession::LocalInputScope;
pub use crate::projectile::{ProjectileHit, ProjectileHookStep};

pub use gameplay::{
	ActionWriter, ConfiguredBehaviorFactory, GameModule, GameModuleFactory, GameModuleResolver,
	GameplayContext, GameplayStatus, ModuleSettingReferences, PreparedModuleConfig,
	SimulationAction, resolve_default_gameplay,
};
pub use input::{InputFrame, MovementTarget};
pub use object_scripts::{
	ObjectScript, ObjectScriptAction, ObjectScriptCommands, ObjectScriptContext,
	ObjectScriptRegistration, ObjectScriptTrigger, ProjectScriptRegistration,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommittedInputAction {
	pub id: String,
	pub tick: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TickOutcome {
	pub tick: u64,
	pub steps: u32,
	/// Accepted fixed steps retained across a runtime asset-preparation boundary.
	/// Fulfill/reject pending spawns, then call tick(0.0) before new elapsed time.
	#[serde(default)]
	pub pending_steps: u32,
	/// A committed particle frame needs presentation without a scene document change.
	#[serde(default, skip_serializing_if = "std::ops::Not::not")]
	pub particles_changed: bool,
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
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub requested_route: Option<crate::project::SceneRouteId>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub sequence_events: Vec<crate::sequence::SequenceEvent>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub trigger_events: Vec<TriggerEvent>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub accepted_action_presses: Vec<CommittedInputAction>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub audio_onsets: Vec<crate::scene_audio::CommittedAudioOnset>,
	/// Local input ownership changed during an accepted fixed step.
	#[serde(default, skip_serializing_if = "std::ops::Not::not")]
	pub possession_changed: bool,
}
pub struct SimulationSession {
	animations: crate::animation::AnimationBank,
	animation_bindings: crate::animation::PreparedAnimationBindings,
	animation_resolver: Option<crate::animation::AnimationResolver>,
	animator_resolver: Option<crate::animation::AnimatorResolver>,
	characters: crate::character_controller::CharacterRuntime,
	lifecycle: crate::runtime_lifecycle::RuntimeLifecycle,
	object_scripts: object_scripts::ObjectScripts,
	particles: ParticleBank,
	project: Arc<ProjectData>,
	world: RuntimeWorld,
	regions: world_regions::WorldRegions,
	collisions: CollisionWorld,
	clock: FixedStepper,
	module: Box<dyn GameModule>,
	inputs: InputFrame,
	possession: possession::LocalPossession,
	pending_possession: Option<possession::PossessionChange>,
	paused: bool,
	tick: u64,
	actions: Vec<SimulationAction>,
	scene_id: SceneId,
	controlled_targets: Vec<EntityKey>,
	controlled_target_set: HashSet<EntityKey>,
	triggers: TriggerTracker,
	sequences: sequence::SequenceDirector,
	camera: GameCameraController,
	camera_objects: camera_objects::CameraObjects,
	input_action_map: Option<crate::input::InputActionMap>,
	conditional_input: Option<conditional_input::ConditionalInputs>,
	control_graph: Option<crate::input::CompiledControlGraph>,
	control_graph_ready: bool,
	control_graph_step: Option<control_graph::CommittedControlStep>,
	visibility_capabilities: Option<HashSet<EntityKey>>,
	camera_input: CameraInputLatch,
	camera_exclusions: Option<CameraExclusions>,
}
impl Drop for SimulationSession {
	fn drop(&mut self) {
		self.object_scripts.stop();
	}
}

impl SimulationSession {
	pub const MAX_ACTIONS_PER_TICK: usize = ActionWriter::MAX_ACTIONS;
	pub const MAX_MATRIX_UPDATES_PER_TICK: u64 = 1_000_000;
	const MAX_STRUCTURAL_WORK_PER_TICK: usize = 1_000_000;
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
		let mut behaviors = project.resolved_behaviors_for(&scene_id)?;
		document.validate()?;
		let mut runtime_document = if crate::linked_prefab::needs_runtime_projection(&document) {
			Arc::new(crate::linked_prefab::resolve_scene(&document))
		} else {
			document
		};
		if runtime_document
			.entities
			.iter()
			.any(|entity| !entity.active)
		{
			Arc::make_mut(&mut runtime_document).project_runtime_activation();
		}
		let matrix = InteractionMatrix::compile(project.physics_layers.as_ref())?;
		if project.physics_layers.is_some() {
			crate::project::validate_required_physics_contacts(&runtime_document, &matrix)?;
		}
		project.sequences.validate_scene(&scene_id, &runtime_document)?;
		let world = RuntimeWorld::new(runtime_document)?;
		behaviors.retain(|behavior| {
			world
				.entity_index(&behavior.entity)
				.is_none_or(|index| world.data().entities[index].active)
		});
		let characters = crate::character_controller::CharacterRuntime::new(&world, &project)?;
		let lifecycle = crate::runtime_lifecycle::RuntimeLifecycle::new(&world);
		let possession = possession::LocalPossession::new(&world, &lifecycle);
		let mut controlled_targets = Vec::new();
		let mut seen = HashSet::new();

		for (index, behavior) in behaviors.iter().enumerate() {
			if matches!(&behavior.settings, BehaviorSettings::AcceptAction { .. }) {
				if !possession
					.owner
					.as_ref()
					.is_some_and(|owner| owner.key == behavior.entity)
				{
					return Err(
						"Accept Action behavior must target the local Player Controller".into(),
					);
				}
			} else if characters.contains(&behavior.entity) {
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
		let collisions = CollisionWorld::with_matrix(&world, matrix)?;
		let clock = FixedStepper::new(project.fixed_hz, project.max_catchup_steps)?;
		let triggers = TriggerTracker::new(&world);
		let object_scripts = object_scripts::ObjectScripts::prepare(
			&world, &lifecycle, factory.object_scripts(), &factory.project_scripts()?,
		)?;
		let mut camera_exclusions = None;
		let mut preparation_budget = Self::MAX_COLLISION_CANDIDATES_PER_TICK;
		let camera = GameCameraController::new_with_lifecycle(
			&world,
			&lifecycle,
			obstruction_query(
				&world,
				&lifecycle,
				&collisions,
				&mut camera_exclusions,
				&mut preparation_budget,
			),
		)?;
		let camera_input = CameraInputLatch::default();
		let camera_objects = camera_objects::CameraObjects::new(&world, &lifecycle);
		let control_graph_ready = control_graph::first_configured_actor(world.data()).is_none();
		let particles = ParticleBank::new(&world);

		Ok(Self {
			animations: Default::default(),
			animation_bindings: Default::default(),
			animation_resolver: None,
			animator_resolver: None,
			characters,
			lifecycle,
			object_scripts,
			particles,
			project,
			world,
			regions: Default::default(),
			collisions,
			clock,
			module,
			inputs: InputFrame {
				local_player: possession.owner.clone(),
				possession_managed: possession.managed,
				..InputFrame::default()
			},
			possession,
			pending_possession: None,
			paused: false,
			tick: 0,
			actions: Vec::new(),
			scene_id,
			controlled_targets,
			controlled_target_set: seen,
			triggers,
			sequences: sequence::SequenceDirector::default(),
			camera,
			camera_objects,
			input_action_map: None,
			conditional_input: None,
			control_graph: None,
			control_graph_ready,
			control_graph_step: None,
			visibility_capabilities: None,
			camera_input,
			camera_exclusions,
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
	/// Samples only particles from the committed runtime step.
	pub fn particles(&self) -> impl Iterator<Item = ParticleSample> + '_ {
		self.particles.samples(self.project.fixed_hz)
	}

	pub fn camera(&self) -> &crate::Camera {
		self.camera.camera()
	}
	pub fn orbit_state(&self) -> Option<&OrbitRuntimeState> {
		self.camera.orbit_state()
	}

	pub fn camera_replay_state(
		&self,
	) -> Result<crate::camera_replay::CameraReplayProjection, String> {
		crate::camera_replay::CameraReplayProjection::from_committed(
			self.camera(),
			self.orbit_state(),
			self.camera.is_following(),
			self.pending_camera_input(),
			self.lifecycle.world_id(),
		)
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
		let mut state = serde_json::json!({"schema_version":1,"lifecycle":self.lifecycle.replay_state(),"characters":self.characters.replay_state(),"animations":self.animations.replay_state()});
		if self.possession.generation > 0 {
			state["local_possession"] = self.possession.replay_state(self.inputs.awaiting_neutral);
		}
		if self.animations.has_animators() {
			state["animators"] = serde_json::Value::Array(self.animations.animator_inspection().into_iter().map(|entry| {
				serde_json::json!({"key":entry.entity.key,"component":entry.component_id,"state":entry.state_id,"ticks":entry.state_ticks,
					"transition":entry.transition,"blend_age":entry.blend_age,"blend_ticks":entry.blend_ticks})
			}).collect());
		}
		if !self.project.sequences.is_empty() {
			state["sequences"] = self.sequences.replay_state();
		}
		state
	}
	pub fn needs_tick(&self) -> bool {
		!self.paused
			&& (self.particles.needs_tick()
				|| self.animations.needs_tick()
				|| self.characters.needs_tick()
				|| self.lifecycle.needs_tick()
				|| self.object_scripts.needs_tick(&self.inputs)
				|| crate::native_behavior_runtime::needs_tick(&self.world)
				|| self.triggers.sample_needed
			|| self.sequences.needs_tick()
				|| !self.inputs.action_presses.is_empty()
				|| self
					.conditional_input
					.as_ref()
					.is_some_and(|input| input.has_pending())
				|| self.inputs.controller_movement != [0.0; 2]
				|| self.inputs.controller_look != [0.0; 2]
				|| self.inputs.control_sample.needs_tick()
				|| self.camera_input.has_pending()
				|| self.camera.needs_tick(&self.world)
				|| self.pending_possession.is_some()
				|| self.module.needs_tick(&self.inputs))
	}
	pub(crate) fn gameplay_status(&self, progress: ProgressView<'_>) -> Option<GameplayStatus> {
		self.module
			.status_in_world(&self.world, &self.collisions, progress)
			.filter(GameplayStatus::is_valid)
	}

	pub(crate) fn needs_tick_with_progress(&self, progress: ProgressView<'_>) -> bool {
		!self.paused
			&& (self.particles.needs_tick()
				|| self.animations.needs_tick()
				|| self.characters.needs_tick()
				|| self.lifecycle.needs_tick()
				|| self.object_scripts.needs_tick(&self.inputs)
				|| crate::native_behavior_runtime::needs_tick(&self.world)
				|| self.triggers.sample_needed
			|| self.sequences.needs_tick()
				|| !self.inputs.action_presses.is_empty()
				|| self
					.conditional_input
					.as_ref()
					.is_some_and(|input| input.has_pending())
				|| self.inputs.controller_movement != [0.0; 2]
				|| self.inputs.controller_look != [0.0; 2]
				|| self.inputs.control_sample.needs_tick()
				|| self.camera_input.has_pending()
				|| self.camera.needs_tick(&self.world)
				|| self.pending_possession.is_some()
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
		let mut document = self.lifecycle.candidate(&self.world, prepared)?;
		if prepared.entities.iter().any(|entity| !entity.active) {
			document.project_runtime_activation();
		}
		Ok(document)
	}
	pub fn commit_runtime_spawn(
		&mut self,
		mut prepared: crate::runtime_lifecycle::PreparedRuntimeSpawn,
	) -> Result<WorldChange, String> {
		if self.paused {
			return Err("Resume Play before admitting runtime entities".into());
		}
		let data = self.runtime_spawn_document(&prepared)?;
		if prepared.entities.iter().any(|entity| !entity.active) {
			let first = data.entities.len() - prepared.entities.len();
			prepared.entities.clone_from_slice(&data.entities[first..]);
		}
		if self.project.physics_layers.is_some() {
			crate::project::validate_required_physics_contacts(&data, &self.collisions.matrix())?;
		}
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
		let collisions = CollisionWorld::with_matrix(&world, self.collisions.matrix())?;
		let mut characters = self.characters.clone();
		characters.reconcile(&world, &self.project)?;
		let mut lifecycle = self.lifecycle.clone();
		lifecycle.admit(&prepared, self.tick)?;
		let mut object_scripts = self.object_scripts.clone();
		object_scripts.admit(&world, &lifecycle, &prepared.entities)?;
		// A new collider can occupy the existing arm even while the target rests.
		// Prepare the pose with the spawn. Keep elapsed time and pending input untouched.
		let mut camera = self.camera.clone();
		let mut camera_exclusions = None;
		if camera.orbit_state().is_some() {
			let mut preparation_budget = Self::MAX_COLLISION_CANDIDATES_PER_TICK;
			camera.step(
				&world,
				&lifecycle,
				0.0,
				CameraIntent::default(),
				self.tick,
				obstruction_query(
					&world,
					&lifecycle,
					&collisions,
					&mut camera_exclusions,
					&mut preparation_budget,
				),
			)?;
		}
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
		if let Some(resolver) = &self.animator_resolver {
			let bindings = resolver(&self.project, &self.scene_id, world.data())?;
			animations.install_animators(&world, &bindings, &handles, self.project.fixed_hz)?;
		} else if prepared.entities.iter().any(|entity| {
			entity
				.animator
				.as_ref()
				.is_some_and(|component| component.enabled)
		}) {
			return Err("Runtime spawned Animator requires a prepared graph".into());
		}
		animations.preserve_existing(&self.animations);
		let keys = prepared
			.entities
			.iter()
			.map(|entity| entity.key.clone())
			.collect();
		if let Some(available) = &mut self.visibility_capabilities {
			available.extend(prepared.visible.into_iter().flatten());
		}
		let camera_changed = camera_pose_changed(self.camera.camera(), camera.camera());
		self.camera = camera;
		self.camera_exclusions = camera_exclusions;
		self.world = world;
		self.animations = animations;
		self.animation_bindings = animation_bindings;
		self.collisions = collisions;
		self.characters = characters;
		self.lifecycle = lifecycle;
		self.object_scripts = object_scripts;
		self.particles.admit(&prepared.entities);
		self.triggers.structural_change(&self.world);
		Ok(WorldChange {
			changed_entities: keys,
			camera_changed,
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
			animators: self.animations.animator_inspection(),
			control_graph: self.control_graph_inspection(),
			play_session_id: play_session_id.into(),
			run_id: run_id.into(),
			scene_id: self.scene_id.clone(),
			tick: self.tick,
			topology_revision: self.lifecycle.topology_revision,
			local_player: self.local_player().cloned(),
			possession_generation: self.local_possession_generation(),
			total_entities: self.world.data().entities.len(),
			entities: self.lifecycle.entities(&self.world, offset, limit),
			timer_count: self.lifecycle.timer_count(),
			pending_spawns: self.lifecycle.request_count(),
			pending_events: self.lifecycle.events.len(),
			event_sequence: self.lifecycle.events.sequence(),
			machine_deadlines: self.lifecycle.machine_deadlines(),
			stale_events_discarded: self.lifecycle.stale_events_discarded,
			last_error: self
				.lifecycle
				.last_error
				.clone()
				.or_else(|| self.camera_object_diagnostic().map(str::to_owned)),
		})
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
		self.require_animators_ready()?;
		self.require_control_graph_ready()?;
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
		if report.steps == 0 {
			self.clock = clock;
			outcome.dropped_seconds = report.dropped_seconds;
			return Ok(outcome);
		}
		if self.tick.checked_add(report.steps as u64).is_none() {
			return Err("Simulation tick counter is exhausted".into());
		}
		let mut before = HashMap::<EntityKey, EntityData>::new();
		let mut structural_before: Option<Arc<SceneData>> = None;
		let mut structural_work = 0usize;
		let mut lifecycle = self.lifecycle.clone();
		let mut possession = self.possession.clone();
		let mut host_possession = self.pending_possession.take();
		let mut characters = self.characters.clone();
		let mut animations = self.animations.clone();
		let mut triggers = self.triggers.clone();
		let mut sequences = self.sequences.clone();
		let mut object_scripts = self.object_scripts.clone();
		// One step can update committed particles after validation. A catch-up
		// batch still stages each intermediate world pose for atomic rollback.
		let mut staged_particles = (report.steps > 1).then(|| self.particles.clone());
		let mut particles_changed = false;
		let mut retired_scripts = object_scripts::ObjectScripts::default();
		let input_layout = self
			.conditional_input
			.as_ref()
			.map(conditional_input::ConditionalInputs::selection);
		let mut camera = self.camera.clone();
		let mut camera_objects = self.camera_objects.clone();
		let mut camera_input = self.camera_input.clone();
		let mut camera_exclusions = self.camera_exclusions.clone();
		let mut trigger_events = Vec::new();
		let mut audio_onsets = Vec::new();
		let mut sequence_events = Vec::new();
		let mut accepted_action_presses = Vec::new();
		let mut control_graph_step = None;
		let mut trigger_event_bytes = 0usize;
		let mut changes = ChangeAccumulator::default();
		let mut actions_applied = 0;
		let mut gameplay_deliveries = 0usize;
		let mut steps = 0;
		let mut pending_steps = 0;
		let mut requested_scene = None;
		let mut requested_route = None;
		let initial_matrix_updates = self.world.matrix_updates();
		let mut collision_candidates_remaining = Self::MAX_COLLISION_CANDIDATES_PER_TICK;
		let mut progress_transaction = progress.transaction();
		let mut view_attachments = Vec::new();
		let region_movement = RegionMovementGuard::new(&self.project, &self.scene_id, &self.regions);
		let result = (|| -> Result<(), String> {
			for _ in 0..report.steps {
				let mut requested_possession = host_possession.take();
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
				let tick = self.tick + u64::from(steps) + 1;
				let sequence_was_active = sequences.needs_tick();
				let sequence_event_start = sequence_events.len();
				sequences.step(
					sequence::SequenceStep {
						library: &self.project.sequences,
						scene: &self.scene_id,
						world: &self.world,
						lifecycle: &lifecycle,
						triggers: &trigger_events[event_start..],
						tick,
						fixed_hz: self.project.fixed_hz,
						skip_pressed: self.inputs.action_pressed(sequence::SKIP_ACTION),
					},
					&animations,
					&mut sequence_events,
					&mut audio_onsets,
				);
				let sequence_gates_input = sequence_was_active
					|| sequences.needs_tick()
					|| sequence_events.len() != sequence_event_start;
				self.actions.clear();
				let mut gameplay_emissions = crate::state_machine_runtime::plan(
					&self.world,
					&mut lifecycle,
					self.tick + u64::from(steps) + 1,
					&trigger_events[event_start..],
					&mut gameplay_deliveries,
					&mut audio_onsets,
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
				let mut camera_delta = camera_input.take_for_step();
				if self.begin_input_layout_step(&characters, &lifecycle)? {
					camera_delta.yaw_delta = 0.0;
					camera_delta.pitch_delta = 0.0;
				}
				let controls = if sequence_gates_input
					|| self.inputs.possession_managed && self.inputs.local_player().is_none()
				{
					None
				} else {
					self.evaluate_control_graph(camera_delta)
				};
				let routed = if sequence_gates_input {
					camera_delta = CameraIntent::default();
					Some(sequence::neutral_gameplay_input(&self.inputs))
				} else {
					controls.as_ref().map(|(graph, _)| self.inputs.with_control_output(graph.intent))
				};
				let step_input = routed.as_ref().unwrap_or(&self.inputs);
				if let Some((route, _)) = &controls {
					camera_delta.yaw_delta = f64::from(route.intent.look.delta[0]);
					camera_delta.pitch_delta = f64::from(route.intent.look.delta[1]);
				}
				object_scripts.plan_tick(
					&self.world,
					&lifecycle,
					object_scripts::ObjectScriptTick {
						tick: self.tick + u64::from(steps) + 1,
						step_seconds: report.step_seconds,
						input: step_input,
						local_owner: step_input.local_player().map(|target| &target.key),
					},
					&mut ActionWriter {
						actions: &mut self.actions,
					},
				)?;
				let projectile_step = ProjectileHookStep {
					tick: self.tick + u64::from(steps) + 1,
					step_seconds: report.step_seconds,
				};
				crate::projectile::plan_launches(&self.world, &mut lifecycle, step_input)?;
				let projectile_hits = crate::projectile::plan_motion(
					&self.world,
					&self.collisions,
					&lifecycle,
					projectile_step,
					&mut collision_candidates_remaining,
					&mut ActionWriter { actions: &mut self.actions },
				)?;
				for hit in projectile_hits {
					let mut output = ActionWriter { actions: &mut self.actions };
					let impact = object_scripts.plan_hit(&self.world, &lifecycle, projectile_step, &hit.event, &mut output)?;
					if let Some(settings) = impact.or_else(|| hit.impact.cloned()) {
						output.push(SimulationAction::EmitParticleBurst { position: hit.event.position, settings })?;
					}
				}
				object_scripts.stage_triggers(&lifecycle, &trigger_events[event_start..])?;
				let look_rate = step_input.controller_look;
				let resident_regions = self.resident_regions();
				self.module.plan_with_progress(
					GameplayContext {
						world: &self.world,
						collisions: &self.collisions,
						input: step_input,
						camera: camera.camera(),
						step_seconds: report.step_seconds,
						progress: progress_transaction.view(),
						triggers: &trigger_events[event_start..],
						resident_regions: &resident_regions,
						trigger_overlaps: triggers.view(),
						lifecycle: &lifecycle,
					},
					&mut ActionWriter {
						actions: &mut self.actions,
					},
				)?;
				player_control::apply_local_player(
					&self.world,
					&mut characters,
					step_input,
					camera.camera(),
					controls.as_ref().map(|(graph, _)| graph.intent.jump),
				)?;
				let route_input = (!scene_request_pending && !self.project.scene_routes.is_empty())
					.then(|| step_input.action_presses.clone());
				let owner = step_input.local_player().map(|target| &target.key);
				let mut acknowledged = HashSet::new();
				for action in &self.actions {
					if let SimulationAction::AcceptAction { actor, action } = action {
						if owner != Some(actor) || !step_input.action_pressed(action) {
							return Err("Gameplay action acknowledgement must match a pressed action of the local player".into());
						}
						acknowledged.insert(action.clone());
					}
				}
				// Later catch-up steps see held state only. A rejected tick
				// rolls back its actions and clears all input on pause.
				let mut step_actions = std::mem::take(&mut self.inputs.action_presses);
				self.inputs.control_sample.finish_step();
				if sequence_gates_input {
					step_actions.clear();
				} else if controls.is_some() {
					step_actions = acknowledged;
				}
				let count = characters.keys().count();
				actions_applied += count;
				if actions_applied > Self::MAX_ACTIONS_PER_TICK {
					return Err("Character action budget exceeded".into());
				}
				let no_cosmetics = std::collections::BTreeMap::new();
				let cosmetics = controls
					.as_ref()
					.map_or(&no_cosmetics, |(graph, _)| &graph.presentation);
				let root_motion = if animations.has_root_motion() {
					let previous_motion = animation_motion(&characters, &lifecycle);
					let previous_jumps: HashSet<_> = characters
						.keys()
						.filter(|key| {
							characters
								.state(key)
								.is_some_and(|state| state.jumped_at_tick == Some(tick - 1))
						})
						.cloned()
						.collect();
					let local_player = self.inputs.local_player().map(|target| &target.key);
					let mut actions = step_actions.clone();
					if self.inputs.controller_owned {
						actions.remove("jump");
						if local_player.is_some_and(|key| previous_jumps.contains(key)) {
							actions.insert("jump".into());
						}
					}
					animations.prepare_root_motion(&crate::animation::AnimatorTick {
						motions: &previous_motion,
						jumped: &previous_jumps,
						local_player,
						accepted_actions: &actions,
						cosmetic: cosmetics,
						fixed_hz: self.project.fixed_hz,
						tick,
					}, |key| sequences.owns_actor(key))?
				} else {
					std::collections::BTreeMap::new()
				};
				let character_change = characters.step(
					&mut self.world,
					&mut self.collisions,
					crate::character_controller::CharacterStep {
						tick,
						seconds: report.step_seconds,
						root_motion: &root_motion,
						region_movement: &region_movement,
					},
					&mut collision_candidates_remaining,
					&mut before,
				)?;
				if controls.is_some()
					&& self
						.inputs
						.local_player()
						.and_then(|target| characters.state(&target.key))
						.is_some_and(|state| state.jumped_at_tick == Some(tick))
				{
					step_actions.insert("jump".into());
				}
				if self.inputs.controller_owned && !step_actions.is_empty() {
					let mut edges = step_actions.iter().cloned().collect::<Vec<_>>();
					edges.sort_unstable();
					accepted_action_presses.extend(
						edges
							.into_iter()
							.map(|id| CommittedInputAction { id, tick }),
					);
				}
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
				// Execute structural actions in submitted order after ordinary actions; despawn last.
				self.actions.sort_by_key(|action| match action {
					SimulationAction::Despawn { .. } => 2,
					SimulationAction::Reparent { .. }
					| SimulationAction::SetParticleEmitterEnabled { .. } => 1,
					_ => 0,
				});
				let mut removed_handles = HashSet::new();
				let mut particle_bursts = Vec::new();
				for action in self.actions.drain(..) {
					let forbidden = match &action {
						SimulationAction::Move { entity, .. }
						| SimulationAction::Rotate { entity, .. }
						| SimulationAction::SetLocalYaw { entity, .. }
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
							object_scripts.plan_destroy(&self.world, &lifecycle, projectile_step, &keys)?;
							structural_work =
								structural_work.saturating_add(self.world.data().entities.len());
							if structural_work > Self::MAX_STRUCTURAL_WORK_PER_TICK {
								return Err("Runtime structural work budget exceeded".into());
							}
							for key in &keys {
								if let Some(handle) = lifecycle.handle(key) {
									removed_handles.insert(handle);
								}
							}
							structural_before
								.get_or_insert_with(|| restore_entities(&self.world, &mut before));
							let change = self.world.remove_runtime_entities(&keys)?;
							if self
								.world
								.matrix_updates()
								.saturating_sub(initial_matrix_updates)
								> Self::MAX_MATRIX_UPDATES_PER_TICK
							{
								return Err("Runtime despawn matrix update budget exceeded".into());
							}
							self.collisions =
								CollisionWorld::with_matrix(&self.world, self.collisions.matrix())?;
							lifecycle.removed(&keys)?;
							object_scripts.prune(&lifecycle, &mut retired_scripts);
							characters.prune(&self.world);
							lifecycle.prune_states(&self.world);
							triggers.invalidate();
							changes.merge(change);
							continue;
						}
						SimulationAction::Reparent {
							target,
							parent,
							mode,
						} => {
							lifecycle.validate_reparent(&target, parent.as_ref())?;
							structural_work =
								structural_work.saturating_add(self.world.data().entities.len());
							if structural_work > Self::MAX_STRUCTURAL_WORK_PER_TICK {
								return Err("Runtime structural work budget exceeded".into());
							}
							let Some(next) = self.world.reparent_candidate(
								&target.key,
								parent.as_ref().map(|parent| &parent.key),
								mode,
							)?
							else {
								continue;
							};
							if next.matrix_updates().saturating_sub(initial_matrix_updates)
								> Self::MAX_MATRIX_UPDATES_PER_TICK
							{
								return Err("Runtime reparent matrix update budget exceeded".into());
							}
							characters.reconcile(&next, &self.project)?;
							let collisions =
								CollisionWorld::with_matrix(&next, self.collisions.matrix())?;
							lifecycle.advance_topology()?;
							structural_before
								.get_or_insert_with(|| restore_entities(&self.world, &mut before));
							let camera_changed = next.data().camera != self.world.data().camera;
							self.world = next;
							self.collisions = collisions;
							triggers.invalidate();
							changes.merge(WorldChange {
								topology_changed: true,
								full_resync: true,
								camera_changed,
								..WorldChange::default()
							});
							continue;
						}
						SimulationAction::SetParticleEmitterEnabled {
							target,
							component_id,
							enabled,
						} => {
							lifecycle.validate_handle(&target)?;
							structural_work =
								structural_work.saturating_add(self.world.data().entities.len());
							if structural_work > Self::MAX_STRUCTURAL_WORK_PER_TICK {
								return Err("Runtime structural work budget exceeded".into());
							}
							let index = self
								.world
								.entity_index(&target.key)
								.ok_or("Runtime particle emitter object is missing")?;
							before
								.entry(target.key.clone())
								.or_insert_with(|| self.world.data().entities[index].clone());
							let Some(entity) = self.world.set_runtime_emitter_enabled(
								index,
								&component_id,
								enabled,
							)?
							else {
								continue;
							};
							let particles =
								staged_particles.get_or_insert_with(|| self.particles.clone());
							particles_changed |= particles.sync_runtime_emitter(entity)?;
							lifecycle.advance_topology()?;
							changes.merge(WorldChange {
								changed_entities: vec![target.key],
								..WorldChange::default()
							});
							continue;
						}
						SimulationAction::EmitParticleBurst { position, settings } => {
							particle_bursts.push((position, settings));
							continue;
						}
						SimulationAction::SetAnimatorBool { entity, parameter, value } => {
							animations.set_script_bool(&entity, parameter, value)?;
							continue;
						}
						SimulationAction::SetLocalPossession { target } => {
							let change = possession::PossessionChange { target };
							possession.validate(&change, &self.world, &lifecycle, &characters)?;
							possession::LocalPossession::stage(&mut requested_possession, change)?;
							continue;
						}
						SimulationAction::DriveCharacter {
							target,
							movement_xz,
							jump_held,
						} => {
							lifecycle.validate_handle(&target)?;
							if possession.owner.as_ref() == Some(&target) {
								return Err(
									"Local input already owns the requested character motor".into(),
								);
							}
							characters.set_input(&target.key, movement_xz, jump_held)?;
							continue;
						}
						SimulationAction::Progress(action) => {
							progress_transaction.apply(&action)?;
							continue;
						}
						SimulationAction::AcceptAction { .. } => continue,
						SimulationAction::PlayAnimation {
							entity,
							clip_index,
							looping,
						} => {
							if !sequences.owns_actor(&entity) {
								animations.play_clip(&entity, clip_index, looping)?;
							}
							continue;
						}
						SimulationAction::SelectCamera { entity } => {
							let target = entity
								.as_ref()
								.map(|key| {
									lifecycle
										.handle(key)
										.ok_or("Runtime camera object does not exist")
								})
								.transpose()?;
							camera_objects.stage_override(target, &self.world, &lifecycle)?;
							continue;
						}
						SimulationAction::AttachToView { entity } => {
							view_attachments.push(entity);
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
						| SimulationAction::AcceptAction { .. }
						| SimulationAction::PlayAnimation { .. }
						| SimulationAction::AttachToView { .. }
						| SimulationAction::SelectCamera { .. }
						| SimulationAction::SetLocalPossession { .. }
						| SimulationAction::DriveCharacter { .. }
						| SimulationAction::Reparent { .. }
						| SimulationAction::SetParticleEmitterEnabled { .. }
						| SimulationAction::EmitParticleBurst { .. }
						| SimulationAction::SetAnimatorBool { .. }
						| SimulationAction::Despawn { .. } => {
							unreachable!("Non-world actions were handled above");
						}
						SimulationAction::Move { entity, .. }
						| SimulationAction::Rotate { entity, .. }
						| SimulationAction::SetVisible { entity, .. }
						| SimulationAction::SetCollisionEnabled { entity, .. }
						| SimulationAction::SetMaterial { entity, .. }
						| SimulationAction::SetLocalYaw { entity, .. }
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
						| SimulationAction::AcceptAction { .. }
						| SimulationAction::PlayAnimation { .. }
						| SimulationAction::AttachToView { .. }
						| SimulationAction::SelectCamera { .. }
						| SimulationAction::SetLocalPossession { .. }
						| SimulationAction::DriveCharacter { .. }
						| SimulationAction::Reparent { .. }
						| SimulationAction::SetParticleEmitterEnabled { .. }
						| SimulationAction::EmitParticleBurst { .. }
						| SimulationAction::SetAnimatorBool { .. }
						| SimulationAction::Despawn { .. } => {
							unreachable!("Non-world actions were handled above");
						}
						SimulationAction::Move {
							entity,
							world_delta,
						} => {
							let body = region_movement.legacy_body(
								&self.world,
								&self.collisions,
								index,
								&mut collision_candidates_remaining,
							)?;
							self.collisions
								.move_entity_with_limit(
									&mut self.world,
									&entity,
									world_delta,
									&mut collision_candidates_remaining,
									|delta| body.map_or(delta, |body| region_movement.clamp(body, delta).delta),
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
						SimulationAction::SetLocalYaw { entity, radians } => {
							let mut transform = self.world.data().entities[index].transform.clone();
							transform.rotation[1] = radians;
							let changes = self.world.set_transform(&entity, transform)?;
							self.collisions.synchronize(&self.world, &changes)?;
							changes
						}
						SimulationAction::SetVisible { entity, visible } => {
							if visible
								&& self
									.world
									.entity_index(&entity)
									.is_none_or(|index| self.world.data().entities[index].active)
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
				sequences.abort_stale(
					&self.world,
					&lifecycle,
					tick,
					&mut sequence_events,
					&mut audio_onsets,
				);
				camera_objects.sequence_camera = sequences.camera().cloned();
				let before_camera = camera.camera().clone();
				let camera_tick = self.tick + u64::from(steps) + 1;
				let switched = camera_objects.advance(
					&self.world,
					&lifecycle,
					&mut camera,
					camera_tick,
					camera_objects::CameraObstructionStep {
						exclusions: &mut camera_exclusions,
						collisions: &self.collisions,
						budget: &mut collision_candidates_remaining,
					},
				)?;
				let clear_camera_input = if switched {
					true
				} else {
					camera.step(
						&self.world,
						&lifecycle,
						report.step_seconds,
						player_control::step_camera_intent(
							camera_delta,
							look_rate,
							report.step_seconds,
						),
						camera_tick,
						obstruction_query(
							&self.world,
							&lifecycle,
							&self.collisions,
							&mut camera_exclusions,
							&mut collision_candidates_remaining,
						),
					)?
				};
				if clear_camera_input {
					camera_input.clear();
					camera_exclusions = None;
				}
				if camera_pose_changed(&before_camera, camera.camera()) {
					changes.merge(WorldChange {
						camera_changed: true,
						..WorldChange::default()
					});
				}
				for entity in view_attachments.drain(..) {
					let index = self
						.world
						.entity_index(&entity)
						.ok_or("View attachment target is missing")?;
					let visual = &self.world.data().entities[index];
					if !self.world.is_root_leaf(index)
						|| visual.collider.is_some()
						|| visual.camera.is_some()
						|| camera.tracks_entity(&entity)
					{
						return Err(
							"View attachment requires an independent root visual with no collider"
								.into(),
						);
					}
					let pose = camera.camera();
					let rotation = glam::Quat::from_rotation_y(-pose.yaw)
						* glam::Quat::from_rotation_x(pose.pitch);
					let (x, y, z) = rotation.to_euler(glam::EulerRot::XYZ);
					let transform = crate::scene_data::TransformData {
						position: pose.position,
						rotation: [x, y, z],
						scale: visual.transform.scale,
					};
					if transform != visual.transform {
						if self
							.world
							.matrix_updates()
							.saturating_sub(initial_matrix_updates)
							>= Self::MAX_MATRIX_UPDATES_PER_TICK
						{
							return Err("Simulation tick matrix update budget exceeded".into());
						}
						before
							.entry(entity.clone())
							.or_insert_with(|| visual.clone());
						changes.merge(self.world.set_transform(&entity, transform)?);
					}
				}
				let motions = animation_motion(&characters, &lifecycle);
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
				if animations.has_animators() {
					let tick = self.tick + u64::from(steps) + 1;
					let jumped = characters
						.keys()
						.filter(|key| {
							characters
								.state(key)
								.is_some_and(|state| state.jumped_at_tick == Some(tick))
						})
						.cloned()
						.collect();
					let local_player = self
						.inputs
						.local_player()
						.map(|target| &target.key)
						.filter(|key| characters.contains(key));
					let animated = animations.step_animators(
						&self.world,
						crate::animation::AnimatorTick {
							motions: &motions,
							jumped: &jumped,
							local_player,
							accepted_actions: &step_actions,
							cosmetic: cosmetics,
							fixed_hz: self.project.fixed_hz,
							tick,
						},
						&mut audio_onsets,
						|key| sequences.owns_actor(key),
						|key, sole, reach| self.collisions.foot_support(
							&self.world, key, sole, reach, &mut collision_candidates_remaining,
						),
					)?;
					changes.merge(WorldChange {
						changed_entities: animated,
						..WorldChange::default()
					});
				}
				changes.merge(WorldChange {
					changed_entities: animations.set_sequence_poses(&self.world, sequences.sampled_clips())?,
					..WorldChange::default()
				});
				if sequence_gates_input && !sequences.needs_tick() {
					if let Some(input) = &mut self.conditional_input {
						input.retarget(possession.owner.clone());
					}
					self.inputs.reset_local_controls(
						possession.owner.clone(),
						possession.managed,
						true,
						self.control_graph.is_some(),
					);
					camera_input.clear();
				}
				control_graph_step = controls.and_then(|sample| {
					let owner = &self.inputs.local_player()?.key;
					let previous = control_graph_step
						.as_ref()
						.or(self.control_graph_step.as_ref());
					control_graph::CommittedControlStep::capture(
						&self.world,
						&lifecycle,
						owner,
						camera_tick,
						sample,
						previous,
					)
				});
				if (requested_possession.is_some() || !removed_handles.is_empty())
					&& let Some(previous) =
						possession.finish_step(requested_possession, &lifecycle)?
				{
					self.commit_possession_input(
						previous.target.as_ref().map(|target| &target.key),
						&possession,
						&mut characters,
					);
					camera_input.clear();
					control_graph_step = None;
				}
				if !particle_bursts.is_empty() && staged_particles.is_none() {
					staged_particles = Some(self.particles.clone());
				}
				if let Some(particles) = &mut staged_particles {
					particles_changed |= particles.step(&self.world, self.project.fixed_hz);
					for (position, settings) in particle_bursts {
						particles_changed |= particles.burst(position, &settings)?;
					}
				}
				steps += 1;
				if !scene_request_pending && requested_scene.is_none()
					&& let Some(route) = scene_flow::select_route(scene_flow::RouteStep {
					project: &self.project,
					scene_id: &self.scene_id,
					tick,
					player: possession.owner.as_ref().map(|actor| &actor.key),
					triggers: &trigger_events[event_start..],
					overlaps: triggers.view(),
					presses: route_input.as_ref(),
					sequence_events: &sequence_events,
					module: self.module.as_ref(),
					progress: progress_transaction.view(),
				})
				{
					requested_scene = Some(route.to.clone());
					requested_route = Some(route.id.clone());
				}
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
			let restored =
				structural_before.unwrap_or_else(|| restore_entities(&self.world, &mut before));
			self.world
				.restore_runtime_document(restored, initial_matrix_updates)?;
			self.collisions = CollisionWorld::with_matrix(&self.world, self.collisions.matrix())?;
			if let Some((input, layout)) = self.conditional_input.as_mut().zip(input_layout) {
				input.restore_selection(layout);
			}
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
		outcome.particles_changed = if let Some(particles) = staged_particles {
			self.particles = particles;
			particles_changed
		} else {
			steps > 0 && self.particles.step(&self.world, self.project.fixed_hz)
		};
		outcome.dropped_seconds = report.dropped_seconds
			+ f64::from(report.steps - steps - pending_steps) / f64::from(self.project.fixed_hz);
		outcome.changes = changes.finish();
		if steps > 0 {
			self.control_graph_step = control_graph_step;
		}
		outcome.requested_scene = requested_scene;
		outcome.requested_route = requested_route;
		outcome.sequence_events = sequence_events;
		self.sequences = sequences;
		self.triggers = triggers;
		self.inputs
			.movement
			.retain(|key, _| self.world.entity_index(key).is_some());
		if let Some(available) = &mut self.visibility_capabilities {
			available.retain(|key| self.world.entity_index(key).is_some());
		}
		self.lifecycle = lifecycle;
		outcome.possession_changed = self.possession.generation != possession.generation;
		self.possession = possession;
		self.object_scripts = object_scripts;
		retired_scripts.stop();
		self.characters = characters;
		self.animations = animations;
		self.animation_bindings
			.retain(|key, _| self.world.entity_index(key).is_some());
		outcome.camera_status_changed = self.camera_objects.active != camera_objects.active
			|| self.camera_input != camera_input
			|| self
				.camera
				.orbit_state()
				.zip(camera.orbit_state())
				.is_some_and(|(previous, next)| !previous.same_status(next));
		self.camera = camera;
		self.camera_objects = camera_objects;
		self.camera_input = camera_input;
		self.camera_exclusions = camera_exclusions;
		outcome.trigger_events = trigger_events;
		outcome.audio_onsets = audio_onsets;
		outcome.accepted_action_presses = accepted_action_presses;
		Ok(outcome)
	}
}
fn animation_motion(
	characters: &crate::character_controller::CharacterRuntime,
	lifecycle: &crate::runtime_lifecycle::RuntimeLifecycle,
) -> std::collections::BTreeMap<EntityKey, crate::animation::AnimationMotion> {
	characters
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
		.collect()
}

fn restore_entities(
	world: &RuntimeWorld,
	before: &mut HashMap<EntityKey, EntityData>,
) -> Arc<SceneData> {
	let mut restored = world.data().clone();
	for entity in &mut restored.entities {
		if let Some(original) = before.remove(&entity.key) {
			*entity = original;
		}
	}
	Arc::new(restored)
}

fn camera_pose_changed(before: &crate::Camera, after: &crate::Camera) -> bool {
	before.position != after.position
		|| before.yaw != after.yaw
		|| before.pitch != after.pitch
		|| before.vertical_fov != after.vertical_fov
		|| before.near != after.near
		|| before.far != after.far
}
