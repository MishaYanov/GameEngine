use engine_core::{
	EntityKey,
	command::GameplayStatus,
	game_progress::ProgressView,
	scene_data::MaterialOverrideData,
	simulation_session::{ActionWriter, GameModule, GameplayContext, InputFrame, SimulationAction},
	triggers::TriggerPhase,
	world::RuntimeWorld,
};

use super::{
	CampaignProgress, ProgressAction,
	preparation::{CourseRoles, FinishRoles},
	settings::SceneInteractions,
};

pub(super) struct CampaignModule {
	base: Box<dyn GameModule>,
	roles: Option<CourseRoles>,
	finish: Option<FinishRoles>,
	interactions: Option<SceneInteractions>,
}

pub(super) const INTERACTION_RADIUS: f32 = 0.35;

impl CampaignModule {
	pub(super) fn new(
		base: Box<dyn GameModule>,
		roles: Option<CourseRoles>,
		finish: Option<FinishRoles>,
		interactions: Option<SceneInteractions>,
	) -> Self {
		Self {
			base,
			roles,
			finish,
			interactions,
		}
	}

	fn pending_objective(
		&self,
		world: &RuntimeWorld,
		progress: &CampaignProgress,
	) -> Option<ProgressAction> {
		if self.interactions.is_some() {
			return None;
		}
		if progress.completed {
			return None;
		}

		if let Some(finish) = &self.finish {
			if progress.courtyard_exit_reached
				&& within_radius(world, &finish.player, &finish.marker, INTERACTION_RADIUS)
			{
				return Some(ProgressAction::CompleteRun);
			}

			return None;
		}

		let roles = self.roles.as_ref()?;

		if !progress.has_courtyard_key
			&& within_radius(world, &roles.player, &roles.key, INTERACTION_RADIUS)
		{
			return Some(ProgressAction::CollectCourtyardKey);
		}

		if progress.has_courtyard_key
			&& !progress.courtyard_exit_reached
			&& within_radius(world, &roles.player, &roles.exit, INTERACTION_RADIUS)
		{
			return Some(ProgressAction::ReachCourtyardExit);
		}

		None
	}

	fn feedback_needed(&self, world: &RuntimeWorld, progress: &CampaignProgress) -> bool {
		let Some(roles) = &self.roles else {
			return false;
		};
		if !progress.has_courtyard_key {
			return false;
		}
		let needs =
			|key: &EntityKey, predicate: &dyn Fn(&engine_core::scene_data::EntityData) -> bool| {
				world
					.entity_index(key)
					.is_some_and(|index| predicate(&world.data().entities[index]))
			};
		needs(&roles.key, &|entity| {
			entity.renderable || entity.collision_enabled
		}) || needs(&roles.exit, &|entity| {
			entity
				.material
				.as_ref()
				.is_none_or(|material| material.base_color_factor != [0.1, 0.9, 0.25, 1.0])
		}) || matches!(&self.interactions, Some(SceneInteractions::Course { key_trigger, .. }) if needs(key_trigger, &|entity| entity.collision_enabled))
			|| matches!(&self.interactions, Some(SceneInteractions::Course { exit_blocker: Some(blocker), .. }) if needs(blocker, &|entity| entity.collision_enabled))
	}

	fn show_unlocked(&self, output: &mut ActionWriter<'_>) -> Result<(), String> {
		let Some(roles) = &self.roles else {
			return Ok(());
		};
		output.push(SimulationAction::SetVisible {
			entity: roles.key.clone(),
			visible: false,
		})?;
		output.push(SimulationAction::SetCollisionEnabled {
			entity: roles.key.clone(),
			enabled: false,
		})?;
		output.push(SimulationAction::SetMaterial {
			entity: roles.exit.clone(),
			material: Some(MaterialOverrideData {
				base_color_factor: [0.1, 0.9, 0.25, 1.0],
			}),
		})?;
		if let Some(SceneInteractions::Course {
			key_trigger,
			exit_blocker,
			..
		}) = &self.interactions
		{
			output.push(SimulationAction::SetCollisionEnabled {
				entity: key_trigger.clone(),
				enabled: false,
			})?;
			if let Some(blocker) = exit_blocker {
				output.push(SimulationAction::SetCollisionEnabled {
					entity: blocker.clone(),
					enabled: false,
				})?;
			}
		}
		Ok(())
	}
}

fn within_radius(world: &RuntimeWorld, first: &EntityKey, second: &EntityKey, radius: f32) -> bool {
	let (Some(first_index), Some(second_index)) =
		(world.entity_index(first), world.entity_index(second))
	else {
		return false;
	};

	let first_position = world.world_matrices()[first_index].w_axis.truncate();

	let second_position = world.world_matrices()[second_index].w_axis.truncate();

	first_position.distance_squared(second_position) <= radius * radius
}

impl GameModule for CampaignModule {
	fn status(&self, progress: ProgressView<'_>) -> Option<GameplayStatus> {
		let progress = progress.get::<CampaignProgress>()?;
		let objective = if self.roles.is_none() && self.finish.is_none() {
			return None;
		} else if progress.completed {
			"Courtyard Run complete"
		} else if self.finish.is_some() {
			if progress.courtyard_exit_reached {
				"Reach the finish marker"
			} else {
				"Start in the courtyard to unlock the finish"
			}
		} else if progress.courtyard_exit_reached {
			"Courtyard exit reached"
		} else if progress.has_courtyard_key {
			"Reach the courtyard exit"
		} else {
			"Find the courtyard key"
		};
		let detail = if progress.has_courtyard_key {
			"Courtyard key collected"
		} else {
			"Courtyard key not collected"
		};
		Some(GameplayStatus {
			objective: objective.into(),
			detail: Some(detail.into()),
			score: Some(progress.score),
			completed: progress.completed,
		})
	}

	fn needs_tick(&self, input: &InputFrame) -> bool {
		self.base.needs_tick(input)
	}
	fn needs_tick_with_progress(
		&self,
		world: &RuntimeWorld,
		input: &InputFrame,
		progress: ProgressView<'_>,
	) -> bool {
		let campaign = progress.get::<CampaignProgress>();
		if campaign.is_some_and(|value| value.completed) {
			return false;
		}
		self.base.needs_tick_with_progress(world, input, progress)
			|| campaign.is_some_and(|value| {
				self.pending_objective(world, value).is_some() || self.feedback_needed(world, value)
			})
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

		if progress.completed {
			return Ok(());
		}

		let entered = |sensor: &EntityKey, player: &EntityKey| {
			context.triggers.iter().any(|event| {
				event.phase == TriggerPhase::Enter
					&& &event.sensor == sensor
					&& &event.other == player
			})
		};
		let mut collected = false;
		let objective = match &self.interactions {
			Some(SceneInteractions::Course {
				key_trigger,
				exit_trigger,
				..
			}) => {
				let roles = self.roles.as_ref().ok_or("Course roles unavailable")?;
				collected = !progress.has_courtyard_key && entered(key_trigger, &roles.player);
				if collected {
					output
						.push_progress::<CampaignProgress>(ProgressAction::CollectCourtyardKey)?;
				}
				if (progress.has_courtyard_key || collected)
					&& !progress.courtyard_exit_reached
					&& context
						.trigger_overlaps
						.contains(exit_trigger, &roles.player)
				{
					Some(ProgressAction::ReachCourtyardExit)
				} else {
					None
				}
			}
			Some(SceneInteractions::Finish { finish_trigger }) => {
				let finish = self.finish.as_ref().ok_or("Finish roles unavailable")?;
				if progress.courtyard_exit_reached
					&& context
						.trigger_overlaps
						.contains(finish_trigger, &finish.player)
				{
					Some(ProgressAction::CompleteRun)
				} else {
					None
				}
			}
			None => self.pending_objective(context.world, progress),
		};
		if collected
			|| matches!(&objective, Some(ProgressAction::CollectCourtyardKey))
			|| self.feedback_needed(context.world, progress)
		{
			self.show_unlocked(output)?;
		}

		if matches!(&objective, Some(ProgressAction::CompleteRun)) {
			return output.push_progress::<CampaignProgress>(ProgressAction::CompleteRun);
		}

		self.base.plan_with_progress(context, output)?;

		if let Some(action) = objective {
			let reached_exit = matches!(&action, ProgressAction::ReachCourtyardExit);
			output.push_progress::<CampaignProgress>(action)?;
			if reached_exit && let Some(roles) = &self.roles {
				output.push(SimulationAction::RequestScene {
					scene_id: roles.destination.clone(),
				})?;
			}
		}

		Ok(())
	}
}
