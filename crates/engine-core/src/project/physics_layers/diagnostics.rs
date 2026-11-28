use crate::{
	EntityKey,
	collider::ColliderInteraction,
	collision::InteractionMatrix,
	components::{ComponentId, NativeBehaviorSpec},
	scene_data::{EntityData, SceneData},
};
use std::collections::HashMap;

#[derive(Debug)]
pub struct PhysicsContactIssue<'a> {
	pub owner: &'a EntityData,
	pub component: Option<&'a ComponentId>,
	pub actor: &'a EntityKey,
	pub sensor: &'a EntityKey,
	reason: &'static str,
}

impl PhysicsContactIssue<'_> {
	pub fn message(&self) -> String {
		let scope = if self.component.is_some() {
			"Required trigger pair"
		} else {
			"Sensor and local player"
		};
		format!(
			"{scope} {:?} and {:?} cannot interact because {}.",
			self.sensor.as_str(),
			self.actor.as_str(),
			self.reason
		)
	}
}

/// Find required native behavior contacts suppressed by the project's policy, then
/// inspect enabled optional sensors against the local player's collider.
pub fn inspect_physics_contacts<'a>(
	scene: &'a SceneData,
	matrix: &InteractionMatrix,
	max: usize,
) -> Vec<PhysicsContactIssue<'a>> {
	if max == 0 {
		return Vec::new();
	}
	let actors = ColliderInteractions::for_scene(scene);
	let mut issues: Vec<_> = required_contacts(scene, matrix, &actors)
		.take(max)
		.collect();
	if let Some(player) = scene
		.local_player()
		.filter(|player| player.collision_enabled)
	{
		let required = issues.len();
		for sensor in &scene.entities {
			if issues.len() == max {
				break;
			}
			if sensor.key == player.key
				|| !sensor.collision_enabled
				|| !actors
					.memberships
					.get(&sensor.key)
					.is_some_and(|collider| collider.sensor)
				|| issues[..required]
					.iter()
					.any(|issue| issue.sensor == &sensor.key && issue.actor == &player.key)
			{
				continue;
			}
			if let Some(reason) = actors.exclusion(matrix, &sensor.key, &player.key) {
				issues.push(PhysicsContactIssue {
					owner: sensor,
					component: None,
					actor: &player.key,
					sensor: &sensor.key,
					reason,
				});
			}
		}
	}
	issues
}

pub(crate) fn validate_required_physics_contacts(
	scene: &SceneData,
	matrix: &InteractionMatrix,
) -> Result<(), String> {
	if *matrix == InteractionMatrix::ALLOW_ALL {
		return Ok(());
	}
	let actors = ColliderInteractions::for_scene(scene);
	required_contacts(scene, matrix, &actors)
		.next()
		.map_or(Ok(()), |issue| Err(issue.message()))
}

struct ColliderInteractions<'a> {
	memberships: HashMap<&'a EntityKey, ColliderInteraction>,
}

impl<'a> ColliderInteractions<'a> {
	fn for_scene(scene: &'a SceneData) -> Self {
		Self {
			memberships: scene
				.entities
				.iter()
				.filter_map(|entity| Some((&entity.key, entity.collider.as_ref()?.interaction())))
				.collect(),
		}
	}

	fn exclusion(
		&self,
		matrix: &InteractionMatrix,
		left: &EntityKey,
		right: &EntityKey,
	) -> Option<&'static str> {
		let (&left, &right) = (self.memberships.get(left)?, self.memberships.get(right)?);
		if !left.detects(right) {
			return Some("their collider masks reject each other");
		}
		(!matrix.allows_pair(left, right))
			.then_some("the project matrix ignores their accepted layers")
	}
}

fn required_contacts<'a>(
	scene: &'a SceneData,
	matrix: &InteractionMatrix,
	actors: &ColliderInteractions<'a>,
) -> impl Iterator<Item = PhysicsContactIssue<'a>> {
	scene.entities.iter().flat_map(move |entity| {
		entity
			.native_behaviors
			.iter()
			.filter(|component| component.enabled)
			.flat_map(move |component| {
				let (direct, inputs): (_, &[_]) = match &component.behavior {
					NativeBehaviorSpec::KeyDoor {
						actor, key_trigger, ..
					} => (Some((actor, key_trigger)), &[]),
					NativeBehaviorSpec::Collectible { actor, trigger, .. } => {
						(Some((actor, trigger)), &[])
					}
					NativeBehaviorSpec::StateMachine { definition } => (None, &definition.inputs),
					NativeBehaviorSpec::TimedSpawner { .. }
					| NativeBehaviorSpec::Projectile { .. }
					| NativeBehaviorSpec::ProjectileLauncher { .. } => (None, &[]),
				};
				direct
					.into_iter()
					.chain(inputs.iter().map(|input| (&input.actor, &input.sensor)))
					.filter_map(move |(actor, sensor)| {
						Some(PhysicsContactIssue {
							owner: entity,
							component: Some(&component.id),
							actor,
							sensor,
							reason: actors.exclusion(matrix, actor, sensor)?,
						})
					})
			})
	})
}
