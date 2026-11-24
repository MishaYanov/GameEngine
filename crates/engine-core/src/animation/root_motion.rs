use super::{
	AnimationAsset, AnimationClipRef, AnimatorGraph, Interpolation, LocalTrs, TrackValues,
	clip_time, sample::interval, validate_times,
};
use glam::Vec2;
use std::collections::{BTreeMap, BTreeSet};

const MAX_ROOT_SPEED: f32 = 100.0;

#[derive(Clone, Copy)]
pub(crate) struct RootMotionDelta {
	pub displacement: Vec2,
	pub motor_weight: f32,
}

/// Compiled against a validated imported graph/rig pair. Shares keyframes with the rig.
pub(super) struct RootMotionPlan {
	node: u16,
	rest_planar: Vec2,
	tracks: BTreeMap<u32, RootTrack>,
}

struct RootTrack {
	channel: usize,
	cycle: Vec2,
}

impl RootMotionPlan {
	pub(super) fn prepare(
		source_node: u32,
		graph: &AnimatorGraph,
		asset: &AnimationAsset,
	) -> Result<Self, String> {
		let selected = asset
			.nodes
			.iter()
			.position(|node| node.source_node == source_node)
			.and_then(|index| u16::try_from(index).ok())
			.ok_or("Root motion source node is absent from the imported rig")?;
		let root = |mut node: u16| -> bool {
			for _ in 0..asset.nodes.len() {
				let Some(rig_node) = asset.nodes.get(usize::from(node)) else {
					return false;
				};
				match rig_node.parent {
					Some(parent) => node = parent,
					None => return node == selected,
				}
			}
			false
		};
		if !root(asset.skin.mesh_node)
			|| asset.skin.joints.iter().copied().any(|joint| !root(joint))
		{
			return Err(
				"Root motion node must be the rig root shared by the mesh and all joints".into(),
			);
		}
		let mut tracks = BTreeMap::new();
		let mut visited = BTreeSet::new();
		for state in &graph.states {
			let AnimationClipRef::Imported { clip_index, .. } = state.clip else {
				return Err("Root motion requires imported clips".into());
			};
			if !visited.insert(clip_index) {
				continue;
			}
			let clip = asset
				.clips
				.get(clip_index as usize)
				.ok_or("Root motion clip is missing")?;
			let mut translations = clip
				.tracks
				.iter()
				.enumerate()
				.filter_map(|(channel, track)| match &track.values {
					TrackValues::Translation(values) if track.node == selected => {
						Some((channel, track, values))
					}
					_ => None,
				});
			let Some((channel, track, values)) = translations.next() else {
				continue;
			};
			if translations.next().is_some() {
				return Err("Root motion clip repeats the source node translation channel".into());
			}
			validate_times(&track.times, values.len())?;
			if !values
				.windows(2)
				.any(|pair| planar(pair[0]) != planar(pair[1]))
			{
				continue;
			}
			if !matches!(track.interpolation, Interpolation::Linear)
				|| clip.duration_seconds <= 0.0
				|| !clip.duration_seconds.is_finite()
				|| track
					.times
					.last()
					.is_some_and(|last| *last > clip.duration_seconds)
				|| values
					.iter()
					.flatten()
					.any(|axis| !axis.is_finite() || axis.abs() > 1.0e6)
			{
				return Err(format!(
					"Root motion clip {:?} requires finite linear translation keys within its duration",
					clip.name
				));
			}
			if values
				.windows(2)
				.zip(track.times.windows(2))
				.any(|(values, times)| {
					planar(values[0]).distance(planar(values[1]))
						> (times[1] - times[0]) * MAX_ROOT_SPEED
				}) {
				return Err(format!(
					"Root motion clip {:?} exceeds {MAX_ROOT_SPEED} units per second",
					clip.name
				));
			}
			let cycle =
				planar(*values.last().expect("validated translation keys")) - planar(values[0]);
			tracks.insert(clip_index, RootTrack { channel, cycle });
		}
		if tracks.is_empty() {
			return Err(
				"Animator clips contain no planar translation on the selected root motion node"
					.into(),
			);
		}
		Ok(Self {
			node: selected,
			rest_planar: planar(asset.nodes[usize::from(selected)].rest.translation),
			tracks,
		})
	}

	pub(super) fn remove_pose_translation(&self, pose: &mut [LocalTrs]) {
		let translation = &mut pose[usize::from(self.node)].translation;
		translation[0] = self.rest_planar.x;
		translation[2] = self.rest_planar.y;
	}

	pub(super) fn delta(
		&self,
		asset: &AnimationAsset,
		index: u32,
		from: f64,
		through: f64,
		looping: bool,
	) -> Result<Option<Vec2>, String> {
		let Some(root) = self.tracks.get(&index) else {
			return Ok(None);
		};
		let clip = &asset.clips[index as usize];
		let duration = f64::from(clip.duration_seconds);
		let track = &clip.tracks[root.channel];
		let TrackValues::Translation(values) = &track.values else {
			return Err("Prepared root motion track changed type".into());
		};
		let sample = |time: f64| -> Result<Vec2, String> {
			let (a, b, weight) = interval(
				&track.times,
				clip_time(time, clip.duration_seconds, looping)?,
			)?;
			Ok(planar(values[a]).lerp(planar(values[b]), weight))
		};
		let wraps = if looping {
			(through / duration).floor() - (from / duration).floor()
		} else {
			0.0
		};
		let displacement = sample(through)? - sample(from)? + root.cycle * wraps as f32;
		if !displacement.is_finite() {
			return Err("Root motion displacement is nonfinite".into());
		}
		Ok(Some(displacement))
	}
}

const fn planar(value: [f32; 3]) -> Vec2 {
	Vec2::new(value[0], value[2])
}
