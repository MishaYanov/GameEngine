use super::*;

#[test]
fn hidden_model_availability_uses_resolved_extension_and_keeps_logical_markers_optional() {
	let fixture = Fixture::new();
	std::fs::write(fixture.root.join("marker.txt"), "logical marker").unwrap();
	let assets = AssetManager::new(&fixture.root).unwrap();
	let reference = |path| engine_core::asset_registry::AssetRef::new(path, None).unwrap();
	assert!(!assets.asset_exists(&reference("marker.txt")).unwrap());
	assert!(!assets.asset_exists(&reference("missing.gltf")).unwrap());
	assert!(assets.asset_exists(&reference("active.gltf")).unwrap());
	let bytes = std::fs::read(fixture.root.join("active.gltf")).unwrap();
	let id = engine_core::asset_catalog::AssetId::generate();
	let record = engine_core::asset_catalog::PersistentAsset {
		id: id.clone(),
		path: "active.gltf".into(),
		kind: engine_core::asset_catalog::AssetKind::Model,
		aliases: vec!["marker.txt".into()],
		revision: 1,
		fingerprint: super::super::project_assets::fingerprint(&bytes),
		bytes: bytes.len() as u64,
		source_path: None,
		meshes: vec![engine_core::asset_catalog::MeshSignature {
			index: 0,
			name: "Mesh".into(),
			primitives: 1,
		}],
		dependencies: vec![],
	};
	let catalog = engine_core::asset_catalog::AssetCatalog {
		schema_version: 1,
		assets: vec![record],
	};
	std::fs::write(
		fixture.root.join(engine_core::asset_catalog::CATALOG_FILE),
		catalog.to_json().unwrap(),
	)
	.unwrap();
	assert!(
		assets.asset_exists(&reference("marker.txt")).unwrap(),
		"supported resolved model wins over legacy alias extension"
	);
	assert!(
		assets
			.asset_exists(&reference("unrelated.note").with_asset_id(Some(id)))
			.unwrap(),
		"explicit stable identity resolves before extension filtering"
	);
}

#[test]
fn candidate_promotion_replaces_all_source_keys_and_discard_preserves_active_snapshot() {
	let fixture = Fixture::new();
	let mut assets = AssetManager::new(&fixture.root).unwrap();
	let original = Arc::new(Model::new(Vec::new()));
	let candidate = Arc::new(Model::new(Vec::new()));
	let active = engine_core::asset_registry::AssetRef::new("active.gltf", Some(0)).unwrap();
	let pending = engine_core::asset_registry::AssetRef::new("unused.gltf", Some(0)).unwrap();
	assets.models.insert(active.clone(), Arc::clone(&original));
	assets
		.models
		.insert(pending.clone(), Arc::clone(&candidate));
	for path in ["active.gltf", "unused.gltf"] {
		assets.cache_resources(&fixture.root.join(path), 0).unwrap();
		drop(assets.load_scene(path).unwrap());
	}
	let prepared = || PreparedGpuAsset {
		animation_candidate: None,
		candidate: fixture.root.join("unused.gltf"),
		destination: fixture.root.join("active.gltf"),
		candidate_relative: "unused.gltf".into(),
		destination_relative: "active.gltf".into(),
		reuses_source: false,
	};
	assets.discard_gpu_asset(prepared());
	assert!(Arc::ptr_eq(&assets.models[&active], &original));
	assert!(!assets.models.contains_key(&pending));
	assert_eq!(assets.resource_cache.len(), 1);
	assets.models.insert(pending, Arc::clone(&candidate));
	assets
		.cache_resources(&fixture.root.join("unused.gltf"), 0)
		.unwrap();
	drop(assets.load_scene("unused.gltf").unwrap());
	assets.promote_gpu_asset(prepared());
	assert!(Arc::ptr_eq(&assets.models[&active], &candidate));
	assert_eq!(
		Arc::strong_count(&original),
		1,
		"external old scene keeps its snapshot alive"
	);
	assert_eq!(assets.resource_cache.len(), 1);
	assert_eq!(assets.resource_bytes, 36);
	assert_eq!(assets.resource_order, [fixture.root.join("active.gltf")]);
	assert_eq!(assets.scene_order, [fixture.root.join("active.gltf")]);
	assert_eq!(
		assets.scene_bytes,
		assets
			.scenes
			.iter()
			.map(|(path, scene)| scene.retained_bytes() + path.as_os_str().len() * 4 + 128)
			.sum::<usize>()
	);
}

struct Fixture {
	root: PathBuf,
}

impl Fixture {
	fn new() -> Self {
		let root = std::env::temp_dir().join(format!("asset-cache-{}", uuid::Uuid::new_v4()));
		std::fs::create_dir(&root).unwrap();
		let root = root.canonicalize().unwrap();
		let source = serde_json::json!({
			"asset": {"version": "2.0"},
			"buffers": [{"byteLength": 36, "uri": format!("data:application/octet-stream;base64,{}", base64::encode([0; 36]))}],
			"bufferViews": [{"buffer":0,"byteLength":36}],
			"accessors": [{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[0,0,0]}],
			"meshes": [{"primitives":[{"attributes":{"POSITION":0}}]}],
			"nodes": [{"mesh":0}], "scenes": [{"nodes":[0]}],
		});
		for name in ["active.gltf", "metadata.gltf", "unused.gltf"] {
			std::fs::write(root.join(name), source.to_string()).unwrap();
		}
		Self { root }
	}
}

impl Drop for Fixture {
	fn drop(&mut self) {
		let _ = std::fs::remove_dir_all(&self.root);
	}
}

#[test]
fn pruning_keeps_live_model_and_metadata_sources_and_reconciles_accounting() {
	let fixture = Fixture::new();
	let mut assets = AssetManager::new(&fixture.root).unwrap();
	for name in ["active.gltf", "metadata.gltf", "unused.gltf"] {
		assets.cache_resources(&fixture.root.join(name), 0).unwrap();
		drop(assets.load_scene(name).unwrap());
	}
	let model = Arc::new(Model::new(Vec::new()));
	assets.models.insert(
		engine_core::asset_registry::AssetRef::new("active.gltf", Some(0)).unwrap(),
		Arc::clone(&model),
	);
	let metadata = assets.load_scene("metadata.gltf").unwrap();
	assert_eq!(assets.resource_bytes, 108);
	assets.prune_unused();
	assert_eq!(assets.resource_cache.len(), 2);
	assert_eq!(assets.resource_bytes, 72);
	assert_eq!(
		assets.resource_order.iter().collect::<Vec<_>>(),
		vec![
			&fixture.root.join("active.gltf"),
			&fixture.root.join("metadata.gltf"),
		]
	);
	assert_eq!(assets.scene_order.len(), 2);
	assert_eq!(
		assets.scene_bytes,
		assets
			.scenes
			.iter()
			.map(|(path, scene)| { scene.retained_bytes() + path.as_os_str().len() * 4 + 128 })
			.sum::<usize>()
	);
	assets.prune_unused();
	assert_eq!(
		assets.resource_bytes, 72,
		"repeated pruning must keep live sources"
	);
	drop(model);
	drop(metadata);
	assets.prune_unused();
	assert!(assets.resource_cache.is_empty());
	assert!(assets.resource_order.is_empty());
	assert_eq!(assets.resource_bytes, 0);
	assert!(assets.scenes.is_empty());
	assert!(assets.scene_order.is_empty());
	assert_eq!(assets.scene_bytes, 0);
	assets
		.cache_resources(&fixture.root.join("unused.gltf"), 0)
		.unwrap();
	assert_eq!(assets.resource_bytes, 36);
	assert_eq!(assets.resource_order.len(), 1);
}

#[test]
fn failed_new_mesh_preparation_does_not_change_cached_entry_or_accounting() {
	let fixture = Fixture::new();
	let mut assets = AssetManager::new(&fixture.root).unwrap();
	let path = fixture.root.join("active.gltf");
	assets.cache_resources(&path, 0).unwrap();
	let order = assets.resource_order.clone();
	let bytes = assets.resource_bytes;
	assert!(assets.cache_resources(&path, 1).is_err());
	assert_eq!(assets.resource_order, order);
	assert_eq!(assets.resource_bytes, bytes);
	assert_eq!(assets.resource_cache[&path].1, bytes);
	assert!(
		assets
			.cache_resources(&fixture.root.join("unused.gltf"), 1)
			.is_err()
	);
	assert_eq!(assets.resource_cache.len(), 1);
	assert_eq!(assets.resource_order, order);
	assert_eq!(assets.resource_bytes, bytes);
}

#[test]
fn preparing_additional_mesh_images_updates_cached_bytes_without_duplicate_entries() {
	let fixture = Fixture::new();
	let path = fixture.root.join("active.gltf");
	let mut source: serde_json::Value =
		serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
	source["accessors"]
		.as_array_mut()
		.unwrap()
		.push(serde_json::json!({
			"bufferView":0,"componentType":5126,"count":3,"type":"VEC2",
		}));
	source["images"] = serde_json::json!([{"uri":"first.png"}, {"uri":"second.png"}]);
	source["textures"] = serde_json::json!([{"source":0}, {"source":1}]);
	source["materials"] = serde_json::json!([
		{"pbrMetallicRoughness":{"baseColorTexture":{"index":0}}},
		{"pbrMetallicRoughness":{"baseColorTexture":{"index":1}}},
	]);
	source["meshes"] = serde_json::json!([
		{"primitives":[{"attributes":{"POSITION":0,"TEXCOORD_0":1},"material":0}]},
		{"primitives":[{"attributes":{"POSITION":0,"TEXCOORD_0":1},"material":1}]},
	]);
	std::fs::write(&path, source.to_string()).unwrap();
	for name in ["first.png", "second.png"] {
		image::RgbaImage::from_pixel(8, 8, image::Rgba([1, 2, 3, 255]))
			.save(fixture.root.join(name))
			.unwrap();
	}
	let encoded_bytes = std::fs::metadata(fixture.root.join("second.png"))
		.unwrap()
		.len() as usize;
	assert!(encoded_bytes < 256);
	let mut assets = AssetManager::new(&fixture.root).unwrap();
	assets.cache_resources(&path, 0).unwrap();
	assert_eq!(assets.resource_bytes, 36 + 256 + encoded_bytes);
	assets.cache_resources(&path, 1).unwrap();
	assert_eq!(assets.resource_bytes, 36 + 256 * 2);
	assert_eq!(assets.resource_cache[&path].1, assets.resource_bytes);
	assets.cache_resources(&path, 0).unwrap();
	assert_eq!(assets.resource_bytes, 36 + 256 * 2);
	assert_eq!(assets.resource_order.len(), 1);
	assets.prune_unused();
	assert_eq!(assets.resource_bytes, 0);
	assert!(assets.resource_order.is_empty());
}
