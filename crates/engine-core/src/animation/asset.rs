use super::{AnimationCharacterIdentity, AnimationSettings, AnimationSourceRef};
use crate::{Bounds3, asset_catalog::AssetId};
use glam::{Mat4, Quat, Vec3};
use serde::Serialize;

pub const MAX_ANIMATION_NODES: usize = 256;
pub const MAX_SKIN_JOINTS: usize = 128;
pub const MAX_ANIMATION_INSTANCES: usize = 64;
pub const MAX_ANIMATION_BYTES: usize = 32 * 1024 * 1024;

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
	pub name: Option<String>,
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
	pub normal_uv: [f32; 2],
	pub tangent: [f32; 4],
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
	pub normal_map: Option<SkinNormalMap>,
}
#[derive(Clone, Copy, Debug)]
pub struct SkinNormalMap {
	pub image: usize,
	pub scale: f32,
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
	pub character: Option<AnimationCharacterIdentity>,
	pub nodes: Vec<RigNode>,
	pub skin: Skin,
	pub clips: Vec<Clip>,
	pub primitives: Vec<SkinPrimitive>,
	pub envelopes: Vec<Option<Bounds3>>,
	pub images: Vec<AnimationImage>,
}
impl AnimationAsset {
	pub fn source_ref(&self, asset_id: AssetId) -> AnimationSourceRef {
		AnimationSourceRef {
			asset_id,
			expected_sha256: self.source_sha256.clone(),
			scene_index: self.source_scene,
			mesh_node: self.skin.source_mesh_node,
			skin_index: self.skin.source_skin,
			character: self.character.clone(),
		}
	}

	/// Resolve a Character joint to the runtime's dense node index.
	pub fn joint_node(&self, stable_id: &str) -> Option<u16> {
		let source_node = self
			.character
			.as_ref()?
			.joints
			.iter()
			.find_map(|(node, id)| (id == stable_id).then_some(*node))?;
		self.nodes
			.iter()
			.position(|node| node.source_node == source_node)
			.and_then(|index| u16::try_from(index).ok())
	}

	/// A replacement must retain every old Character identity. Validate before publication
	/// because a referenced object or Prop may be defined in an unopened document.
	pub fn require_compatible_character(&self, previous: &Self) -> Result<(), String> {
		if let Some(identity) = &previous.character {
			self.require_character_ids(identity)?;
			for joint in identity.joints.values() {
				if let Some(parent) = previous.joint_parent_id(joint)
					&& self.joint_parent_id(joint) != Some(parent)
				{
					return Err(format!(
						"Character joint {joint:?} changed its parent {parent:?}; review the reimport"
					));
				}
			}
		}
		Ok(())
	}

	fn joint_parent_id(&self, joint: &str) -> Option<&str> {
		let node = &self.nodes[usize::from(self.joint_node(joint)?)];
		let parent = &self.nodes[usize::from(node.parent?)];
		self.character
			.as_ref()?
			.joints
			.get(&parent.source_node)
			.map(String::as_str)
	}

	pub fn validate_source(&self, source: &AnimationSourceRef) -> Result<(), String> {
		source.validate()?;
		if source.expected_sha256 == self.source_sha256 {
			if source.scene_index != self.source_scene
				|| source.mesh_node != self.skin.source_mesh_node
				|| source.skin_index != self.skin.source_skin
				|| source
				.character
				.as_ref()
				.is_some_and(|identity| self.character.as_ref() != Some(identity))
			{
				return Err(
					"Animation source identity does not match the referenced revision".into(),
				);
			}
			return Ok(());
		}
		let previous = source
			.character
			.as_ref()
			.ok_or("Animation source changed; review and adopt its current version")?;
		self.require_character_ids(previous)
	}

	/// Translate a validated source's authored clip coordinate into this decoded revision.
	pub fn resolve_clip_index(
		&self,
		source: &AnimationSourceRef,
		index: u32,
	) -> Result<u32, String> {
		if source.expected_sha256 == self.source_sha256 {
			return self
				.clips
				.get(index as usize)
				.map(|_| index)
				.ok_or_else(|| "Selected animation clip does not exist".into());
		}
		let id = source
			.character
			.as_ref()
			.and_then(|identity| identity.clips.get(index as usize))
			.ok_or("Animation source changed; review the selected clip")?;
		self.character
			.as_ref()
			.and_then(|identity| identity.clips.iter().position(|candidate| candidate == id))
			.map(|index| index as u32)
			.ok_or_else(|| format!("Character clip {id:?} is absent; review the reimport"))
	}

	pub(super) fn resolve_root_node(
		&self,
		source: &AnimationSourceRef,
		source_node: u32,
	) -> Result<u32, String> {
		if source.expected_sha256 == self.source_sha256 {
			return Ok(source_node);
		}
		let (previous, current) = source
			.character
			.as_ref()
			.zip(self.character.as_ref())
			.ok_or("Animation source changed; review the root motion node")?;
		if let Some(id) = previous.joints.get(&source_node) {
			return current
				.joints
				.iter()
				.find_map(|(node, candidate)| (candidate == id).then_some(*node))
				.ok_or_else(|| format!("Character root motion joint {id:?} is absent"));
		}
		if previous.root_node == Some(source_node) {
			return current
				.root_node
				.ok_or_else(|| "Character has lost the shared root motion node".into());
		}
		Err("Character root motion source lacks a stable identity".into())
	}

	fn require_character_ids(&self, previous: &AnimationCharacterIdentity) -> Result<(), String> {
		let current = self
			.character
			.as_ref()
			.ok_or("Character reimport lacks stable joint and clip identities; review the asset")?;
		if let Some(id) = previous.clips.iter().find(|id| !current.clips.contains(id)) {
			return Err(format!(
				"Character clip {id:?} was removed; review the reimport"
			));
		}
		if let Some(id) = previous
			.joints
			.values()
			.find(|id| !current.joints.values().any(|current| current == *id))
		{
			return Err(format!(
				"Character joint {id:?} was removed; review the reimport"
			));
		}
		if previous.root_node.is_some() && current.root_node.is_none() {
			return Err(
				"Character reimport removed the shared rig root; review the reimport".into(),
			);
		}
		Ok(())
	}

	pub fn validate_settings(&self, s: &AnimationSettings) -> Result<(), String> {
		s.validate()?;
		self.validate_source(&s.source)?;
		for index in s.clip_indices() {
			self.resolve_clip_index(&s.source, index)?;
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
			+ self
			.character
			.as_ref()
			.map_or(0, AnimationCharacterIdentity::owned_bytes)
			+ self.nodes.capacity() * std::mem::size_of::<RigNode>()
			+ self
			.nodes
			.iter()
			.filter_map(|node| node.name.as_ref())
			.map(String::capacity)
			.sum::<usize>()
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
