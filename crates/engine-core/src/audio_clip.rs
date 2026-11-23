//! Stable Audio Clip references and the first supported PCM format.
use crate::asset_catalog::AssetId;
use serde::{Deserialize, Serialize};

pub const AUDIO_CLIP_SAMPLE_RATE_HZ: u32 = 16_000;
pub const MAX_AUDIO_CLIP_SECONDS: u32 = 30;
pub const MAX_AUDIO_CLIP_FRAMES: u32 = AUDIO_CLIP_SAMPLE_RATE_HZ * MAX_AUDIO_CLIP_SECONDS;
pub const MAX_AUDIO_CLIP_PCM_BYTES: usize = MAX_AUDIO_CLIP_FRAMES as usize * 2;
pub const MAX_AUDIO_CLIP_WAV_BYTES: usize = 44 + MAX_AUDIO_CLIP_PCM_BYTES;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioClipRef {
	pub asset: AssetId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioClipInfo {
	pub sample_rate_hz: u32,
	pub frames: u32,
	pub channels: u8,
}
