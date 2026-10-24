#![allow(dead_code)]
use engine_core::{
	EntityKey, SceneData,
	scene_data::{CURRENT_SCHEMA_VERSION, CameraData, EntityData, TransformData},
};
use std::{
	fs,
	path::PathBuf,
	sync::atomic::{AtomicU64, Ordering},
};
pub fn key(value: &str) -> EntityKey {
	EntityKey::new(value).unwrap()
}
pub fn entity(value: &str, position: [f32; 3]) -> EntityData {
	EntityData::new(key(value), value, "models/triangle.gltf").with_transform(TransformData {
		position,
		..Default::default()
	})
}
pub fn scene(entities: Vec<EntityData>) -> SceneData {
	let camera = engine_core::Camera::default();
	SceneData {
		schema_version: CURRENT_SCHEMA_VERSION,
		bindings: Default::default(),
		entities,
		lights: vec![],
		camera: CameraData {
			position: camera.position,
			yaw: camera.yaw,
			pitch: camera.pitch,
			vertical_fov: camera.vertical_fov,
			near_plane: camera.near,
			far_plane: camera.far,
		},
	}
}
pub fn close(a: f32, b: f32) {
	assert!((a - b).abs() < 1e-5, "{a} != {b}");
}
pub struct Directory(pub PathBuf);
impl Directory {
	pub fn new(label: &str) -> Self {
		static NEXT: AtomicU64 = AtomicU64::new(0);
		let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
			.join("../../target")
			.join(format!(
				"qa-core-{label}-{}-{}",
				std::process::id(),
				NEXT.fetch_add(1, Ordering::Relaxed)
			));
		fs::create_dir(&path).unwrap();
		Self(path)
	}
}
impl Drop for Directory {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.0);
	}
}
