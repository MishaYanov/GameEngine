use crate::Camera;
use engine_core::{EntityKey, components::AudioSourceSpatial, world::RuntimeWorld};
use glam::Vec3;

#[derive(Clone, Copy)]
pub struct ListenerPose {
	pub position: [f32; 3],
	pub right: [f32; 3],
}

pub fn spatial_gains(source: [f32; 3], listener: ListenerPose, near: f32, far: f32) -> [f32; 2] {
	let offset = Vec3::from_array(source) - Vec3::from_array(listener.position);
	let distance = offset.length();
	if !distance.is_finite() || !near.is_finite() || !far.is_finite() || near < 0.0 || far <= near {
		return [0.0; 2];
	}
	let gain = (1.0 - (distance - near) / (far - near)).clamp(0.0, 1.0);
	let right = Vec3::from_array(listener.right).normalize_or_zero();
	let pan = (offset.normalize_or_zero().dot(right)).clamp(-1.0, 1.0);
	[
		gain * ((1.0 - pan) * 0.5).sqrt(),
		gain * ((1.0 + pan) * 0.5).sqrt(),
	]
}

pub fn scene_stereo_gains(
	world: &RuntimeWorld,
	camera: &Camera,
	key: &EntityKey,
	spatial: AudioSourceSpatial,
) -> [f32; 2] {
	let AudioSourceSpatial::Spatial { near, far } = spatial else {
		return [1.0; 2];
	};
	let Some(position) = world
		.entity_index(key)
		.and_then(|index| world.world_matrices().get(index))
		.map(|matrix| matrix.w_axis.truncate().to_array())
	else {
		return [0.0; 2];
	};
	spatial_gains(
		position,
		ListenerPose {
			position: camera.position,
			right: camera.right(),
		},
		near,
		far,
	)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn turning_swaps_channels_and_distance_fades_to_silence() {
		let listener = ListenerPose {
			position: [0.0; 3],
			right: [1.0, 0.0, 0.0],
		};
		let reversed = ListenerPose {
			right: [-1.0, 0.0, 0.0],
			..listener
		};
		assert_eq!(
			spatial_gains([-1.0, 0.0, 0.0], listener, 1.0, 5.0),
			[1.0, 0.0]
		);
		assert_eq!(
			spatial_gains([-1.0, 0.0, 0.0], reversed, 1.0, 5.0),
			[0.0, 1.0]
		);
		let center = spatial_gains([0.0; 3], listener, 1.0, 5.0);
		assert!(center[0].is_finite() && center[0] == center[1]);
		assert_eq!(
			spatial_gains([3.0, 0.0, 0.0], listener, 1.0, 5.0),
			[0.0, 0.5]
		);
		assert_eq!(spatial_gains([5.0, 0.0, 0.0], listener, 1.0, 5.0), [0.0; 2]);
	}
}
