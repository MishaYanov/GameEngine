use crate::{asset_catalog::AssetId, components::ComponentId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnimationSourceRef {
	pub asset_id: AssetId,
	pub expected_sha256: String,
	pub scene_index: u32,
	pub mesh_node: u32,
	pub skin_index: u32,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AnimationPlayback {
	SingleClip { clip_index: u32, looping: bool },
	Locomotion3 { idle: u32, walk: u32, air: u32 },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnimationSettings {
	pub source: AnimationSourceRef,
	pub playback: AnimationPlayback,
	pub speed_numerator: u16,
	pub speed_denominator: u16,
}
impl AnimationSettings {
	pub fn validate(&self) -> Result<(), String> {
		if self.source.expected_sha256.len() != 64
			|| !self
				.source
				.expected_sha256
				.bytes()
				.all(|v| v.is_ascii_digit() || (b'a'..=b'f').contains(&v))
		{
			return Err("Animation source requires a lowercase SHA-256 fingerprint".into());
		}
		let (n, d) = (
			u32::from(self.speed_numerator),
			u32::from(self.speed_denominator),
		);
		if !(1..=16).contains(&n) || !(1..=16).contains(&d) || n * 4 < d || n > d * 4 {
			return Err(
				"Animation speed requires a ratio in 1/4..=4 with numerator/denominator 1..=16"
					.into(),
			);
		}
		if self.clip_indices().iter().any(|&v| v >= 32) {
			return Err("Animation clip index exceeds 31".into());
		}
		Ok(())
	}
	pub fn clip_indices(&self) -> Vec<u32> {
		match self.playback {
			AnimationPlayback::SingleClip { clip_index, .. } => vec![clip_index],
			AnimationPlayback::Locomotion3 { idle, walk, air } => vec![idle, walk, air],
		}
	}
	pub fn owned_bytes(&self) -> usize {
		self.source.expected_sha256.capacity() + self.source.asset_id.as_str().len()
	}
	pub fn retained_bytes(&self) -> usize {
		std::mem::size_of::<Self>() + self.owned_bytes()
	}
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnimationComponent {
	pub id: ComponentId,
	pub enabled: bool,
	pub settings: AnimationSettings,
}
