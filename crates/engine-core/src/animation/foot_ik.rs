//! One deform-chain foot placement constraint for a playing Character rig.
use super::{AnimationAsset, AnimationCharacterIdentity, AnimationPose, instance::make_pose};
use glam::{Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

const MM_TO_UNITS: f32 = 0.001;
const MIN_SEGMENT: f32 = 0.00001;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FootIkSettings {
	/// Persistent Character joint ID. Its two parents form the rest of the leg.
	pub foot_joint: String,
	/// How far above and below the animated sole to look for solid support.
	pub reach_mm: u16,
	/// Vertical distance between the ankle joint and the sole of the foot.
	pub sole_offset_mm: u16,
}

impl FootIkSettings {
	pub(super) fn validate(&self, character: &AnimationCharacterIdentity) -> Result<(), String> {
		if !character.joints.values().any(|id| id == &self.foot_joint) {
			return Err("Foot IK requires a joint from the imported Character".into());
		}
		if !(1..=1000).contains(&self.reach_mm) || self.sole_offset_mm > 1000 {
			return Err("Foot IK probe reach must be 1–1000 mm and sole offset 0–1000 mm".into());
		}
		Ok(())
	}
}

#[derive(Clone, Copy)]
pub(super) struct FootIkPlan {
	upper: usize,
	lower: usize,
	foot: usize,
	reach: f32,
	sole_offset: f32,
}

impl FootIkPlan {
	pub(super) fn prepare(
		asset: &AnimationAsset,
		settings: &FootIkSettings,
	) -> Result<Self, String> {
		let foot = asset
			.joint_node(&settings.foot_joint)
			.ok_or("Foot IK joint is unavailable")?;
		let (lower, upper) = asset.nodes[usize::from(foot)]
			.parent
			.and_then(|lower| Some((lower, asset.nodes.get(usize::from(lower))?.parent?)))
			.filter(|(lower, upper)| {
				asset.skin.joints.contains(lower) && asset.skin.joints.contains(upper)
			})
			.ok_or("Foot IK requires two consecutive deform-joint parents above the foot")?;
		let rest_length = |joint: u16| {
			Vec3::from_array(asset.nodes[usize::from(joint)].rest.translation).length()
		};
		if rest_length(foot) <= MIN_SEGMENT || rest_length(lower) <= MIN_SEGMENT {
			return Err("Foot IK requires two nonzero leg segments".into());
		}
		Ok(Self {
			upper: usize::from(upper),
			lower: usize::from(lower),
			foot: usize::from(foot),
			reach: f32::from(settings.reach_mm) * MM_TO_UNITS,
			sole_offset: f32::from(settings.sole_offset_mm) * MM_TO_UNITS,
		})
	}

	pub(super) fn apply(
		self,
		pose: &AnimationPose,
		placement: Mat4,
		probe: impl FnOnce(Vec3, f32) -> Option<f32>,
	) -> Result<Option<AnimationPose>, String> {
		let ankle = pose.globals[self.foot].w_axis.truncate();
		let animated = placement.transform_point3(ankle);
		let sole = animated - self.sole_offset * Vec3::Y;
		let Some(height) = probe(sole, self.reach) else {
			return Ok(None);
		};
		let adjustment = height - sole.y;
		if adjustment.abs() <= MIN_SEGMENT {
			return Ok(None);
		}
		let target = ankle + placement.inverse().transform_vector3(adjustment * Vec3::Y);
		let hip = pose.globals[self.upper].w_axis.truncate();
		let knee = pose.globals[self.lower].w_axis.truncate();
		let Some((desired_knee, desired_ankle)) = solve_leg(hip, knee, ankle, target) else {
			return Ok(None);
		};

		let mut local = pose.local.clone();
		let hip_parent = pose.asset.nodes[self.upper]
			.parent
			.map_or(Mat4::IDENTITY, |parent| pose.globals[usize::from(parent)]);
		let hip_inverse = hip_parent.inverse();
		let upper_delta = rotation_between(hip_inverse, knee - hip, desired_knee - hip);
		local[self.upper].rotation_xyzw = (upper_delta
			* Quat::from_array(local[self.upper].rotation_xyzw))
			.normalize()
			.to_array();
		let hip_global = hip_parent * local[self.upper].matrix();
		let lower_direction = local[self.lower]
			.matrix()
			.transform_vector3(Vec3::from_array(local[self.foot].translation));
		let lower_target = hip_global
			.inverse()
			.transform_vector3(desired_ankle - desired_knee);
		let lower_delta =
			Quat::from_rotation_arc(lower_direction.normalize(), lower_target.normalize());
		local[self.lower].rotation_xyzw = (lower_delta
			* Quat::from_array(local[self.lower].rotation_xyzw))
			.normalize()
			.to_array();

		// Keep the animated ankle orientation while the hip and knee bend.
		let knee_global = hip_global * local[self.lower].matrix();
		let (_, ankle_rotation, _) =
			(knee_global.inverse() * pose.globals[self.foot]).to_scale_rotation_translation();
		local[self.foot].rotation_xyzw = ankle_rotation.normalize().to_array();
		make_pose(
			Arc::clone(&pose.asset),
			local,
			placement,
			pose.elapsed_ticks,
			pose.clip_index,
			pose.mode,
			pose.revision,
		)
			.map(Some)
	}
}

fn solve_leg(hip: Vec3, knee: Vec3, ankle: Vec3, target: Vec3) -> Option<(Vec3, Vec3)> {
	let upper = (knee - hip).length();
	let lower = (ankle - knee).length();
	let delta = target - hip;
	let aim = delta.try_normalize()?;
	if upper <= MIN_SEGMENT || lower <= MIN_SEGMENT {
		return None;
	}
	let margin = upper.min(lower) * 0.0001;
	let distance = delta
		.length()
		.clamp((upper - lower).abs() + margin, upper + lower - margin);
	let bend = knee - hip;
	let pole = (bend - aim * bend.dot(aim))
		.try_normalize()
		.filter(|_| bend.cross(aim).length() > MIN_SEGMENT)
		.unwrap_or_else(|| aim.any_orthonormal_vector());
	let forward = (upper * upper + distance * distance - lower * lower) / (2.0 * distance);
	let side = (upper * upper - forward * forward).max(0.0).sqrt();
	Some((hip + aim * forward + pole * side, hip + aim * distance))
}

fn rotation_between(inverse: Mat4, before: Vec3, after: Vec3) -> Quat {
	Quat::from_rotation_arc(
		inverse.transform_vector3(before).normalize(),
		inverse.transform_vector3(after).normalize(),
	)
}
