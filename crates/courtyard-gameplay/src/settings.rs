//! Courtyard-owned authoring schema. Core transports this as a bounded module payload.
use engine_core::{
	EntityKey,
	collider::ColliderData,
	project::ProjectData,
	scene_catalog::SceneId,
	scene_data::{EntityData, SceneData},
	simulation_session::PreparedModuleConfig,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub use crate::chapters::{CampaignDefinition, ChapterDefinition};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InteractionSettings {
	pub schema_version: u32,
	pub scenes: BTreeMap<String, SceneInteractions>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub route_gate: Option<RouteGateSettings>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub project_save_id: Option<String>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub checkpoints: Vec<crate::checkpoint::CheckpointDef>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub campaign: Option<CampaignDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteGateSettings {
	pub scene_id: SceneId,
	pub lever: EntityKey,
	pub gate: EntityKey,
	pub action: String,
	pub radius: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SceneInteractions {
	Course {
		key_trigger: EntityKey,
		exit_trigger: EntityKey,
		destination_scene: SceneId,
		#[serde(default, skip_serializing_if = "Option::is_none")]
		exit_blocker: Option<EntityKey>,
	},
	Finish {
		finish_trigger: EntityKey,
	},
}

impl InteractionSettings {
	pub fn from_project(project: &ProjectData) -> Result<Option<Self>, String> {
		let Some(value) = project
			.gameplay_settings
			.as_ref()
			.filter(|value| !value.is_null())
		else {
			return Ok(None);
		};
		project.validate()?;
		let settings: Self = serde_json::from_value(value.clone())
			.map_err(|error| format!("Courtyard interaction settings: {error}"))?;
		match (settings.schema_version, &settings.campaign) {
			(1, None) | (2, Some(_)) => {}
			(1 | 2, _) => {
				return Err("Courtyard campaigns require schema 2 and an ordered route".into());
			}
			_ => return Err("Unsupported Courtyard interaction settings version".into()),
		}
		for (id, scene) in &settings.scenes {
			let id = SceneId::new(id)?;
			project.scenes.get(&id).map_err(|_| {
				format!(
					"Interaction settings reference missing scene {:?}",
					id.as_str()
				)
			})?;
			match scene {
				SceneInteractions::Course {
					key_trigger,
					exit_trigger,
					destination_scene,
					exit_blocker,
				} => {
					key_trigger.validate()?;
					exit_trigger.validate()?;
					if key_trigger == exit_trigger {
						return Err("Key and Exit need different trigger entities".into());
					}
					if let Some(blocker) = exit_blocker {
						blocker.validate()?;
					}
					if destination_scene == &id {
						return Err("Exit destination must be a different scene".into());
					}
					project.scenes.get(destination_scene).map_err(|_| {
						format!(
							"Exit destination {:?} is missing from the project",
							destination_scene.as_str()
						)
					})?;
					if settings.campaign.is_none()
						&& !matches!(
							settings.scenes.get(destination_scene.as_str()),
							Some(SceneInteractions::Finish { .. })
						) && !(destination_scene.as_str() == super::FINISH_SCENE_ID
						&& !settings.scenes.contains_key(destination_scene.as_str()))
					{
						return Err("Exit destination needs Finish interaction settings".into());
					}
				}
				SceneInteractions::Finish { finish_trigger } => finish_trigger.validate()?,
			}
		}
		if let Some(gate) = &settings.route_gate {
			gate.validate(project, &settings.scenes)?;
		}
		if let Some(campaign) = &settings.campaign {
			campaign.validate(project, &settings)?;
		}
		Ok(Some(settings))
	}
}

impl RouteGateSettings {
	fn validate(
		&self,
		project: &ProjectData,
		scenes: &BTreeMap<String, SceneInteractions>,
	) -> Result<(), String> {
		self.lever.validate()?;
		self.gate.validate()?;
		if self.lever == self.gate {
			return Err("Route lever and gate must be different entities".into());
		}
		if !self.radius.is_finite() || self.radius <= 0.0 || self.radius > 3.0 {
			return Err("Route lever radius must be positive and at most 3 metres".into());
		}
		if !matches!(
			scenes.get(self.scene_id.as_str()),
			Some(SceneInteractions::Course { .. })
		) {
			return Err("Route gate must reference a scene with Course settings".into());
		}
		let profile = project
			.input_profile
			.as_ref()
			.ok_or("Route gate needs its Interact action bound in the authored input profile")?;
		let action = profile
			.actions
			.iter()
			.find(|action| action.id == self.action)
			.ok_or("Route gate references an unbound game input action")?;
		if profile.movement.contains_key(action.key)
			|| profile
				.actions
				.iter()
				.any(|other| other.id != action.id && other.key == action.key)
		{
			return Err("Route gate needs a dedicated Interact key".into());
		}
		Ok(())
	}
}

pub(crate) fn validate_trigger(
	config: &PreparedModuleConfig<'_>,
	sensor: &EntityKey,
	player: &EntityKey,
	label: &str,
) -> Result<(), String> {
	let pair = trigger_pair(config.document(), sensor, player, label)?;
	if !pair.sensor.collision_enabled || !pair.collider.interaction().sensor {
		return Err(format!("{label} needs an enabled sensor collider"));
	}
	if !pair.player.collision_enabled {
		return Err("Player collision must start enabled".into());
	}
	if !pair.filters_detect() {
		return Err(format!(
			"{label} and Player collision layers/masks must accept each other"
		));
	}
	Ok(())
}

pub(crate) struct TriggerPair<'a> {
	pub sensor: &'a EntityData,
	pub collider: &'a ColliderData,
	pub player: &'a EntityData,
	player_collider: &'a ColliderData,
}

impl TriggerPair<'_> {
	pub(crate) fn filters_detect(&self) -> bool {
		self.collider
			.interaction()
			.detects(self.player_collider.interaction())
	}
}

pub(crate) fn trigger_pair<'a>(
	document: &'a SceneData,
	sensor: &EntityKey,
	player: &EntityKey,
	label: &str,
) -> Result<TriggerPair<'a>, String> {
	let entity = |key: &EntityKey| {
		document
			.entities
			.iter()
			.find(|entity| &entity.key == key)
			.ok_or_else(|| format!("{label} references missing entity {:?}", key.as_str()))
	};
	if sensor == player {
		return Err(format!("{label} must be different from Player"));
	}
	let sensor = entity(sensor)?;
	let player = entity(player)?;
	let collider = sensor
		.collider
		.as_ref()
		.ok_or_else(|| format!("{label} needs a sensor collider"))?;
	let player_collider = player
		.collider
		.as_ref()
		.ok_or("Player needs a collider to enter interaction triggers")?;
	Ok(TriggerPair {
		sensor,
		collider,
		player,
		player_collider,
	})
}
