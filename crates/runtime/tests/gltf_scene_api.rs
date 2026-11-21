use serde_json::{Value, json};
use std::fs;
use support::Directory;

#[path = "gltf_scene_api/cache.rs"]
mod cache;
#[path = "gltf_scene_api/decoding.rs"]
mod decoding;
#[path = "gltf_scene_api/metadata_jobs.rs"]
mod metadata_jobs;
mod support;

fn model() -> Value {
	let mut bytes = Vec::new();
	for value in [0.0_f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
		bytes.extend_from_slice(&value.to_le_bytes());
	}
	for index in [0_u16, 1, 2] {
		bytes.extend_from_slice(&index.to_le_bytes());
	}
	json!({"asset":{"version":"2.0"},"scene":1,"scenes":[{"nodes":[0]},{"nodes":[1]}],
        "nodes":[{"name":"Unused","mesh":0},{"name":"Root","translation":[4,0,0],"children":[2,3]},{"name":"First","mesh":1,"translation":[0,2,0]},{"name":"Second","mesh":1,"translation":[0,0,3]}],
        "buffers":[{"byteLength":42,"uri":format!("data:application/octet-stream;base64,{}",base64::encode(bytes))}],
        "bufferViews":[{"buffer":0,"byteOffset":0,"byteLength":36},{"buffer":0,"byteOffset":36,"byteLength":6}],
        "accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]},{"bufferView":1,"componentType":5123,"count":3,"type":"SCALAR"}],
        "meshes":[{"primitives":[{"attributes":{"POSITION":0},"indices":1}]},{"primitives":[{"attributes":{"POSITION":0},"indices":1}]}]})
}
fn write(directory: &Directory, value: &Value) -> std::path::PathBuf {
	let path = directory.0.join("scene.gltf");
	fs::write(&path, value.to_string()).unwrap();
	path
}
