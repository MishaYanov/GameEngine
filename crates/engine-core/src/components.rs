//! Typed component adapters over the existing single entity backing store.
mod entity;
mod interactable;
mod native_behavior;
mod player_controller;
mod scene_validation;

use crate::{
	EntityKey,
	asset_catalog::AssetId,
	scene_data::{ColliderData, EntityData, MaterialOverrideData, SceneData, validate_model_path},
};
pub use interactable::{
	InteractableComponent, InteractableSettings, MAX_INTERACTION_REACH, interaction_reachable,
};
pub use native_behavior::{NativeBehaviorComponent, NativeBehaviorSpec};
pub use player_controller::{
	InputActionsRef, PlayerControllerComponent, PlayerControllerSettings, PlayerMovementSpace,
};
pub use scene_validation::{validate_scene_components, validate_scene_components_with_slots};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

pub const MAX_COMPONENTS_PER_OBJECT: usize = 16;
pub const MAX_NATIVE_BEHAVIORS_PER_OBJECT: usize = 8;
pub const MAX_ACTIVE_NATIVE_BEHAVIORS: usize = 4096;
pub const MESH_TYPE: &str = "engine.mesh_renderer";
pub const COLLIDER_TYPE: &str = "engine.collider";
pub const NATIVE_TYPE: &str = "engine.native_behavior";
pub const CHARACTER_TYPE: &str = "engine.character_controller";
pub const ANIMATION_TYPE: &str = "engine.animation";
pub const INTERACTABLE_TYPE: &str = "engine.interactable";
pub const CAMERA_TYPE: &str = "engine.camera";
pub const PLAYER_CONTROLLER_TYPE: &str = "engine.player_controller";
pub const NO_MESH_PATH: &str = "__no_mesh__";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ComponentId(String);
impl ComponentId {
	pub fn new(value: impl Into<String>) -> Result<Self, String> {
		let value = value.into();
		if value.is_empty()
			|| value.len() > 128
			|| !value
				.bytes()
				.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
		{
			return Err(
				"Component ID requires 1–128 ASCII letters, digits, underscores or hyphens".into(),
			);
		}
		Ok(Self(value.into_boxed_str().into_string()))
	}
	pub fn generate() -> Self {
		Self(uuid::Uuid::new_v4().to_string())
	}
	pub fn as_str(&self) -> &str {
		&self.0
	}
}
impl<'de> Deserialize<'de> for ComponentId {
	fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
		Self::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
	}
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ComponentData {
	PlayerController {
		settings: PlayerControllerSettings,
	},
	Camera {
		settings: crate::game_camera::CameraSettings,
	},
	CharacterController {
		settings: crate::character_controller::CharacterControllerSettings,
	},
	Animation {
		settings: crate::animation::AnimationSettings,
	},
	Interactable {
		settings: InteractableSettings,
	},
	MeshRenderer {
		model_path: String,
		#[serde(default)]
		asset_id: Option<AssetId>,
		#[serde(default)]
		mesh_index: Option<usize>,
		#[serde(default)]
		material: Option<MaterialOverrideData>,
	},
	Collider {
		collider: ColliderData,
	},
	NativeBehavior {
		behavior: NativeBehaviorSpec,
	},
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComponentRecord {
	pub id: ComponentId,
	pub schema_version: u32,
	pub enabled: bool,
	pub data: ComponentData,
}
impl ComponentRecord {
	pub fn type_id(&self) -> &'static str {
		match self.data {
			ComponentData::PlayerController { .. } => PLAYER_CONTROLLER_TYPE,
			ComponentData::Camera { .. } => CAMERA_TYPE,
			ComponentData::CharacterController { .. } => CHARACTER_TYPE,
			ComponentData::Animation { .. } => ANIMATION_TYPE,
			ComponentData::Interactable { .. } => INTERACTABLE_TYPE,
			ComponentData::MeshRenderer { .. } => MESH_TYPE,
			ComponentData::Collider { .. } => COLLIDER_TYPE,
			ComponentData::NativeBehavior { .. } => NATIVE_TYPE,
		}
	}
	pub fn validate(&self) -> Result<(), String> {
		ComponentId::new(self.id.as_str())?;
		if self.schema_version != 1 {
			return Err("Unsupported component schema version".into());
		}
		match &self.data {
			ComponentData::PlayerController { settings } if settings.local && !self.enabled => {
				Err("A disabled Player Controller cannot own local input".into())
			}
			ComponentData::PlayerController { .. } => Ok(()),
			ComponentData::Camera { settings } => settings.validate(),
			ComponentData::CharacterController { settings } => settings.validate(),
			ComponentData::Animation { settings } => settings.validate(),
			ComponentData::Interactable { settings } => settings.validate(),
			ComponentData::MeshRenderer {
				model_path,
				mesh_index,
				material,
				..
			} => {
				validate_model_path(model_path)?;
				if model_path == NO_MESH_PATH || mesh_index.is_some_and(|index| index > 1_000_000) {
					return Err("Invalid mesh component asset or mesh index".into());
				}
				if material.as_ref().is_some_and(|material| {
					!material
						.base_color_factor
						.iter()
						.all(|value| value.is_finite() && (0.0..=1.0).contains(value))
				}) {
					return Err("Mesh material requires finite 0–1 color components".into());
				}
				Ok(())
			}
			ComponentData::Collider { collider } => collider.validate(),
			ComponentData::NativeBehavior { behavior } => behavior.validate(),
		}
	}
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComponentTypeDescriptor {
	pub type_id: String,
	pub kind: String,
	pub label: String,
	pub description: String,
	pub max_per_object: usize,
}
pub fn descriptors() -> Vec<ComponentTypeDescriptor> {
	[
		(
			PLAYER_CONTROLLER_TYPE,
			"player_controller",
			"Player Controller",
			"Routes local input to a scene-root Character Controller.",
			1,
		),
		(
			CAMERA_TYPE,
			"camera",
			"Camera",
			"Perspective lens and Fixed, Follow or Orbit game camera.",
			1,
		),
		(
			CHARACTER_TYPE,
			"character_controller",
			"Character Controller",
			"Grounded motion for an upright scene-root box body.",
			1,
		),
		(
			ANIMATION_TYPE,
			"animation",
			"Animation",
			"Deterministic native animation playback.",
			1,
		),
		(
			INTERACTABLE_TYPE,
			"interactable",
			"Interactable",
			"Game action and reach for a deliberate interaction.",
			1,
		),
		(
			MESH_TYPE,
			"mesh_renderer",
			"Mesh Renderer",
			"Shared model, material and visible state.",
			1,
		),
		(
			COLLIDER_TYPE,
			"collider",
			"Collider",
			"Authored shape, sensor/filter data and enabled state.",
			1,
		),
		(
			NATIVE_TYPE,
			"native_behavior",
			"Native Behavior",
			"Independent native gameplay state for this attachment.",
			MAX_NATIVE_BEHAVIORS_PER_OBJECT,
		),
	]
	.into_iter()
	.map(
		|(type_id, kind, label, description, max_per_object)| ComponentTypeDescriptor {
			type_id: type_id.into(),
			kind: kind.into(),
			label: label.into(),
			description: description.into(),
			max_per_object,
		},
	)
	.collect()
}

/// Stable semantic addresses for flat linked-prefab overrides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PropertyId {
	CharacterController,
	Animation,
	Name,
	TransformPosition,
	TransformRotation,
	TransformScale,
	MeshModelPath,
	MeshAssetId,
	MeshIndex,
	MeshMaterial,
	Collider,
	ComponentEnabled,
	NativeBehavior,
}

/// Singleton metadata is stored only when it differs from deterministic legacy IDs.
pub fn singleton_ids(records: &[ComponentRecord]) -> BTreeMap<String, ComponentId> {
	records
		.iter()
		.filter(|record| matches!(record.type_id(), MESH_TYPE | COLLIDER_TYPE))
		.filter(|record| {
			record.id.as_str()
				!= if record.type_id() == MESH_TYPE {
					"mesh"
				} else {
					"collider"
				}
		})
		.map(|record| (record.type_id().into(), record.id.clone()))
		.collect()
}
