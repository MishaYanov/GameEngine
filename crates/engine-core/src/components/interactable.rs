//! Authored settings and a read-only reach test for an explicitly selected interaction.

use super::ComponentId;
use crate::{
	EntityKey, collision::CollisionWorld, input::valid_game_action_id, world::RuntimeWorld,
};
use serde::{Deserialize, Serialize};

pub const MAX_INTERACTION_REACH: f32 = 3.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InteractableSettings {
	pub action: String,
	pub reach: f32,
}

impl InteractableSettings {
	pub fn validate(&self) -> Result<(), String> {
		if !valid_game_action_id(&self.action) {
			return Err("Interactable requires a valid game input action ID".into());
		}
		if !valid_reach(self.reach) {
			return Err("Interactable reach must be positive and at most 3 metres".into());
		}
		Ok(())
	}
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InteractableComponent {
	pub id: ComponentId,
	pub enabled: bool,
	pub settings: InteractableSettings,
}

/// Tests an explicit interaction target at world-space reach through compatible solid bounds.
pub fn interaction_reachable(
	world: &RuntimeWorld,
	collisions: &CollisionWorld,
	actor: &EntityKey,
	target: &EntityKey,
	reach: f32,
) -> bool {
	if !valid_reach(reach) || actor == target {
		return false;
	}
	let (Some(actor_index), Some(target_index)) =
		(world.entity_index(actor), world.entity_index(target))
	else {
		return false;
	};
	let actor_position = world.world_matrices()[actor_index].w_axis.truncate();
	let target_position = world.world_matrices()[target_index].w_axis.truncate();
	actor_position.is_finite()
		&& target_position.is_finite()
		&& actor_position.distance_squared(target_position) <= reach * reach
		&& collisions.interaction_segment_clear(world, actor, target)
}

fn valid_reach(reach: f32) -> bool {
	reach.is_finite() && reach > 0.0 && reach <= MAX_INTERACTION_REACH
}
