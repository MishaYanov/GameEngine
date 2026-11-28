//! Scene route facts are read from a successful fixed-step candidate.
use super::{GameModule, SimulationSession};
use crate::{
	EntityKey,
	game_progress::ProgressView,
	project::{ProjectData, SceneRoute, SceneRouteActivation, SceneRouteCondition},
	scene_catalog::SceneId,
	sequence::SequenceEvent,
	triggers::{TriggerEvent, TriggerPhase, TriggerView},
};
use glam::Vec3;
use std::collections::HashSet;

pub(super) struct RouteStep<'a> {
	pub project: &'a ProjectData,
	pub scene_id: &'a SceneId,
	pub tick: u64,
	pub player: Option<&'a EntityKey>,
	pub triggers: &'a [TriggerEvent],
	pub overlaps: TriggerView<'a>,
	pub presses: Option<&'a HashSet<String>>,
	pub sequence_events: &'a [SequenceEvent],
	pub module: &'a dyn GameModule,
	pub progress: ProgressView<'a>,
}

impl RouteStep<'_> {
	fn activates(&self, activation: &SceneRouteActivation) -> bool {
		match activation {
			SceneRouteActivation::EnterSensor { sensor, actor } => {
				let actor = actor.as_ref().or(self.player);
				actor.is_some_and(|actor| {
					self.triggers.iter().any(|event| {
						event.phase == TriggerPhase::Enter
							&& &event.sensor == sensor
							&& &event.other == actor
					})
				})
			}
			SceneRouteActivation::InteractInSensor {
				sensor,
				actor,
				action,
			} => {
				let actor = actor.as_ref().or(self.player);
				actor.is_some_and(|actor| {
					self.player.is_none_or(|player| player == actor)
						&& self.presses.is_some_and(|presses| presses.contains(action))
						&& self.overlaps.contains(sensor, actor)
				})
			}
			SceneRouteActivation::SequenceCompleted { instance } => {
				self.sequence_events.iter().any(|event| match event {
					SequenceEvent::Finished { instance: id, tick }
					| SequenceEvent::Skipped { instance: id, tick } => id == instance && *tick == self.tick,
					_ => false,
				})
			}
			SceneRouteActivation::Manual => false,
		}
	}
}

impl SimulationSession {
	pub(crate) fn route_overlaps(&self, activation: &SceneRouteActivation) -> bool {
		let Some((sensor, actor)) = activation.sensor_actor() else {
			return false;
		};
		actor
			.or_else(|| self.local_player().map(|owner| &owner.key))
			.is_some_and(|actor| self.triggers.view().contains(sensor, actor))
	}

	pub(crate) fn route_distance(&self, activation: &SceneRouteActivation) -> Option<f32> {
		let (sensor, actor) = activation.sensor_actor()?;
		let actor = actor.or_else(|| self.local_player().map(|owner| &owner.key))?;
		let actor_index = self.world.entity_index(actor)?;
		if !self.world.data().entities[actor_index].active {
			return None;
		}
		let position = self.world.world_matrices()[actor_index].transform_point3(Vec3::ZERO);
		let bounds = self.collisions.body_bounds(sensor).ok()?;
		let closest = position.clamp(
			Vec3::from_array(bounds.min()),
			Vec3::from_array(bounds.max()),
		);
		let distance = position.distance(closest);
		distance.is_finite().then_some(distance)
	}

	pub(crate) fn validate_route_condition(
		&self,
		route: &SceneRoute,
		progress: ProgressView<'_>,
	) -> Result<(), String> {
		if let Some(SceneRouteCondition::GameplayFlag { key }) = &route.condition
			&& self.module.route_flag(key, progress).is_none()
		{
			return Err(format!(
				"Scene route {:?} uses an unregistered gameplay flag {key:?}",
				route.id.as_str()
			));
		}
		Ok(())
	}

	pub(crate) fn route_condition_satisfied(
		&self,
		route: &SceneRoute,
		progress: ProgressView<'_>,
	) -> bool {
		condition_satisfied(route, self.module.as_ref(), progress)
	}
}

pub(super) fn select_route<'a>(step: RouteStep<'a>) -> Option<&'a SceneRoute> {
	let mut selected: Option<&SceneRoute> = None;
	let mut ambiguous = None;
	for route in &step.project.scene_routes {
		if &route.from != step.scene_id
			|| !step.activates(&route.activate)
			|| !condition_satisfied(route, step.module, step.progress)
		{
			continue;
		}
		match selected {
			Some(current) if current.priority > route.priority => {}
			Some(current) if current.priority == route.priority => ambiguous = Some(route),
			_ => {
				selected = Some(route);
				ambiguous = None;
			}
		}
	}
	if let Some((a, b)) = selected.zip(ambiguous) {
		engine_logger::warn!(
			target: "engine_core::play",
			"Conflicting scene routes in scene {:?}, tick {}: {:?}, {:?}",
			step.scene_id.as_str(),
			step.tick,
			a.id.as_str(),
			b.id.as_str()
		);
		None
	} else {
		selected
	}
}

fn condition_satisfied(
	route: &SceneRoute,
	module: &dyn GameModule,
	progress: ProgressView<'_>,
) -> bool {
	match &route.condition {
		Some(SceneRouteCondition::GameplayFlag { key }) => {
			module.route_flag(key, progress) == Some(true)
		}
		None => true,
	}
}
