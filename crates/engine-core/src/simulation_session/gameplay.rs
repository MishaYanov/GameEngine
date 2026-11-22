use super::InputFrame;
use crate::{
	EntityKey,
	collision::CollisionWorld,
	game_progress::{GameProgress, ProgressAction, ProgressState, ProgressView},
	project::{BehaviorSettings, BehaviorSpace, ProjectData, ResolvedBehavior},
	scene_catalog::SceneId,
	scene_data::{MaterialOverrideData, SceneData, TransformData},
	triggers::{TriggerEvent, TriggerView},
	world::RuntimeWorld,
};
use glam::Vec3;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

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
	/// Select a clip on an enabled, prepared animation for this Play run.
	PlayAnimation {
		entity: EntityKey,
		clip_index: u32,
		looping: bool,
	},
	/// Select a live Camera object at the end of the fixed step. None restores the authored default.
	SelectCamera {
		entity: Option<EntityKey>,
	},
	/// Place a root visual at the final camera pose after movement and look input.
	AttachToView {
		entity: EntityKey,
	},
}
pub struct ActionWriter<'a> {
	pub(super) actions: &'a mut Vec<SimulationAction>,
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
	pub collisions: &'a CollisionWorld,
	pub input: &'a InputFrame,
	/// The camera committed by the previous fixed step.
	pub camera: &'a crate::Camera,
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

	/// Projects live affordances from committed state. Ordinary status has no spatial inputs.
	fn status_in_world(
		&self,
		_world: &RuntimeWorld,
		_collisions: &CollisionWorld,
		progress: ProgressView<'_>,
	) -> Option<GameplayStatus> {
		self.status(progress)
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
	pub(super) project: &'a ProjectData,
	pub(super) scene_id: &'a SceneId,
	pub(super) document: &'a SceneData,
	pub(super) behaviors: Vec<ResolvedBehavior>,
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
