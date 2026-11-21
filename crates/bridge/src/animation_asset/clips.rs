use std::collections::{BTreeMap, HashSet};

use engine_core::animation::{Clip, Interpolation, LocalTrs, Track, TrackValues, validate_times};
use glam::Quat;

use super::decode::{charge, require_f32};

pub(super) fn decode_clips(
	doc: &gltf::Document,
	bin: &[u8],
	mapping: &BTreeMap<usize, u16>,
	remaining: &mut usize,
) -> Result<Vec<Clip>, String> {
	let mut clips = Vec::new();
	let mut total_keys = 0usize;
	for animation in doc.animations() {
		let mut tracks = Vec::new();
		let mut seen = HashSet::new();
		let mut duration = 0_f32;
		for channel in animation.channels() {
			if tracks.len() >= 768 {
				return Err("Animation clip exceeds 768 channels".into());
			}
			let target = channel.target();
			let dense = *mapping
				.get(&target.node().index())
				.ok_or("Animation target outside scene")?;
			let property = target.property();
			if !seen.insert((dense, format!("{property:?}"))) {
				return Err("Duplicate animation node/property channel".into());
			}
			let sampler = channel.sampler();
			require_f32(&sampler.input(), gltf::accessor::Dimensions::Scalar)?;
			match property {
				gltf::animation::Property::Translation | gltf::animation::Property::Scale => {
					require_f32(&sampler.output(), gltf::accessor::Dimensions::Vec3)?
				}
				gltf::animation::Property::Rotation => {
					let a = sampler.output();
					if a.dimensions() != gltf::accessor::Dimensions::Vec4
						|| !matches!(
							(a.data_type(), a.normalized()),
							(gltf::accessor::DataType::F32, false)
								| (
									gltf::accessor::DataType::I8
										| gltf::accessor::DataType::U8 | gltf::accessor::DataType::I16
										| gltf::accessor::DataType::U16,
									true
								)
						) {
						return Err("Invalid rotation output encoding".into());
					}
				}
				_ => return Err("Morph animation is outside the subset".into()),
			}
			let interpolation = match sampler.interpolation() {
				gltf::animation::Interpolation::Step => Interpolation::Step,
				gltf::animation::Interpolation::Linear => Interpolation::Linear,
				_ => return Err("CUBICSPLINE is outside the animation subset".into()),
			};
			let count = sampler.input().count();
			total_keys = total_keys.checked_add(count).ok_or("Key count overflow")?;
			if count > 4096 || total_keys > 262144 || sampler.output().count() != count {
				return Err("Animation key count exceeds bounds or differs from output".into());
			}
			charge(remaining, count, 20)?;
			let reader = channel.reader(|b| if b.index() == 0 { Some(bin) } else { None });
			let times: Vec<_> = reader
				.read_inputs()
				.ok_or("Missing animation times")?
				.collect();
			use gltf::animation::util::ReadOutputs;
			let values = match reader.read_outputs().ok_or("Missing animation values")? {
				ReadOutputs::Translations(v) => {
					let v: Vec<_> = v.collect();
					if v.iter().flatten().any(|v| !v.is_finite() || v.abs() > 1e6) {
						return Err("Invalid translation keys".into());
					}
					TrackValues::Translation(v)
				}
				ReadOutputs::Scales(v) => {
					let v: Vec<_> = v.collect();
					for scale in &v {
						LocalTrs {
							translation: [0.; 3],
							rotation_xyzw: [0., 0., 0., 1.],
							scale: *scale,
						}
						.validate()?;
					}
					TrackValues::Scale(v)
				}
				ReadOutputs::Rotations(v) => {
					let mut v: Vec<_> = v.into_f32().collect();
					for value in &mut v {
						let q = Quat::from_array(*value);
						if !q.is_finite() || (q.length_squared() - 1.).abs() > 1e-3 {
							return Err("Invalid rotation keys".into());
						}
						*value = q.normalize().to_array();
					}
					TrackValues::Rotation(v)
				}
				_ => return Err("Morph animation is outside the subset".into()),
			};
			validate_times(&times, values.len())?;
			duration = duration.max(*times.last().unwrap());
			tracks.push(Track {
				node: dense,
				interpolation,
				times,
				values,
			});
		}
		if tracks.is_empty() {
			return Err("Animation clip has no supported channels".into());
		}
		let name = animation
			.name()
			.map(str::to_owned)
			.unwrap_or_else(|| format!("Clip {}", animation.index()));
		if name.len() > 1024 {
			return Err("Animation clip name exceeds 1024 bytes".into());
		}
		clips.push(Clip {
			source_index: animation.index() as u32,
			name,
			duration_seconds: duration,
			tracks,
		});
	}
	Ok(clips)
}
