use super::ComponentId;
use crate::{
	animation::MarkerId, asset_catalog::AssetId, audio_clip::AudioClipRef, gameplay_events::Signal,
};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacyCombatCue {
	Swing,
	Hit,
	Hurt,
	Victory,
	Defeat,
}

impl LegacyCombatCue {
	pub fn id(self) -> &'static str {
		match self {
			Self::Swing => "swing",
			Self::Hit => "hit",
			Self::Hurt => "hurt",
			Self::Victory => "victory",
			Self::Defeat => "defeat",
		}
	}
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AudioEventBinding {
	GameplaySignal { signal: Signal },
	AnimationMarker { clip: AssetId, marker: MarkerId },
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
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub event: Option<AudioEventBinding>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub replaces_cue: Option<LegacyCombatCue>,
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
			event: None,
			replaces_cue: None,
		}
	}
}

impl AudioSourceSettings {
	pub fn validate(&self) -> Result<(), String> {
		if self.event.is_some()
			&& (self.playback != AudioSourcePlayback::OnEvent
				|| self.r#loop
				|| self.bus != AudioSourceBus::Effects)
		{
			return Err("Audio events require On Event, Effects and Loop off".into());
		}
		if self.replaces_cue.is_some() && (self.event.is_none() || self.clip.is_none()) {
			return Err("Replacing a legacy cue requires an event and Audio Clip".into());
		}
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
