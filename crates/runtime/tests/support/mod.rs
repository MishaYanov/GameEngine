#![allow(dead_code)]
use runtime::{
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
	EntityData::new(key(value), value, "props/robot.glb").with_transform(TransformData {
		position,
		..TransformData::default()
	})
}
pub fn scene(entities: Vec<EntityData>) -> SceneData {
	let camera = runtime::Camera::default();
	SceneData {
		schema_version: CURRENT_SCHEMA_VERSION,
		bindings: Default::default(),
		entities,
		camera: CameraData {
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
pub fn close(actual: f32, expected: f32) {
	assert!((actual - expected).abs() < 1e-4, "{actual} != {expected}");
}
pub fn matrix_close(actual: glam::Mat4, expected: glam::Mat4) {
	for (a, b) in actual
		.to_cols_array()
		.into_iter()
		.zip(expected.to_cols_array())
	{
		close(a, b);
	}
}
pub struct Directory(pub PathBuf);
impl Directory {
	pub fn new(label: &str) -> Self {
		static NEXT: AtomicU64 = AtomicU64::new(0);
		let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
			.join("../../target")
			.join(format!(
				"qa-{label}-{}-{}",
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
