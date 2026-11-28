use super::{ComponentId, SceneFog};
use crate::{EntityKey, SceneData, asset_catalog::AssetId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkySettings {
	pub zenith: [f32; 3],
	pub horizon: [f32; 3],
	pub yaw_degrees: f32,
	#[serde(default, skip_serializing_if = "SkyBackground::is_gradient")]
	pub background: SkyBackground,
}

pub const MAX_SKY_PANORAMA_WIDTH: u32 = 2_048;
pub const MAX_SKY_PANORAMA_HEIGHT: u32 = 1_024;
pub const MAX_SKY_PANORAMA_FILE_BYTES: usize = 12 * 1024 * 1024;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SkyBackground {
	#[default]
	Gradient,
	Panorama {
		image: AssetId,
	},
}

impl SkyBackground {
	pub fn is_gradient(&self) -> bool {
		matches!(self, Self::Gradient)
	}
	pub fn image(&self) -> Option<&AssetId> {
		match self {
			Self::Gradient => None,
			Self::Panorama { image } => Some(image),
		}
	}
}

impl Default for SkySettings {
	fn default() -> Self {
		Self {
			zenith: [0.035, 0.075, 0.18],
			horizon: [0.30, 0.46, 0.65],
			yaw_degrees: 0.0,
			background: Default::default(),
		}
	}
}

impl SkySettings {
	pub fn validate(&self) -> Result<(), String> {
		if !self
			.zenith
			.iter()
			.chain(&self.horizon)
			.all(|value| (0.0..=1.0).contains(value))
		{
			return Err("Sky colors require finite linear RGB values from 0 to 1".into());
		}
		if !(-360.0..=360.0).contains(&self.yaw_degrees) {
			return Err("Sky yaw requires finite degrees from -360 to 360".into());
		}
		Ok(())
	}
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkyComponent {
	pub id: ComponentId,
	pub enabled: bool,
	pub settings: SkySettings,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneEnvironment {
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub active_sky: Option<EntityKey>,
	#[serde(
		default = "default_ambient",
		skip_serializing_if = "is_default_ambient"
	)]
	pub ambient_linear: [f32; 3],
	#[serde(default, skip_serializing_if = "is_zero")]
	pub sky_exposure_ev: f32,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub fog: Option<SceneFog>,
}

pub const DEFAULT_AMBIENT_LINEAR: [f32; 3] = [0.25; 3];
pub const MAX_SKY_EXPOSURE_EV: f32 = 8.0;

fn default_ambient() -> [f32; 3] {
	DEFAULT_AMBIENT_LINEAR
}
fn is_default_ambient(value: &[f32; 3]) -> bool {
	*value == DEFAULT_AMBIENT_LINEAR
}
fn is_zero(value: &f32) -> bool {
	*value == 0.0
}

impl Default for SceneEnvironment {
	fn default() -> Self {
		Self {
			active_sky: None,
			ambient_linear: DEFAULT_AMBIENT_LINEAR,
			sky_exposure_ev: 0.0,
			fog: None,
		}
	}
}

impl SceneEnvironment {
	pub fn is_empty(&self) -> bool {
		self == &Self::default()
	}

	pub(crate) fn validate_lighting(ambient: [f32; 3], exposure: f32) -> Result<(), String> {
		if !ambient.iter().all(|channel| (0.0..=1.0).contains(channel)) {
			return Err("Ambient color requires finite linear RGB values from 0 to 1".into());
		}
		if !(-MAX_SKY_EXPOSURE_EV..=MAX_SKY_EXPOSURE_EV).contains(&exposure) {
			return Err("Sky exposure requires a finite value from -8 to 8 EV".into());
		}
		Ok(())
	}

	pub(crate) fn retained_bytes(&self) -> usize {
		self.active_sky.as_ref().map_or(0, |key| key.as_str().len())
	}
}

impl SceneData {
	fn enabled_sky(&self, key: &EntityKey) -> Option<&SkyComponent> {
		self.entities
			.iter()
			.find(|entity| {
				&entity.key == key && entity.parent.is_none() && entity.linked_prefab.is_none()
			})
			.and_then(|entity| entity.sky.as_deref())
			.filter(|sky| sky.enabled)
	}

	pub fn active_sky(&self) -> Option<&SkySettings> {
		self.environment
			.active_sky
			.as_ref()
			.and_then(|key| self.enabled_sky(key))
			.map(|sky| &sky.settings)
	}

	pub(crate) fn validate_environment(&self) -> Result<(), String> {
		SceneEnvironment::validate_lighting(
			self.environment.ambient_linear,
			self.environment.sky_exposure_ev,
		)?;
		if let Some(fog) = &self.environment.fog {
			fog.validate()?;
		}
		if self.environment.active_sky.is_some() && self.active_sky().is_none() {
			return Err("Scene Sky requires an enabled Sky on a scene-root object".into());
		}
		Ok(())
	}

	pub(crate) fn set_active_sky(&mut self, key: Option<EntityKey>) -> Result<(), String> {
		if key
			.as_ref()
			.is_some_and(|key| self.enabled_sky(key).is_none())
		{
			return Err("Choose an enabled Sky on a scene-root object".into());
		}
		self.environment.active_sky = key;
		Ok(())
	}

	pub(crate) fn clear_unavailable_sky(&mut self) {
		if self.environment.active_sky.is_some() && self.active_sky().is_none() {
			self.environment.active_sky = None;
		}
	}
}
