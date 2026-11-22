//! Eligible edits to configured campaign triggers in retained authored scenes.

use engine_core::{
	EntityKey, collider::ColliderData, project::ProjectData, scene_catalog::SceneId,
	scene_data::SceneData,
};
use serde::{Deserialize, Serialize};

use crate::settings::{InteractionSettings, SceneInteractions, trigger_pair};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CampaignRepairRole {
	KeyTrigger,
	ExitTrigger,
	FinishTrigger,
}

impl CampaignRepairRole {
	const ALL: [Self; 3] = [Self::KeyTrigger, Self::ExitTrigger, Self::FinishTrigger];

	fn configured_entity(self, interactions: &SceneInteractions) -> Option<&EntityKey> {
		match (self, interactions) {
			(Self::KeyTrigger, SceneInteractions::Course { key_trigger, .. }) => Some(key_trigger),
			(Self::ExitTrigger, SceneInteractions::Course { exit_trigger, .. }) => {
				Some(exit_trigger)
			}
			(Self::FinishTrigger, SceneInteractions::Finish { finish_trigger }) => {
				Some(finish_trigger)
			}
			_ => None,
		}
	}
}

/// The authored scene, configured role, and exact entity bound to a repair.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CampaignRepairTarget {
	pub scene_id: SceneId,
	pub role: CampaignRepairRole,
	pub entity_key: EntityKey,
}

/// Collider values for an eligible edit, before applying it to the scene.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CampaignRepairPreview {
	pub target: CampaignRepairTarget,
	pub before: ColliderData,
	pub after: ColliderData,
}

/// Yield candidates for standalone triggers with valid, enabled collision pairs.
pub fn sensor_repairs(
	project: &ProjectData,
	scene: &SceneId,
	document: &SceneData,
) -> impl Iterator<Item = CampaignRepairPreview> {
	let settings = InteractionSettings::from_project(project).ok().flatten();
	CampaignRepairRole::ALL.into_iter().filter_map(move |role| {
		let interactions = settings.as_ref()?.scenes.get(scene.as_str())?;
		let key = role.configured_entity(interactions)?;
		let player = document.bindings.get("player")?;
		let pair = trigger_pair(document, key, player, "Configured trigger").ok()?;
		if pair.collider.interaction().sensor
			|| !pair.sensor.collision_enabled
			|| !pair.player.collision_enabled
			|| !pair.filters_detect()
			|| document.entities.iter().any(|entity| {
				entity.linked_prefab.as_ref().is_some_and(|link| {
					&entity.key == key || link.entity_map.values().any(|member| member == key)
				})
			}) {
			return None;
		}
		let before = pair.collider.clone();
		let mut after = before.clone();
		match &mut after {
			ColliderData::Box { interaction, .. } | ColliderData::Sphere { interaction, .. } => {
				interaction.sensor = true;
			}
		}
		Some(CampaignRepairPreview {
			target: CampaignRepairTarget {
				scene_id: scene.clone(),
				role,
				entity_key: key.clone(),
			},
			before,
			after,
		})
	})
}
