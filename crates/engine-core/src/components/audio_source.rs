use super::ComponentId;
use crate::audio_clip::AudioClipRef;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioSourcePlayback {
	OnSceneEnter,
	OnEvent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioSourceBus {
	Effects,
	Music,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum AudioSourceSpatial {
	#[default]
	TwoD,
	Spatial {
		near: f32,
		far: f32,
	},
}

impl AudioSourceSpatial {
	fn is_two_d(&self) -> bool {
		matches!(self, Self::TwoD)
	}

	fn validate(self) -> Result<(), String> {
		match self {
			Self::TwoD => Ok(()),
			Self::Spatial { near, far }
				if near.is_finite()
					&& far.is_finite()
					&& near >= 0.0 && far > near
					&& far <= 10_000.0 =>
			{
				Ok(())
			}
			Self::Spatial { .. } => {
				Err("Spatial Audio Source distances require 0 ≤ near < far ≤ 10000".into())
			}
		}
	}
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioSourceSettings {
	pub clip: Option<AudioClipRef>,
	pub gain: f32,
	pub r#loop: bool,
	pub playback: AudioSourcePlayback,
	pub bus: AudioSourceBus,
	#[serde(default, skip_serializing_if = "AudioSourceSpatial::is_two_d")]
	pub spatial: AudioSourceSpatial,
}

impl Default for AudioSourceSettings {
	fn default() -> Self {
		Self {
			clip: None,
			gain: 1.0,
			r#loop: false,
			playback: AudioSourcePlayback::OnSceneEnter,
			bus: AudioSourceBus::Effects,
			spatial: AudioSourceSpatial::TwoD,
		}
	}
}

impl AudioSourceSettings {
	pub fn validate(&self) -> Result<(), String> {
		if self.gain.is_finite() && (0.0..=1.0).contains(&self.gain) {
			self.spatial.validate()
		} else {
			Err("Audio Source gain requires a finite value from 0 to 1".into())
		}
	}
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioSourceComponent {
	pub id: ComponentId,
	pub enabled: bool,
	pub settings: AudioSourceSettings,
}
