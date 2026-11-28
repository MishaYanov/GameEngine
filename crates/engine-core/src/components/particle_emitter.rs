//! Authored configuration for bounded, deterministic visual particle emitters.

use serde::{Deserialize, Serialize};

use super::ComponentId;
use crate::asset_catalog::AssetId;

pub const MAX_ENABLED_PARTICLE_EMITTERS: usize = 64;
pub(crate) const MAX_PARTICLES_PER_EMITTER: usize = 192;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParticleEmitterSettings {
	pub rate_per_second: f32,
	pub lifetime_seconds: f32,
	pub speed: f32,
	/// Half-angle of the emission cone around the object's local +Y.
	pub spread_degrees: f32,
	/// Signed world Y acceleration in scene units per second squared.
	pub gravity: f32,
	pub size: f32,
	pub color: [f32; 3],
	pub seed: u32,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub sprite_asset_id: Option<AssetId>,
	#[serde(default, skip_serializing_if = "is_false")]
	pub linger_on_despawn: bool,
}

impl Default for ParticleEmitterSettings {
	fn default() -> Self {
		Self {
			rate_per_second: 32.0,
			lifetime_seconds: 1.8,
			speed: 2.0,
			spread_degrees: 28.0,
			gravity: 0.5,
			size: 0.15,
			color: [1.0, 0.35, 0.08],
			seed: 1,
			sprite_asset_id: None,
			linger_on_despawn: false,
		}
	}
}

impl ParticleEmitterSettings {
	pub fn validate(&self) -> Result<(), String> {
		for (name, value, min, max) in [
			("Rate", self.rate_per_second, 0.0, 128.0),
			("Lifetime", self.lifetime_seconds, 0.05, 10.0),
			("Speed", self.speed, 0.0, 40.0),
			("Spread", self.spread_degrees, 0.0, 90.0),
			("Gravity", self.gravity, -30.0, 30.0),
			("Size", self.size, 0.01, 2.0),
		] {
			if !(min..=max).contains(&value) {
				return Err(format!("Particle {name} must be from {min} to {max}"));
			}
		}
		if !self.color.iter().all(|value| (0.0..=1.0).contains(value)) {
			return Err("Particle color must have RGB channels from 0 to 1".into());
		}
		Ok(())
	}
}

/// One-shot emission with the same particle motion and visual settings as a trail.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParticleBurstSettings {
	pub count: u16,
	/// Burst emission ignores the continuous rate and trail retirement option.
	pub settings: ParticleEmitterSettings,
}

impl Default for ParticleBurstSettings {
	fn default() -> Self {
		Self {
			count: 12,
			settings: ParticleEmitterSettings::default(),
		}
	}
}

impl ParticleBurstSettings {
	pub fn validate(&self) -> Result<(), String> {
		if !(1..=MAX_PARTICLES_PER_EMITTER).contains(&usize::from(self.count)) {
			return Err(format!(
				"Particle burst count must be from 1 to {MAX_PARTICLES_PER_EMITTER}"
			));
		}
		self.settings.validate()
	}
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParticleEmitterComponent {
	pub id: ComponentId,
	pub enabled: bool,
	pub settings: ParticleEmitterSettings,
}

fn is_false(value: &bool) -> bool {
	!*value
}
