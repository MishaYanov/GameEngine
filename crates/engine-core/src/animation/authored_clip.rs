use crate::{
	EntityKey, asset_catalog::AssetId, components::ComponentId, linked_prefab::PropAddress,
	scene_catalog::SceneId, scene_data::TransformData,
};
use glam::{EulerRot, Quat, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

mod resolve;
mod timeline;
pub use timeline::{ClipKeyValue, MarkerId, TimelineEdit, TimelineMarker};

pub const AUTHORED_CLIP_TICKS_PER_SECOND: u32 = 1_000;
pub const MAX_AUTHORED_CLIP_TICKS: u32 = 600 * AUTHORED_CLIP_TICKS_PER_SECOND;
pub const MAX_AUTHORED_CLIP_BYTES: usize = 1024 * 1024;
const MAX_TRACKS: usize = 64;
const MAX_KEYS: usize = 4_096;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AuthoredClipOwner {
	Scene { scene_id: SceneId },
	Prop { asset_id: AssetId },
}

/// Imported and authored clips retain distinct, exact asset revisions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AnimationClipRef {
	Imported {
		source: super::AnimationSourceRef,
		clip_index: u32,
	},
	Authored {
		asset_id: AssetId,
		expected_sha256: String,
	},
}

impl AnimationClipRef {
	pub fn validate(&self) -> Result<(), String> {
		match self {
			Self::Imported { source, clip_index } => super::AnimationSettings {
				source: source.clone(),
				playback: super::AnimationPlayback::SingleClip {
					clip_index: *clip_index,
					looping: false,
				},
				speed_numerator: 1,
				speed_denominator: 1,
			}
			.validate(),
			Self::Authored {
				expected_sha256, ..
			} => {
				if expected_sha256.len() == 64
					&& expected_sha256
						.bytes()
						.all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
				{
					Ok(())
				} else {
					Err("Authored Animation Clip reference requires a lowercase SHA-256 fingerprint".into())
				}
			}
		}
	}
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum VisualTargetId {
	Scene { entity_key: EntityKey },
	Prop { address: PropAddress },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VisualInterpolation {
	Hold,
	#[default]
	Linear,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClipKey<T> {
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub id: Option<ComponentId>,
	pub at: u32,
	pub value: T,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum VisualTrack {
	LocalPosition {
		#[serde(default)]
		interpolation: VisualInterpolation,
		keys: Vec<ClipKey<[f32; 3]>>,
	},
	LocalRotation {
		#[serde(default)]
		interpolation: VisualInterpolation,
		keys: Vec<ClipKey<[f32; 4]>>,
	},
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopedTrack {
	pub target: VisualTargetId,
	pub track: VisualTrack,
}

/// Authored times are whole milliseconds. A portable clip owns one document scope.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthoredClip {
	pub schema_version: u32,
	pub name: String,
	pub owner: AuthoredClipOwner,
	pub duration_ticks: u32,
	pub tracks: Vec<ScopedTrack>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub markers: Vec<TimelineMarker>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VisualPose {
	pub entity_key: EntityKey,
	pub position: [f32; 3],
	pub rotation_xyzw: [f32; 4],
	pub scale: [f32; 3],
}

pub struct ResolvedAuthoredClip {
	clip: AuthoredClip,
	bindings: BTreeMap<VisualTargetId, VisualPose>,
}

impl AuthoredClip {
	pub fn from_json(bytes: &[u8]) -> Result<Self, String> {
		if bytes.len() > MAX_AUTHORED_CLIP_BYTES {
			return Err("Authored clip exceeds the 1 MiB size limit".into());
		}
		let clip: Self = serde_json::from_slice(bytes)
			.map_err(|error| format!("Invalid authored clip: {error}"))?;
		clip.validate()?;
		Ok(clip)
	}

	pub fn to_json(&self) -> Result<Vec<u8>, String> {
		self.validate()?;
		let bytes = serde_json::to_vec_pretty(self).map_err(|error| error.to_string())?;
		if bytes.len() > MAX_AUTHORED_CLIP_BYTES {
			return Err("Authored clip exceeds the 1 MiB size limit".into());
		}
		Ok(bytes)
	}

	pub fn validate(&self) -> Result<(), String> {
		if !matches!(self.schema_version, 1 | 2) {
			return Err("Unsupported authored clip schema".into());
		}
		if self.name.trim().is_empty()
			|| self.name.len() > 128
			|| self.name.chars().any(char::is_control)
		{
			return Err("Authored clip requires a name of 1–128 bytes with no controls".into());
		}
		if !(1..=MAX_AUTHORED_CLIP_TICKS).contains(&self.duration_ticks) {
			return Err("Authored clip duration must be 1–600000 milliseconds".into());
		}
		if !(1..=MAX_TRACKS).contains(&self.tracks.len()) {
			return Err("Authored clip requires 1–64 visual tracks".into());
		}
		let mut targets = BTreeSet::new();
		let mut key_count = 0;
		for track in &self.tracks {
			validate_target(&self.owner, &track.target)?;
			let (kind, count) = track.track.validate(self.duration_ticks)?;
			key_count += count;
			if key_count > MAX_KEYS {
				return Err("Authored clip exceeds 4096 total keys".into());
			}
			if !targets.insert((&track.target, kind)) {
				return Err("Authored clip has two tracks for the same target and property".into());
			}
		}
		self.validate_timeline_ids()
	}
}

impl ResolvedAuthoredClip {
	pub fn markers(&self) -> &[TimelineMarker] {
		&self.clip.markers
	}

	pub fn retained_bytes(&self) -> usize {
		let key_bytes = self
			.clip
			.tracks
			.iter()
			.map(|track| match &track.track {
				VisualTrack::LocalPosition { keys, .. } => {
					keys.capacity() * (std::mem::size_of::<ClipKey<[f32; 3]>>() + 128)
				}
				VisualTrack::LocalRotation { keys, .. } => {
					keys.capacity() * (std::mem::size_of::<ClipKey<[f32; 4]>>() + 128)
				}
			})
			.sum::<usize>();
		key_bytes + self.clip.tracks.len() * 1024 + self.clip.markers.len() * 512
	}

	pub fn duration_millis(&self) -> u32 {
		self.clip.duration_ticks
	}
	pub fn target_keys(&self) -> impl Iterator<Item = &EntityKey> {
		self.bindings.values().map(|pose| &pose.entity_key)
	}

	pub fn duration_ticks(&self) -> u32 {
		self.clip.duration_ticks
	}

	pub fn sample(&self, tick: u32) -> Result<Vec<VisualPose>, String> {
		if tick > self.clip.duration_ticks {
			return Err("Sample time is outside the authored clip".into());
		}
		let mut poses = self.bindings.clone();
		for scoped in &self.clip.tracks {
			let pose = poses
				.get_mut(&scoped.target)
				.ok_or("Authored clip target is unresolved")?;
			scoped.track.sample_into(tick, pose)?;
		}
		Ok(poses.into_values().collect())
	}
}

impl VisualPose {
	fn bind(entity_key: EntityKey, transform: &TransformData) -> Self {
		let [x, y, z] = transform.rotation;
		Self {
			entity_key,
			position: transform.position,
			rotation_xyzw: Quat::from_euler(EulerRot::XYZ, x, y, z).to_array(),
			scale: transform.scale,
		}
	}

	pub fn transform(&self) -> TransformData {
		let (x, y, z) = Quat::from_array(self.rotation_xyzw).to_euler(EulerRot::XYZ);
		TransformData {
			position: self.position,
			rotation: [x, y, z],
			scale: self.scale,
		}
	}
}

impl VisualTrack {
	fn validate(&self, duration: u32) -> Result<(u8, usize), String> {
		match self {
			Self::LocalPosition { keys, .. } => {
				validate_keys(keys, duration, |value| {
					value
						.iter()
						.all(|v| v.is_finite() && v.abs() <= 1_000_000.0)
				})?;
				Ok((0, keys.len()))
			}
			Self::LocalRotation { keys, .. } => {
				validate_keys(keys, duration, |value| {
					let rotation = Quat::from_array(*value);
					rotation.is_finite() && (rotation.length_squared() - 1.0).abs() <= 1.0e-4
				})?;
				Ok((1, keys.len()))
			}
		}
	}

	fn sample_into(&self, tick: u32, pose: &mut VisualPose) -> Result<(), String> {
		match self {
			Self::LocalPosition {
				interpolation,
				keys,
			} => {
				let (a, b, t) = key_span(keys, tick, *interpolation);
				pose.position = Vec3::from_array(*a)
					.lerp(Vec3::from_array(*b), t)
					.to_array();
			}
			Self::LocalRotation {
				interpolation,
				keys,
			} => {
				let (a, b, t) = key_span(keys, tick, *interpolation);
				pose.rotation_xyzw = super::short_slerp(*a, *b, t)?;
			}
		}
		Ok(())
	}
}

fn validate_target(owner: &AuthoredClipOwner, target: &VisualTargetId) -> Result<(), String> {
	let key = match (owner, target) {
		(AuthoredClipOwner::Scene { .. }, VisualTargetId::Scene { entity_key }) => entity_key,
		(
			AuthoredClipOwner::Prop { .. },
			VisualTargetId::Prop {
				address: PropAddress::Object {
					placement_path,
					source_entity,
				},
			},
		) if placement_path.len() <= crate::linked_prefab::MAX_PROP_NESTING => source_entity,
		_ => {
			return Err(
				"Authored clip targets must be visual objects in the owning Scene or Prop".into(),
			);
		}
	};
	key.validate()?;
	if key.as_str().len() > 256 {
		return Err("Authored clip object keys are limited to 256 bytes".into());
	}
	Ok(())
}

fn validate_keys<T>(
	keys: &[ClipKey<T>],
	duration: u32,
	valid: impl Fn(&T) -> bool,
) -> Result<(), String> {
	if keys.is_empty()
		|| keys.len() > MAX_KEYS
		|| keys.windows(2).any(|window| window[0].at >= window[1].at)
		|| keys
			.iter()
			.any(|key| key.at > duration || !valid(&key.value))
	{
		return Err(
			"Clip keys must be finite, in range, and strictly ordered with normalized rotations"
				.into(),
		);
	}
	Ok(())
}

fn key_span<T>(
	keys: &[ClipKey<T>],
	tick: u32,
	interpolation: VisualInterpolation,
) -> (&T, &T, f32) {
	let after = keys.partition_point(|key| key.at <= tick);
	let start = &keys[after.saturating_sub(1)];
	let Some(end) = keys
		.get(after)
		.filter(|end| end.at != start.at && interpolation == VisualInterpolation::Linear)
	else {
		return (&start.value, &start.value, 0.0);
	};
	let ratio = tick.saturating_sub(start.at) as f32 / (end.at - start.at) as f32;
	(&start.value, &end.value, ratio)
}
