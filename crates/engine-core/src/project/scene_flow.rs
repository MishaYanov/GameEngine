//! Persistent links between complete scene documents.
use super::ProjectData;
use crate::{
	EntityKey, SceneData, input::valid_game_action_id, scene_catalog::SceneId,
	scene_data::EntityData,
};
use glam::Vec3;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, sync::Arc};

pub const MAX_SCENE_ROUTES: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SceneRouteId(String);

impl SceneRouteId {
	pub fn new(id: impl Into<String>) -> Result<Self, String> {
		Self::try_from(id.into())
	}

	pub fn as_str(&self) -> &str {
		&self.0
	}
}

impl TryFrom<String> for SceneRouteId {
	type Error = String;

	fn try_from(id: String) -> Result<Self, String> {
		if id.is_empty()
			|| id.len() > 64
			|| !id
			.bytes()
			.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
		{
			return Err(
				"Scene route ID must contain 1–64 ASCII letters, digits, underscores, or hyphens"
					.into(),
			);
		}
		Ok(Self(id))
	}
}

impl From<SceneRouteId> for String {
	fn from(id: SceneRouteId) -> Self {
		id.0
	}
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneRoute {
	pub id: SceneRouteId,
	pub from: SceneId,
	pub to: SceneId,
	pub activate: SceneRouteActivation,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub condition: Option<SceneRouteCondition>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub preload: Option<SceneRoutePreload>,
	/// Destination entity designating the world-space arrival position.
	pub arrival: EntityKey,
	/// Defaults to the destination's local player when present.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub arrival_actor: Option<EntityKey>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub start_sequence: Option<EntityKey>,
	#[serde(default, skip_serializing_if = "is_default_priority")]
	pub priority: i16,
}

impl SceneRoute {
	pub(crate) fn prepare_arrival(
		&self,
		mut scene: Arc<SceneData>,
	) -> Result<Arc<SceneData>, String> {
		let marker = self.find_entity(&scene, &self.arrival, "arrival marker")?;
		let Some(actor) = self.arrival_actor_in(&scene)? else {
			// Camera-only destinations use their authored presentation.
			return Ok(scene);
		};
		let matrices = scene.world_matrices()?;
		let actor_index = scene
			.entities
			.iter()
			.position(|entity| entity.key == actor.key)
			.ok_or_else(|| self.error("arrival actor is missing"))?;
		let marker_index = scene
			.entities
			.iter()
			.position(|entity| entity.key == marker.key)
			.ok_or_else(|| self.error("arrival marker is missing"))?;
		let position = matrices[marker_index]
			.transform_point3(Vec3::ZERO)
			.to_array();
		if !position.iter().all(|coordinate| coordinate.is_finite()) {
			return Err(self.error("arrival position is not finite"));
		}
		Arc::make_mut(&mut scene).entities[actor_index]
			.transform
			.position = position;
		Ok(scene)
	}

	fn validate(&self, project: &ProjectData) -> Result<(), String> {
		if project.scenes.get(&self.from).is_err() {
			return Err(self.error(format!("unknown source scene {:?}", self.from.as_str())));
		}
		if project.scenes.get(&self.to).is_err() {
			return Err(self.error(format!("unknown destination scene {:?}", self.to.as_str())));
		}
		if self.from == self.to {
			return Err(self.error("source and destination must be different"));
		}
		validate_key(&self.arrival).map_err(|error| self.error(error))?;
		if let Some(actor) = &self.arrival_actor {
			validate_key(actor).map_err(|error| self.error(error))?;
		}
		if let Some(condition) = &self.condition {
			let SceneRouteCondition::GameplayFlag { key } = condition;
			SceneRouteId::new(key).map_err(|_| self.error("invalid gameplay flag ID"))?;
		}
		if let SceneRouteActivation::InteractInSensor { action, .. } = &self.activate
			&& !valid_game_action_id(action)
		{
			return Err(self.error("invalid interaction action ID"));
		}
		if let Some((sensor, actor)) = self.activate.sensor_actor() {
			validate_key(sensor).map_err(|error| self.error(error))?;
			if let Some(actor) = actor {
				validate_key(actor).map_err(|error| self.error(error))?;
			}
		}
		if let SceneRouteActivation::SequenceCompleted { instance } = &self.activate {
			validate_key(instance).map_err(|error| self.error(error))?;
			self.validate_sequence(project, &self.from, instance)?;
		}
		if let Some(instance) = &self.start_sequence {
			validate_key(instance).map_err(|error| self.error(error))?;
			self.validate_sequence(project, &self.to, instance)?;
		}
		match &self.preload {
			Some(SceneRoutePreload::Approach {
				     distance,
				     release_distance,
			     }) => {
				if self.activate.sensor_actor().is_none()
					|| !distance.is_finite()
					|| !release_distance.is_finite()
					|| *distance <= 0.0
					|| *release_distance <= *distance
				{
					return Err(self.error(
						"approach preload requires a sensor and finite, increasing positive distances",
					));
				}
			}
			Some(SceneRoutePreload::WhenSequenceStarts { instance }) => {
				validate_key(instance).map_err(|error| self.error(error))?;
				self.validate_sequence(project, &self.from, instance)?;
			}
			None => {}
		}
		Ok(())
	}

	fn validate_sequence(
		&self,
		project: &ProjectData,
		scene: &SceneId,
		instance: &EntityKey,
	) -> Result<(), String> {
		if project.sequences.instance_for(scene, instance).is_none() {
			return Err(self.error(format!(
				"sequence instance {:?} is missing from scene {:?}",
				instance.as_str(),
				scene.as_str()
			)));
		}
		Ok(())
	}

	fn arrival_actor_in<'a>(&self, scene: &'a SceneData) -> Result<Option<&'a EntityData>, String> {
		let actor = if let Some(key) = &self.arrival_actor {
			Some(self.find_entity(scene, key, "arrival actor")?)
		} else {
			scene.local_player()
		};
		if let Some(actor) = actor
			&& (!actor.active || actor.parent.is_some() || actor.key == self.arrival)
		{
			return Err(self.error("arrival actor must be an active root distinct from the marker"));
		}
		Ok(actor)
	}

	fn find_entity<'a>(
		&self,
		scene: &'a SceneData,
		key: &EntityKey,
		role: &str,
	) -> Result<&'a EntityData, String> {
		scene
			.entities
			.iter()
			.find(|entity| &entity.key == key)
			.ok_or_else(|| self.error(format!("{role} {:?} is missing", key.as_str())))
	}

	fn error(&self, message: impl std::fmt::Display) -> String {
		format!("Scene route {:?}: {message}", self.id.as_str())
	}
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SceneRouteActivation {
	EnterSensor {
		sensor: EntityKey,
		#[serde(default, skip_serializing_if = "Option::is_none")]
		actor: Option<EntityKey>,
	},
	InteractInSensor {
		sensor: EntityKey,
		#[serde(default, skip_serializing_if = "Option::is_none")]
		actor: Option<EntityKey>,
		#[serde(default = "default_interact_action")]
		action: String,
	},
	SequenceCompleted {
		instance: EntityKey,
	},
	Manual,
}

impl SceneRouteActivation {
	pub fn sensor_actor(&self) -> Option<(&EntityKey, Option<&EntityKey>)> {
		match self {
			Self::EnterSensor { sensor, actor } | Self::InteractInSensor { sensor, actor, .. } => {
				Some((sensor, actor.as_ref()))
			}
			Self::SequenceCompleted { .. } | Self::Manual => None,
		}
	}
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SceneRouteCondition {
	GameplayFlag { key: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SceneRoutePreload {
	Approach {
		distance: f32,
		release_distance: f32,
	},
	WhenSequenceStarts {
		instance: EntityKey,
	},
}

impl ProjectData {
	pub fn scene_route(&self, id: &SceneRouteId) -> Option<&SceneRoute> {
		self.scene_routes.iter().find(|route| &route.id == id)
	}

	pub(super) fn validate_scene_routes(&self) -> Result<(), String> {
		if self.scene_routes.len() > MAX_SCENE_ROUTES {
			return Err(format!("Project exceeds {MAX_SCENE_ROUTES} scene routes"));
		}
		let mut ids = HashSet::with_capacity(self.scene_routes.len());
		let mut triggers = HashSet::with_capacity(self.scene_routes.len());
		for route in &self.scene_routes {
			route.validate(self)?;
			if !ids.insert(&route.id) {
				return Err(route.error("duplicate route ID"));
			}
			if route.activate != SceneRouteActivation::Manual
				&& !triggers.insert((&route.from, &route.activate, route.priority))
			{
				return Err(route.error("ambiguous source event at this route priority"));
			}
		}
		for route in &self.scene_routes {
			let mut seen = HashSet::new();
			let mut stack = vec![route];
			while let Some(route) = stack.pop() {
				if let Some(instance) = &route.start_sequence {
					let next = self.scene_routes.iter().filter(|candidate| {
						candidate.from == route.to
							&& matches!(&candidate.activate, SceneRouteActivation::SequenceCompleted { instance: candidate } if candidate == instance)
					});
					for next in next {
						if seen.insert(&next.id) {
							stack.push(next);
						}
					}
				}
			}
			if seen.contains(&route.id) {
				return Err(route.error("automatic sequence routes form a cycle"));
			}
		}
		Ok(())
	}

	/// Validate every authored trigger and arrival associated with this document.
	pub fn validate_scene_routes_for(&self, id: &SceneId, scene: &SceneData) -> Result<(), String> {
		for route in &self.scene_routes {
			if &route.to == id {
				route.find_entity(scene, &route.arrival, "arrival marker")?;
				route.arrival_actor_in(scene)?;
			}
			if &route.from == id
				&& let Some((sensor, actor)) = route.activate.sensor_actor()
			{
				let sensor = route.find_entity(scene, sensor, "sensor")?;
				if !sensor
					.collider
					.as_ref()
					.is_some_and(|collider| collider.interaction().sensor)
				{
					return Err(route.error("trigger entity requires a sensor collider"));
				}
				let actor = match actor {
					Some(key) => route.find_entity(scene, key, "trigger actor")?,
					None => scene.local_player().ok_or_else(|| {
						route.error(
							"trigger requires a local Player Controller or an explicit actor",
						)
					})?,
				};
				if actor.collider.is_none() || actor.key == sensor.key {
					return Err(route.error("trigger actor requires a distinct collider"));
				}
			}
		}
		Ok(())
	}
}

fn default_interact_action() -> String {
	"interact".into()
}

fn is_default_priority(priority: &i16) -> bool {
	*priority == 0
}

fn validate_key(key: &EntityKey) -> Result<(), String> {
	key.validate()?;
	if key.as_str().len() > 256 || key.as_str().chars().any(char::is_control) {
		return Err(
			"Scene route entity keys are limited to 256 bytes without control characters".into(),
		);
	}
	Ok(())
}
