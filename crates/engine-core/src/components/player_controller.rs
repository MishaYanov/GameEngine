use super::ComponentId;
use crate::{
	character_controller::validate_character_body,
	scene_data::{EntityData, SceneData},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum InputActionsRef {
	#[default]
	ProjectDefault,
	Asset {
		asset_id: crate::asset_catalog::AssetId,
	},
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayerMovementSpace {
	World,
	#[default]
	CameraRelative,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerControllerSettings {
	pub input: InputActionsRef,
	pub space: PlayerMovementSpace,
	#[serde(default)]
	pub local: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerControllerComponent {
	pub id: ComponentId,
	pub enabled: bool,
	pub settings: PlayerControllerSettings,
}

impl EntityData {
	pub(crate) fn validate_player_controller(&self) -> Result<(), String> {
		let Some(controller) = &self.player_controller else {
			return Ok(());
		};
		if !controller.enabled {
			return if controller.settings.local {
				Err("A disabled Player Controller cannot own local input".into())
			} else {
				Ok(())
			};
		}
		if self.parent.is_some() || self.linked_prefab.is_some() {
			return Err("Player Controller must be on an independent scene-root object; place visual Props below the actor".into());
		}
		let motor = self
			.character_controller
			.as_ref()
			.filter(|motor| motor.enabled)
			.ok_or(
				"Player Controller requires an enabled Character Controller on the same object",
			)?;
		validate_character_body(self, &motor.settings)
	}

	pub(crate) fn clear_local_player(&mut self) {
		if let Some(controller) = &mut self.player_controller {
			controller.settings.local = false;
		}
	}

	pub fn is_local_player(&self) -> bool {
		self.player_controller
			.as_ref()
			.is_some_and(|controller| controller.enabled && controller.settings.local)
	}
}

impl SceneData {
	/// Validated scenes contain at most one local Player Controller.
	pub fn local_player(&self) -> Option<&EntityData> {
		self.entities.iter().find(|entity| entity.is_local_player())
	}
}
