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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioSourceSettings {
	pub clip: Option<AudioClipRef>,
	pub gain: f32,
	pub r#loop: bool,
	pub playback: AudioSourcePlayback,
	pub bus: AudioSourceBus,
}

impl Default for AudioSourceSettings {
	fn default() -> Self {
		Self {
			clip: None,
			gain: 1.0,
			r#loop: false,
			playback: AudioSourcePlayback::OnSceneEnter,
			bus: AudioSourceBus::Effects,
		}
	}
}

impl AudioSourceSettings {
	pub fn validate(&self) -> Result<(), String> {
		if self.gain.is_finite() && (0.0..=1.0).contains(&self.gain) {
			Ok(())
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
