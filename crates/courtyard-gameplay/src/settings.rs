//! Courtyard-owned authoring schema. Core transports this as a bounded module payload.
use engine_core::{
	EntityKey, project::ProjectData, scene_catalog::SceneId,
	simulation_session::PreparedModuleConfig,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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
		if settings.schema_version != 1 {
			return Err("Unsupported Courtyard interaction settings version".into());
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
					if !matches!(
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
	let entity = |key: &EntityKey| {
		config
			.document()
			.entities
			.iter()
			.find(|entity| &entity.key == key)
			.ok_or_else(|| format!("{label} references missing entity {:?}", key.as_str()))
	};
	if sensor == player {
		return Err(format!("{label} must be different from Player"));
	}
	let sensor_entity = entity(sensor)?;
	let player_entity = entity(player)?;
	let sensor_interaction = sensor_entity
		.collider
		.as_ref()
		.ok_or_else(|| format!("{label} needs a sensor collider"))?
		.interaction();
	let player_interaction = player_entity
		.collider
		.as_ref()
		.ok_or("Player needs a collider to enter interaction triggers")?
		.interaction();
	if !sensor_entity.collision_enabled || !sensor_interaction.sensor {
		return Err(format!("{label} needs an enabled sensor collider"));
	}
	if !player_entity.collision_enabled {
		return Err("Player collision must start enabled".into());
	}
	if sensor_interaction.mask & player_interaction.layers == 0
		|| player_interaction.mask & sensor_interaction.layers == 0
	{
		return Err(format!(
			"{label} and Player collision layers/masks must accept each other"
		));
	}
	Ok(())
}
