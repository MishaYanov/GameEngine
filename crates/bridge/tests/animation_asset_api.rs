use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

use bridge::animation_asset;
use engine_core::{
	EntityKey, SceneData,
	animation::{AnimationAsset, AnimationPlayback},
	scene_data::CameraData,
};

#[path = "animation_asset_api/decoding.rs"]
mod decoding;
#[path = "animation_asset_api/runtime.rs"]
mod runtime;

const GLB: &[u8] = include_bytes!("../../../apps/game/assets/characters/two-joint.glb");
fn root() -> PathBuf {
	PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../apps/game/assets")
}
fn key(value: &str) -> EntityKey {
	EntityKey::new(value).unwrap()
}
fn decoded() -> Arc<AnimationAsset> {
	Arc::new(animation_asset::decode(GLB).unwrap())
}
fn close(a: f32, b: f32) {
	assert!((a - b).abs() < 2e-5, "{a} != {b}");
}

fn scene(looping: bool) -> SceneData {
	let all = SceneData::from_json(include_str!(
		"../../../apps/game/assets/scenes/animation-room.json"
	))
	.unwrap();
	let mut entity = all
		.entities
		.into_iter()
		.find(|e| e.key.as_str() == "clip-demo")
		.unwrap();
	entity.transform.position = [0.; 3];
	entity.animation.as_mut().unwrap().settings.playback = AnimationPlayback::SingleClip {
		clip_index: 1,
		looping,
	};
	let camera = engine_core::Camera::default();
	SceneData {
		schema_version: engine_core::scene_data::CURRENT_SCHEMA_VERSION,
		bindings: BTreeMap::new(),
		entities: vec![entity],
		camera: CameraData {
			follow: None,
			orbit: None,
			position: camera.position,
			yaw: camera.yaw,
			pitch: camera.pitch,
			vertical_fov: camera.vertical_fov,
			near_plane: camera.near,
			far_plane: camera.far,
		},
		lights: vec![],
	}
}
