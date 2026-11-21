use std::{fs, sync::Arc};

use runtime::assets::AssetManager;
use serde_json::json;

use super::{Directory, model, write};

#[test]
fn scene_descriptor_cache_shares_canonical_paths_freezes_snapshot_and_clear_reloads() {
	let directory = Directory::new("gltf-cache");
	let mut value = model();
	write(&directory, &value);
	let mut assets = AssetManager::new(&directory.0).unwrap();
	let first = assets.load_scene("scene.gltf").unwrap();
	let alias = assets.load_scene(directory.0.join("./scene.gltf")).unwrap();
	assert!(Arc::ptr_eq(&first, &alias));
	value["nodes"][1]["name"] = json!("Changed");
	write(&directory, &value);
	let cached = assets.load_scene("scene.gltf").unwrap();
	assert_eq!(cached.nodes[0].name, "Root");
	assets.clear();
	let refreshed = assets.load_scene("scene.gltf").unwrap();
	assert!(!Arc::ptr_eq(&first, &refreshed));
	assert_eq!(refreshed.nodes[0].name, "Changed");
	assert!(assets.load_scene("../outside.gltf").is_err());
}

#[test]
fn descriptor_fifo_evicts_after_eight_models_while_existing_arc_stays_usable() {
	let directory = Directory::new("gltf-fifo");
	let value =
		json!({"asset":{"version":"2.0"},"scenes":[{"nodes":[0]}],"nodes":[{"name":"Original"}]});
	for index in 0..9 {
		fs::write(
			directory.0.join(format!("model-{index}.gltf")),
			value.to_string(),
		)
		.unwrap();
	}
	let mut assets = AssetManager::new(&directory.0).unwrap();
	let first = assets.load_scene("model-0.gltf").unwrap();
	for index in 1..9 {
		assets.load_scene(format!("model-{index}.gltf")).unwrap();
	}
	let reloaded = assets.load_scene("model-0.gltf").unwrap();
	assert!(!Arc::ptr_eq(&first, &reloaded));
	assert_eq!(first.nodes[0].name, "Original");
}
