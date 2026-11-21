use super::*;
use glam::{Mat4, Quat, Vec3};

pub fn short_slerp(a: [f32; 4], b: [f32; 4], t: f32) -> Result<[f32; 4], String> {
	let (mut a, mut b) = (Quat::from_array(a), Quat::from_array(b));
	if !t.is_finite()
		|| !(0. ..=1.).contains(&t)
		|| !a.is_finite()
		|| !b.is_finite()
		|| a.length_squared() < 1e-12
		|| b.length_squared() < 1e-12
	{
		return Err("Invalid quaternion interpolation".into());
	}
	a = a.normalize();
	b = b.normalize();
	let mut d = a.dot(b);
	if d < 0. {
		b = -b;
		d = -d;
	}
	d = d.clamp(0., 1.);
	let q = if d > 0.9995 {
		(a * (1. - t) + b * t).normalize()
	} else {
		let angle = d.acos();
		(a * (((1. - t) * angle).sin() / angle.sin()) + b * ((t * angle).sin() / angle.sin()))
			.normalize()
	};
	Ok(q.to_array())
}
fn interval(times: &[f32], t: f64) -> Result<(usize, usize, f32), String> {
	if times.is_empty() || !t.is_finite() {
		return Err("Invalid sample time/keys".into());
	}
	if t <= f64::from(times[0]) {
		return Ok((0, 0, 0.));
	}
	let last = times.len() - 1;
	if t >= f64::from(times[last]) {
		return Ok((last, last, 0.));
	}
	let b = times.partition_point(|v| f64::from(*v) <= t);
	let a = b - 1;
	let d = f64::from(times[b]) - f64::from(times[a]);
	if d <= 0. {
		return Err("Unsorted animation keys".into());
	}
	Ok((a, b, ((t - f64::from(times[a])) / d) as f32))
}
pub fn clip_time(t: f64, d: f32, looping: bool) -> Result<f64, String> {
	if !t.is_finite() || t < 0. || !d.is_finite() || d < 0. {
		return Err("Invalid clip time".into());
	}
	Ok(if d == 0. {
		0.
	} else if looping {
		t.rem_euclid(f64::from(d))
	} else {
		t.min(f64::from(d))
	})
}
pub fn sample_local(
	asset: &AnimationAsset,
	clip: usize,
	time: f64,
) -> Result<Vec<LocalTrs>, String> {
	let clip = asset.clips.get(clip).ok_or("Unknown animation clip")?;
	let mut pose: Vec<_> = asset.nodes.iter().map(|n| n.rest).collect();
	for track in &clip.tracks {
		let out = pose
			.get_mut(usize::from(track.node))
			.ok_or("Unknown track node")?;
		let (a, b, t) = interval(&track.times, time)?;
		let t = match track.interpolation {
			Interpolation::Step => 0.,
			Interpolation::Linear => t,
		};
		match &track.values {
			TrackValues::Translation(v) => {
				out.translation = Vec3::from_array(*v.get(a).ok_or("Missing key")?)
					.lerp(Vec3::from_array(*v.get(b).ok_or("Missing key")?), t)
					.to_array();
			}
			TrackValues::Scale(v) => {
				out.scale = Vec3::from_array(*v.get(a).ok_or("Missing key")?)
					.lerp(Vec3::from_array(*v.get(b).ok_or("Missing key")?), t)
					.to_array();
			}
			TrackValues::Rotation(v) => {
				out.rotation_xyzw = short_slerp(
					*v.get(a).ok_or("Missing key")?,
					*v.get(b).ok_or("Missing key")?,
					t,
				)?;
			}
		}
	}
	Ok(pose)
}
pub fn global_pose(asset: &AnimationAsset, pose: &[LocalTrs]) -> Result<Vec<Mat4>, String> {
	if pose.len() != asset.nodes.len() {
		return Err("Pose size differs from rig".into());
	}
	let mut out: Vec<Mat4> = Vec::with_capacity(pose.len());
	for (i, (node, p)) in asset.nodes.iter().zip(pose).enumerate() {
		p.validate()?;
		let m = match node.parent {
			None => p.matrix(),
			Some(parent) if usize::from(parent) < i => out[usize::from(parent)] * p.matrix(),
			_ => return Err("Rig is not topologically ordered".into()),
		};
		checked_similarity_inverse(m)?;
		out.push(m);
	}
	Ok(out)
}
pub fn mesh_local_palette(asset: &AnimationAsset, globals: &[Mat4]) -> Result<Vec<Mat4>, String> {
	let inv = checked_similarity_inverse(
		*globals
			.get(usize::from(asset.skin.mesh_node))
			.ok_or("Missing mesh basis")?,
	)?;
	if asset.skin.joints.len() != asset.skin.inverse_bind.len() {
		return Err("Skin matrix count mismatch".into());
	}
	asset
		.skin
		.joints
		.iter()
		.zip(&asset.skin.inverse_bind)
		.map(|(j, b)| {
			let m = inv * *globals.get(usize::from(*j)).ok_or("Missing joint")? * *b;
			if !m.is_finite() {
				return Err("Nonfinite palette".into());
			}
			Ok(m)
		})
		.collect()
}
pub fn blend_poses(a: &[LocalTrs], b: &[LocalTrs], t: f32) -> Result<Vec<LocalTrs>, String> {
	if a.len() != b.len() || a.len() > MAX_ANIMATION_NODES {
		return Err("Pose blend rig mismatch".into());
	}
	a.iter()
		.zip(b)
		.map(|(a, b)| {
			Ok(LocalTrs {
				translation: Vec3::from_array(a.translation)
					.lerp(Vec3::from_array(b.translation), t)
					.to_array(),
				scale: Vec3::from_array(a.scale)
					.lerp(Vec3::from_array(b.scale), t)
					.to_array(),
				rotation_xyzw: short_slerp(a.rotation_xyzw, b.rotation_xyzw, t)?,
			})
		})
		.collect()
}
pub fn build_envelopes(asset: &AnimationAsset) -> Result<Vec<Option<crate::Bounds3>>, String> {
	let mut boxes: Vec<Option<crate::Bounds3>> = vec![None; asset.skin.joints.len()];
	for vertex in asset.primitives.iter().flat_map(|p| &p.vertices) {
		for n in 0..4 {
			if vertex.weights[n] == 0. {
				continue;
			}
			let j = usize::from(vertex.joints[n]);
			let p = asset
				.skin
				.inverse_bind
				.get(j)
				.ok_or("Unknown envelope joint")?
				.transform_point3(Vec3::from_array(vertex.position));
			if !p.is_finite() {
				return Err("Nonfinite envelope".into());
			}
			let p = p.to_array();
			let point = crate::Bounds3::new(p, p)?;
			boxes[j] = Some(boxes[j].map_or(point, |b| b.union(point)));
		}
	}
	Ok(boxes)
}
pub fn animated_bounds(
	asset: &AnimationAsset,
	globals: &[Mat4],
	placement: Mat4,
) -> Result<crate::Bounds3, String> {
	let mut min = Vec3::splat(f32::INFINITY);
	let mut max = Vec3::splat(f32::NEG_INFINITY);
	let mut found = false;
	for (j, envelope) in asset.skin.joints.iter().zip(&asset.envelopes) {
		let Some(b) = envelope else { continue };
		let m = placement * *globals.get(usize::from(*j)).ok_or("Missing bound joint")?;
		let b = b.transformed(m)?;
		min = min.min(Vec3::from_array(b.min()));
		max = max.max(Vec3::from_array(b.max()));
		found = true;
	}
	if !found {
		return Err("Animation has no weighted geometry".into());
	}
	let pad = Vec3::splat(1e-5 * min.abs().max(max.abs()).max_element().max(1.));
	crate::Bounds3::new((min - pad).to_array(), (max + pad).to_array())
}
