//! Gameplay for projects that opt in to the Courtyard Run module.

use engine_core::{
	EntityKey,
	game_progress::{GameProgress, ProgressState, ProgressView},
	project::{BehaviorSettings, ProjectData, ResolvedBehavior},
	scene_catalog::SceneId,
	simulation_session::{
		ActionWriter, ConfiguredBehaviorFactory, GameModule, GameModuleFactory, GameplayContext,
		InputFrame, PreparedModuleConfig, resolve_default_gameplay,
	},
	world::RuntimeWorld,
};
use std::{collections::HashSet, mem::size_of, sync::Arc};

pub const COURTYARD_RUN_ID: &str = "courtyard_run";

pub fn resolve(project: &ProjectData) -> Result<Arc<dyn GameModuleFactory>, String> {
	if project.gameplay_module.as_deref() == Some(COURTYARD_RUN_ID) {
		return Ok(Arc::new(CampaignFactory));
	}
	resolve_default_gameplay(project)
}

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

#[cfg(test)]
mod tests {
	use super::*;
	use engine_core::{SceneData, game_session::GameSession};

	#[test]
	fn bundled_project_opts_in_to_campaign_progress() {
		let mut project =
			ProjectData::from_json(include_str!("../../../apps/game/project.json")).unwrap();
		let document = Arc::new(
			SceneData::from_json(include_str!(
				"../../../apps/game/assets/scenes/courtyard.json"
			))
			.unwrap(),
		);
		let courtyard = SceneId::new("courtyard").unwrap();
		let factory = resolve(&project).unwrap();
		let mut campaign = GameSession::with_factory_for_scene(
			Arc::new(project.clone()),
			courtyard.clone(),
			Arc::clone(&document),
			factory,
		)
		.unwrap();
		assert!(
			!campaign
				.progress::<CampaignProgress>()
				.unwrap()
				.has_courtyard_key
		);

		campaign
			.active_mut()
			.set_controlled_input([1.0, 0.0, 0.0])
			.unwrap();
		for _ in 0..70 {
			campaign.tick(1.0 / f64::from(project.fixed_hz)).unwrap();
		}
		assert!(
			campaign
				.progress::<CampaignProgress>()
				.unwrap()
				.has_courtyard_key
		);
		assert_eq!(campaign.progress_revision(), 1);

		project.gameplay_module = None;
		let factory = resolve(&project).unwrap();
		let generic =
			GameSession::with_factory_for_scene(Arc::new(project), courtyard, document, factory)
				.unwrap();
		assert!(generic.progress::<CampaignProgress>().is_none());
	}
}
