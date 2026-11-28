use crate::{asset_catalog::AssetId, components::ComponentId};
use serde::{Deserialize, Deserializer, Serialize, de};
use std::{
	collections::{BTreeMap, HashSet},
	fmt,
};

/// Persistent Character identities indexed by the GLB coordinates at authoring time.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnimationCharacterIdentity {
	pub clips: Vec<String>,
	#[serde(deserialize_with = "deserialize_joint_ids")]
	pub joints: BTreeMap<u32, String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub root_node: Option<u32>,
}

impl AnimationCharacterIdentity {
	pub fn valid_id(id: &str) -> bool {
		!id.is_empty()
			&& id.len() <= 128
			&& id
			.bytes()
			.all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
	}

	pub fn validate(&self) -> Result<(), String> {
		fn unique_ids<'a>(ids: impl Iterator<Item=&'a String>) -> bool {
			let mut unique = HashSet::new();
			ids.into_iter()
				.all(|id| AnimationCharacterIdentity::valid_id(id) && unique.insert(id))
		}
		if !(1..=32).contains(&self.clips.len()) || !unique_ids(self.clips.iter()) {
			return Err("Character requires 1..32 valid, unique clip IDs".into());
		}
		if !(1..=super::MAX_SKIN_JOINTS).contains(&self.joints.len())
			|| self
			.joints
			.keys()
			.chain(self.root_node.iter())
			.any(|node| *node as usize >= super::MAX_ANIMATION_NODES)
			|| !unique_ids(self.joints.values())
		{
			return Err("Character requires valid node indices and 1..128 unique joint IDs".into());
		}
		Ok(())
	}

	pub(super) fn owned_bytes(&self) -> usize {
		self.clips.capacity() * std::mem::size_of::<String>()
			+ self.clips.iter().map(String::capacity).sum::<usize>()
			+ self.joints.len() * std::mem::size_of::<(u32, String)>()
			+ self.joints.values().map(String::capacity).sum::<usize>()
	}
}

fn deserialize_joint_ids<'de, D: Deserializer<'de>>(
	deserializer: D,
) -> Result<BTreeMap<u32, String>, D::Error> {
	struct JointIds;

	impl<'de> de::Visitor<'de> for JointIds {
		type Value = BTreeMap<u32, String>;

		fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
			formatter.write_str("a map of glTF node indices to Character joint IDs")
		}

		fn visit_map<M: de::MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
			let mut joints = BTreeMap::new();
			// Internally tagged components buffer JSON object keys as strings before
			// deserializing the binding. Parse the wire key explicitly in both paths.
			while let Some((key, id)) = map.next_entry::<String, String>()? {
				let node = key
					.parse::<u32>()
					.ok()
					.filter(|node| {
						(*node as usize) < super::MAX_ANIMATION_NODES && node.to_string() == key
					})
					.ok_or_else(|| {
						de::Error::custom(
							"Character joint node index must be canonical and bounded",
						)
					})?;
				if joints.len() >= super::MAX_SKIN_JOINTS {
					return Err(de::Error::custom("Character requires at most 128 joints"));
				}
				if joints.insert(node, id).is_some() {
					return Err(de::Error::custom("Character repeats a joint node index"));
				}
			}
			Ok(joints)
		}
	}

	deserializer.deserialize_map(JointIds)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnimationSourceRef {
	pub asset_id: AssetId,
	pub expected_sha256: String,
	pub scene_index: u32,
	pub mesh_node: u32,
	pub skin_index: u32,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub character: Option<AnimationCharacterIdentity>,
}

impl AnimationSourceRef {
	pub fn same_rig(&self, other: &Self) -> bool {
		self == other
			|| (self.asset_id == other.asset_id
			&& self.character.is_some()
			&& other.character.is_some())
	}

	pub fn validate(&self) -> Result<(), String> {
		if self.expected_sha256.len() != 64
			|| !self
			.expected_sha256
			.bytes()
			.all(|v| v.is_ascii_digit() || (b'a'..=b'f').contains(&v))
		{
			return Err("Animation source requires a lowercase SHA-256 fingerprint".into());
		}
		if let Some(character) = &self.character {
			character.validate()?;
		}
		Ok(())
	}
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
		self.source.validate()?;
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
		if let Some(character) = &self.source.character
			&& self
			.clip_indices()
			.iter()
			.any(|&index| index as usize >= character.clips.len())
		{
			return Err("Selected clip was absent from the authored Character revision".into());
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
		self.source.expected_sha256.capacity()
			+ self.source.asset_id.as_str().len()
			+ self
			.source
			.character
			.as_ref()
			.map_or(0, AnimationCharacterIdentity::owned_bytes)
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnimatorComponent {
	pub id: ComponentId,
	pub enabled: bool,
	pub settings: super::AnimatorSettings,
}
