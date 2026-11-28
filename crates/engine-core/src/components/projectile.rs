//! Authored motion and launch settings for a spawned game Prop.

use super::{ParticleBurstSettings, validate_model_path};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectileSettings {
	pub speed: f32,
	pub lifetime_ticks: u32,
	pub collision_mask: u32,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub impact: Option<ParticleBurstSettings>,
}

impl ProjectileSettings {
	pub fn validate(&self) -> Result<(), String> {
		if !(0.01..=1000.).contains(&self.speed) || !(1..=1_000_000).contains(&self.lifetime_ticks)
		{
			return Err("Projectile requires speed 0.01–1000 and lifetime 1–1000000 ticks".into());
		}
		if let Some(impact) = &self.impact {
			impact.validate()?;
		}
		Ok(())
	}
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectileLauncherSettings {
	pub prefab_path: String,
	pub action: String,
	pub max_alive: u32,
	/// Offset in the launcher's local orientation, expressed in world units.
	pub spawn_offset: [f32; 3],
}

impl ProjectileLauncherSettings {
	pub fn validate(&self) -> Result<(), String> {
		validate_model_path(&self.prefab_path)?;
		if !self.prefab_path.to_ascii_lowercase().ends_with(".json")
			|| !crate::input::valid_game_action_id(&self.action)
			|| !(1..=128).contains(&self.max_alive)
			|| !self.spawn_offset.iter().all(|offset| offset.abs() <= 1000.)
		{
			return Err("Projectile Launcher needs a relative JSON Prop, an Input Action ID, max alive 1–128, and finite offsets up to 1000 units".into());
		}
		Ok(())
	}
}
