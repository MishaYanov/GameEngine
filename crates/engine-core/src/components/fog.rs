use serde::{Deserialize, Serialize};

/// A world-space atmosphere that is densest below its base height.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneFog {
	/// Linear RGB color of the scattered light.
	pub color_linear: [f32; 3],
	/// Extinction per world unit at and below the base height.
	pub density: f32,
	/// World Y at which the density begins to decrease.
	pub base_height: f32,
	/// Exponential falloff per world unit above the base height.
	pub height_falloff: f32,
	/// Camera distance in world units before air contributes to the fog.
	#[serde(default)]
	pub start_distance: f32,
}

impl SceneFog {
	pub(crate) fn validate(&self) -> Result<(), String> {
		if !self
			.color_linear
			.iter()
			.all(|value| (0.0..=1.0).contains(value))
		{
			return Err("Fog color requires finite linear RGB values from 0 to 1".into());
		}
		if !(0.0..=1.0).contains(&self.density) || !(0.0..=1.0).contains(&self.height_falloff) {
			return Err("Fog density and height falloff require finite values from 0 to 1".into());
		}
		if !self.base_height.is_finite()
			|| !self.start_distance.is_finite()
			|| self.start_distance < 0.0
		{
			return Err(
				"Fog requires a finite base height and nonnegative finite start distance".into(),
			);
		}
		Ok(())
	}
}
