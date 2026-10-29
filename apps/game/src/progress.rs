use runtime::{
	EntityKey,
	game_progress::{GameProgress, ProgressState, ProgressView},
	project::{BehaviorSettings, ProjectData, ResolvedBehavior},
	scene_catalog::SceneId,
	simulation_session::{
		ActionWriter, ConfiguredBehaviorFactory, GameModule, GameModuleFactory, GameplayContext,
		InputFrame, PreparedModuleConfig,
	},
	world::RuntimeWorld,
};
use std::{collections::HashSet, mem::size_of};

#[derive(Clone, Default, PartialEq, Eq)]
pub struct CampaignProgress {
	pub score: i32,
	pub has_courtyard_key: bool,
}

pub enum ProgressAction {
	AwardScore { amount: i32 },
	UnlockCourtyard,
}

impl GameProgress for CampaignProgress {
	type Action = ProgressAction;
	fn validate(&self) -> Result<(), String> {
		if self.score < 0 {
			return Err("Campaign score must be nonnegative".into());
		}
		Ok(())
	}
	fn retained_bytes(&self) -> usize {
		size_of::<Self>()
	}
	fn action_bytes(_: &ProgressAction) -> usize {
		size_of::<ProgressAction>()
	}
	fn apply(&mut self, action: &ProgressAction) -> Result<(), String> {
		match action {
			ProgressAction::AwardScore { amount } => {
				if *amount < 0 {
					return Err("Score awards must be nonnegative".into());
				}
				self.score = self
					.score
					.checked_add(*amount)
					.ok_or("Campaign score overflow")?;
			}
			ProgressAction::UnlockCourtyard => self.has_courtyard_key = true,
		}
		Ok(())
	}
}

pub struct CampaignFactory;
impl GameModuleFactory for CampaignFactory {
	fn create_progress(&self) -> Result<ProgressState, String> {
		ProgressState::new(CampaignProgress::default())
	}
	fn create(
		&self,
		project: &ProjectData,
		scene_id: &SceneId,
	) -> Result<Box<dyn GameModule>, String> {
		let targets = controlled_targets(&project.resolved_behaviors_for(scene_id)?);
		let base = ConfiguredBehaviorFactory.create(project, scene_id)?;
		Ok(Box::new(CampaignModule { base, targets }))
	}
	fn create_prepared(
		&self,
		config: PreparedModuleConfig<'_>,
	) -> Result<Box<dyn GameModule>, String> {
		let targets = controlled_targets(config.behaviors());
		let base = ConfiguredBehaviorFactory.create_prepared(config)?;
		Ok(Box::new(CampaignModule { base, targets }))
	}
}

fn controlled_targets(behaviors: &[ResolvedBehavior]) -> Vec<EntityKey> {
	let mut seen = HashSet::new();
	behaviors
		.iter()
		.filter(|behavior| {
			matches!(&behavior.settings, BehaviorSettings::ControlledMove { .. })
				&& seen.insert(behavior.entity.clone())
		})
		.map(|behavior| behavior.entity.clone())
		.collect()
}

struct CampaignModule {
	base: Box<dyn GameModule>,
	targets: Vec<EntityKey>,
}

impl CampaignModule {
	fn key_ready(&self, world: &RuntimeWorld, progress: &CampaignProgress) -> bool {
		!progress.has_courtyard_key
			&& self.targets.iter().any(|key| {
				world
					.entity_index(key)
					.is_some_and(|index| world.world_matrices()[index].w_axis.x >= 1.0)
			})
	}
}

impl GameModule for CampaignModule {
	fn needs_tick(&self, input: &InputFrame) -> bool {
		self.base.needs_tick(input)
	}
	fn needs_tick_with_progress(
		&self,
		world: &RuntimeWorld,
		input: &InputFrame,
		progress: ProgressView<'_>,
	) -> bool {
		self.base.needs_tick_with_progress(world, input, progress)
			|| progress
				.get::<CampaignProgress>()
				.is_some_and(|value| self.key_ready(world, value))
	}
	fn plan(
		&self,
		world: &RuntimeWorld,
		input: &InputFrame,
		step: f32,
		output: &mut ActionWriter<'_>,
	) -> Result<(), String> {
		self.base.plan(world, input, step, output)
	}
	fn plan_with_progress(
		&self,
		context: GameplayContext<'_>,
		output: &mut ActionWriter<'_>,
	) -> Result<(), String> {
		let progress = context
			.progress
			.get::<CampaignProgress>()
			.ok_or("Campaign progress is unavailable")?;
		let unlock = self.key_ready(context.world, progress);
		self.base.plan_with_progress(context, output)?;
		if unlock {
			output.push_progress::<CampaignProgress>(ProgressAction::UnlockCourtyard)?;
		}
		Ok(())
	}
}
