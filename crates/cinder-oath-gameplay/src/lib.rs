//! Native gameplay for the authored Cinder Oath project.

mod duel;
mod scripts {
	pub(super) mod open_door_on_trigger;
}

use std::{collections::BTreeMap, sync::Arc};

use engine_core::{
	SceneData,
	command::{GameplayModuleDescriptorDto, GameplayRoleDescriptorDto},
	game_progress::ProgressState,
	project::{BehaviorSettings, ProjectData},
	scene_catalog::SceneId,
	scene_data::EntityData,
	simulation_session::{
		ConfiguredBehaviorFactory, GameModule, GameModuleFactory, PreparedModuleConfig,
	},
};
use serde::Deserialize;

pub use duel::{CinderProgress, Phase};
use duel::{Duel, Intro, Timing};

pub const CINDER_OATH_ID: &str = "cinder_oath";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
	schema_version: u32,
	intro_scene: SceneId,
	arena_scene: SceneId,
	intro_camera_travel: [f32; 3],
}

impl Settings {
	fn from_project(project: &ProjectData) -> Result<Self, String> {
		let value = project
			.gameplay_settings
			.as_ref()
			.ok_or("Cinder Oath requires project gameplay settings")?;
		let settings: Self = serde_json::from_value(value.clone())
			.map_err(|cause| format!("Invalid Cinder Oath settings: {cause}"))?;
		if settings.schema_version != 1
			|| settings.intro_scene == settings.arena_scene
			|| settings
				.intro_camera_travel
				.iter()
				.any(|delta| !delta.is_finite() || delta.abs() > 20.0)
		{
			return Err(
				"Cinder Oath requires distinct intro and arena scenes and a bounded camera move"
					.into(),
			);
		}
		project.scene_path(&settings.intro_scene)?;
		project.scene_path(&settings.arena_scene)?;
		Ok(settings)
	}
}

struct Factory {
	settings: Settings,
	timing: Timing,
}

impl GameModuleFactory for Factory {
	fn create_progress(&self) -> Result<ProgressState, String> {
		ProgressState::new(CinderProgress::default())
	}

	fn create(&self, _: &ProjectData, _: &SceneId) -> Result<Box<dyn GameModule>, String> {
		Err("Cinder Oath requires a prepared scene".into())
	}

	fn create_prepared(
		&self,
		config: PreparedModuleConfig<'_>,
	) -> Result<Box<dyn GameModule>, String> {
		let document = config.document();
		let camera = document.authored_game_camera()?;
		if config.scene_id() == &self.settings.intro_scene {
			let track = role(document, "camera_track")?;
			let knight = role(document, "knight")?;
			if track.key == knight.key
				|| !is_visual_root(track)
				|| camera
					.follow
					.as_ref()
					.is_none_or(|rig| rig.target != track.key)
			{
				return Err(
					"Follow the separate Camera Track entity in the Cinder Oath intro".into(),
				);
			}
			return Ok(Box::new(Intro::new(
				track,
				self.settings.intro_camera_travel,
				self.settings.arena_scene.clone(),
				self.timing,
			)));
		}
		if config.scene_id() == &self.settings.arena_scene {
			let player = role(document, "player")?;
			let enemy = role(document, "enemy")?;
			let sword = role(document, "sword")?;
			if player.key == enemy.key || player.key == sword.key || enemy.key == sword.key {
				return Err("Cinder Oath arena roles need three distinct entities".into());
			}
			if !is_fighter_root(player) || !is_fighter_root(enemy) {
				return Err("Player and Warden must be root entities with solid colliders".into());
			}
			if !is_visual_root(sword)
				|| document
					.entities
					.iter()
					.any(|entity| entity.parent.as_ref() == Some(&sword.key))
			{
				return Err(
					"The first-person Sword must be a root visual with no children or collider"
						.into(),
				);
			}
			if camera
				.orbit
				.as_ref()
				.is_none_or(|rig| rig.target != player.key)
			{
				return Err(
					"The Cinder Oath arena needs a game camera targeting the Player".into(),
				);
			}
			let speed = config
				.behaviors()
				.iter()
				.find_map(|behavior| match &behavior.settings {
					BehaviorSettings::ControlledMove { speed, .. }
						if behavior.entity == player.key && *speed > 0.0 =>
					{
						Some(*speed)
					}
					_ => None,
				})
				.ok_or("Assign a positive Controlled Move speed to the arena Player")?;
			return Ok(Box::new(Duel::new(
				player.key.clone(),
				enemy.key.clone(),
				sword.key.clone(),
				speed,
				self.timing,
			)));
		}
		ConfiguredBehaviorFactory.create_prepared(config)
	}
}

/// Resolve Cinder Oath's registered native rules for a checked authored project.
pub fn resolve(project: &ProjectData) -> Result<Arc<dyn GameModuleFactory>, String> {
	Ok(Arc::new(Factory {
		settings: Settings::from_project(project)?,
		timing: Timing::new(project.fixed_hz),
	}))
}

/// Return the authored intro and its arena for a Cinder Oath project.
pub fn intro_route(project: &ProjectData) -> Result<Option<(SceneId, SceneId)>, String> {
	if project.gameplay_module.as_deref() != Some(CINDER_OATH_ID) {
		return Ok(None);
	}
	let settings = Settings::from_project(project)?;
	Ok(Some((settings.intro_scene, settings.arena_scene)))
}

pub const fn object_script_registration() -> engine_core::simulation_session::ObjectScriptRegistration {
	engine_core::simulation_session::ObjectScriptRegistration {
		descriptor: scripts::open_door_on_trigger::descriptor,
		instantiate: scripts::open_door_on_trigger::instantiate,
	}
}

pub fn descriptor() -> GameplayModuleDescriptorDto {
	let roles = |items: &[(&str, &str, &str)]| {
		items
			.iter()
			.map(|&(name, label, hint)| GameplayRoleDescriptorDto {
				name: name.into(),
				label: label.into(),
				requirement_hint: hint.into(),
			})
			.collect()
	};
	GameplayModuleDescriptorDto {
		id: CINDER_OATH_ID.into(),
		label: "Cinder Oath".into(),
		description:
			"A cinematic knight reveal followed by a first-person duel with the Cinder Warden."
				.into(),
		default_roles: Vec::new(),
		scene_roles: BTreeMap::from([
			(
				"intro".into(),
				roles(&[
					(
						"camera_track",
						"Camera Track",
						"Root camera follow target without a collider.",
					),
					(
						"knight",
						"Knight",
						"Animated knight shown during the intro.",
					),
				]),
			),
			(
				"arena".into(),
				roles(&[
					(
						"player",
						"Player",
						"Solid root body with Controlled Move and the game camera.",
					),
					("enemy", "Cinder Warden", "Solid animated root actor."),
					(
						"sword",
						"First-Person Sword",
						"A root visual with no children or collider.",
					),
				]),
			),
		]),
	}
}

fn role<'a>(document: &'a SceneData, role: &str) -> Result<&'a EntityData, String> {
	let key = document
		.bindings
		.get(role)
		.ok_or_else(|| format!("Assign the Cinder Oath {role} role"))?;
	document
		.entities
		.iter()
		.find(|entity| entity.key == *key)
		.ok_or_else(|| format!("Cinder Oath {role} refers to an entity outside the scene"))
}

fn is_visual_root(entity: &EntityData) -> bool {
	entity.parent.is_none() && entity.collider.is_none()
}

fn is_fighter_root(entity: &EntityData) -> bool {
	entity.parent.is_none()
		&& entity
			.character_controller
			.as_ref()
			.is_none_or(|component| !component.enabled)
		&& entity.collision_enabled
		&& entity
			.collider
			.as_ref()
			.is_some_and(|collider| !collider.interaction().sensor)
}

#[cfg(test)]
mod tests;
