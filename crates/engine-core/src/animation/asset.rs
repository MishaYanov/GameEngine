use crate::{Bounds3, asset_catalog::AssetId, components::ComponentId};
use glam::{Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};

pub const MAX_ANIMATION_NODES: usize = 256;
pub const MAX_SKIN_JOINTS: usize = 128;
pub const MAX_ANIMATION_INSTANCES: usize = 64;
pub const MAX_ANIMATION_BYTES: usize = 32 * 1024 * 1024;

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

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct LocalTrs {
	pub translation: [f32; 3],
	pub rotation_xyzw: [f32; 4],
	pub scale: [f32; 3],
}
impl LocalTrs {
	pub fn matrix(self) -> Mat4 {
		Mat4::from_scale_rotation_translation(
			Vec3::from_array(self.scale),
			Quat::from_array(self.rotation_xyzw),
			Vec3::from_array(self.translation),
		)
	}
	pub fn validate(&self) -> Result<(), String> {
		let q = Quat::from_array(self.rotation_xyzw);
		if self
			.translation
			.iter()
			.any(|v| !v.is_finite() || v.abs() > 1e6)
			|| self
				.scale
				.iter()
				.any(|v| !v.is_finite() || *v < 1e-4 || *v > 1e4)
			|| !q.is_finite()
			|| (q.length_squared() - 1.).abs() > 1e-3
		{
			return Err("Invalid animation TRS".into());
		}
		checked_similarity_inverse(self.matrix()).map(|_| ())
	}
}
#[derive(Clone, Debug)]
pub struct RigNode {
	pub source_node: u32,
	pub parent: Option<u16>,
	pub rest: LocalTrs,
}
#[derive(Clone, Copy, Debug)]
pub enum Interpolation {
	Step,
	Linear,
}
#[derive(Clone, Debug)]
pub enum TrackValues {
	Translation(Vec<[f32; 3]>),
	Rotation(Vec<[f32; 4]>),
	Scale(Vec<[f32; 3]>),
}
impl TrackValues {
	pub fn retained_bytes(&self) -> usize {
		match self {
			Self::Translation(v) | Self::Scale(v) => v.capacity() * 12,
			Self::Rotation(v) => v.capacity() * 16,
		}
	}
	pub fn len(&self) -> usize {
		match self {
			Self::Translation(v) | Self::Scale(v) => v.len(),
			Self::Rotation(v) => v.len(),
		}
	}
	pub fn is_empty(&self) -> bool {
		self.len() == 0
	}
}
#[derive(Clone, Debug)]
pub struct Track {
	pub node: u16,
	pub interpolation: Interpolation,
	pub times: Vec<f32>,
	pub values: TrackValues,
}
#[derive(Clone, Debug)]
pub struct Clip {
	pub source_index: u32,
	pub name: String,
	pub duration_seconds: f32,
	pub tracks: Vec<Track>,
}
#[derive(Clone, Debug)]
pub struct Skin {
	pub mesh_node: u16,
	pub source_mesh_node: u32,
	pub source_skin: u32,
	pub mesh_index: u32,
	pub joints: Vec<u16>,
	pub inverse_bind: Vec<Mat4>,
}
#[derive(Clone, Copy, Debug)]
pub struct SkinVertex {
	pub position: [f32; 3],
	pub normal: [f32; 3],
	pub uv: [f32; 2],
	pub joints: [u16; 4],
	pub weights: [f32; 4],
}
#[derive(Clone, Debug)]
pub struct SkinPrimitive {
	pub vertices: Vec<SkinVertex>,
	pub indices: Vec<u32>,
	pub color: [f32; 4],
	pub double_sided: bool,
	pub material_index: Option<usize>,
	pub base_color_image: Option<usize>,
}
#[derive(Clone, Debug)]
pub struct AnimationImage {
	pub width: u32,
	pub height: u32,
	pub rgba: Vec<u8>,
}
#[derive(Clone, Debug)]
pub struct AnimationAsset {
	pub source_sha256: String,
	pub source_scene: u32,
	pub nodes: Vec<RigNode>,
	pub skin: Skin,
	pub clips: Vec<Clip>,
	pub primitives: Vec<SkinPrimitive>,
	pub envelopes: Vec<Option<Bounds3>>,
	pub images: Vec<AnimationImage>,
}
impl AnimationAsset {
	pub fn validate_settings(&self, s: &AnimationSettings) -> Result<(), String> {
		s.validate()?;
		if s.source.expected_sha256 != self.source_sha256
			|| s.source.scene_index != self.source_scene
			|| s.source.mesh_node != self.skin.source_mesh_node
			|| s.source.skin_index != self.skin.source_skin
		{
			return Err("Animation source identity/version does not match prepared asset".into());
		}
		if s.clip_indices()
			.iter()
			.any(|&index| index as usize >= self.clips.len())
		{
			return Err("Selected animation clip does not exist".into());
		}
		Ok(())
	}
	pub fn joint_count(&self) -> usize {
		self.skin.joints.len()
	}
	pub fn retained_bytes(&self) -> usize {
		std::mem::size_of::<Self>()
			+ self.clips.capacity() * std::mem::size_of::<Clip>()
			+ self.primitives.capacity() * std::mem::size_of::<SkinPrimitive>()
			+ self.images.capacity() * std::mem::size_of::<AnimationImage>()
			+ self.envelopes.capacity() * std::mem::size_of::<Option<Bounds3>>()
			+ self.images.iter().map(|i| i.rgba.capacity()).sum::<usize>()
			+ self.source_sha256.capacity()
			+ self.nodes.capacity() * std::mem::size_of::<RigNode>()
			+ self.skin.joints.capacity() * 2
			+ self.skin.inverse_bind.capacity() * 64
			+ self
				.clips
				.iter()
				.map(|c| {
					c.name.capacity()
						+ c.tracks.capacity() * std::mem::size_of::<Track>()
						+ c.tracks
							.iter()
							.map(|t| t.times.capacity() * 4 + t.values.retained_bytes())
							.sum::<usize>()
				})
				.sum::<usize>()
			+ self
				.primitives
				.iter()
				.map(|p| {
					p.vertices.capacity() * std::mem::size_of::<SkinVertex>()
						+ p.indices.capacity() * 4
				})
				.sum::<usize>()
	}
}

pub fn checked_similarity_inverse(m: Mat4) -> Result<Mat4, String> {
	if !m.is_finite()
		|| m.x_axis.w.abs() > 1e-5
		|| m.y_axis.w.abs() > 1e-5
		|| m.z_axis.w.abs() > 1e-5
		|| (m.w_axis.w - 1.).abs() > 1e-5
	{
		return Err("Animated basis must be finite and affine".into());
	}
	let a = [
		m.x_axis.truncate(),
		m.y_axis.truncate(),
		m.z_axis.truncate(),
	];
	let l = a.map(|v| v.length());
	if l.iter().any(|v| !v.is_finite() || *v < 1e-4 || *v > 1e4) {
		return Err("Animated scale must be in [0.0001,10000]".into());
	}
	let max = l.into_iter().fold(0_f32, f32::max);
	if l.iter().any(|v| (*v - max).abs() > max * 1e-4) {
		return Err("Nonuniform animated scale is unsupported".into());
	}
	let [x, y, z] = [a[0] / l[0], a[1] / l[1], a[2] / l[2]];
	if x.dot(y).abs() > 1e-4
		|| x.dot(z).abs() > 1e-4
		|| y.dot(z).abs() > 1e-4
		|| x.cross(y).dot(z) < 0.999
	{
		return Err("Animated shear/reflection is unsupported".into());
	}
	let inverse = m.inverse();
	if !inverse.is_finite() {
		return Err("Nonfinite animated inverse".into());
	}
	Ok(inverse)
}
pub fn validate_times(times: &[f32], count: usize) -> Result<(), String> {
	if times.is_empty()
		|| times.len() > 4096
		|| times.len() != count
		|| times.iter().any(|v| !v.is_finite() || *v < 0. || *v > 600.)
		|| times.windows(2).any(|v| v[0] >= v[1])
	{
		return Err("Invalid animation key times/count".into());
	}
	Ok(())
}
pub fn validated_influences(
	j: [u16; 4],
	w: [f32; 4],
	count: usize,
) -> Result<([u16; 4], [f32; 4]), String> {
	if count == 0
		|| count > 128
		|| j.iter().any(|v| usize::from(*v) >= count)
		|| w.iter().any(|v| !v.is_finite() || *v < 0. || *v > 1.)
	{
		return Err("Invalid skin influences".into());
	}
	let sum: f64 = w.iter().map(|v| f64::from(*v)).sum();
	if sum <= 1e-8 || (sum - 1.).abs() > 0.01 {
		return Err("Skin weights must sum near one".into());
	}
	Ok((j, w.map(|v| (f64::from(v) / sum) as f32)))
}
