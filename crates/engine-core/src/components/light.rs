//! Authored light settings and their world-space renderer projection.
use glam::{Mat4, Vec3};
use serde::{Deserialize, Serialize};

use crate::{
	EntityKey,
	scene_data::{LightData, LightKind},
};

use super::ComponentId;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LightComponent {
	pub id: ComponentId,
	pub enabled: bool,
	pub settings: LightSettings,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LightSettings {
	pub kind: LightKind,
	pub color: [f32; 3],
	pub intensity: f32,
	pub range: f32,
}

impl LightSettings {
	pub fn validate(self) -> Result<(), String> {
		if !self.color.iter().all(|value| (0.0..=1.0).contains(value))
			|| !(0.0..=1_000_000.0).contains(&self.intensity)
			|| !self.range.is_finite()
			|| self.range <= 0.0
		{
			return Err("Light requires a linear RGB color, intensity from 0 to 1000000, and positive range".into());
		}
		Ok(())
	}

	pub fn world_direction(world: Mat4) -> [f32; 3] {
		world
			.transform_vector3(Vec3::NEG_Z)
			.try_normalize()
			.unwrap_or(Vec3::NEG_Z)
			.to_array()
	}

	pub fn project(self, key: &EntityKey, name: &str, world: Mat4) -> LightData {
		LightData {
			key: key.clone(),
			name: name.into(),
			kind: self.kind,
			position: world.transform_point3(Vec3::ZERO).to_array(),
			direction: Self::world_direction(world),
			color: self.color,
			intensity: self.intensity,
			range: self.range,
		}
	}
}

impl From<&LightData> for LightSettings {
	fn from(light: &LightData) -> Self {
		Self {
			kind: light.kind,
			color: light.color,
			intensity: light.intensity,
			range: light.range,
		}
	}
}
