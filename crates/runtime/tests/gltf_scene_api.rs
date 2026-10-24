mod support;
use runtime::assets::{AssetManager, gltf_loader};
use serde_json::{Value, json};
use std::{fs, sync::Arc};
use support::*;

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

#[test]
fn default_scene_import_retains_hierarchy_mesh_identity_local_trs_and_world_transforms() {
	let directory = Directory::new("gltf-scene");
	let path = write(&directory, &model());
	let imported = gltf_loader::load_scene(path).unwrap();
	assert_eq!(imported.scene_index, Some(1));
	assert_eq!(imported.root_node_indices, vec![1]);
	assert_eq!(
		imported
			.nodes
			.iter()
			.map(|n| n.node_index)
			.collect::<Vec<_>>(),
		vec![1, 2, 3]
	);
	assert_eq!(imported.nodes[0].mesh_index, None);
	assert_eq!(imported.nodes[1].mesh_index, Some(1));
	assert_eq!(imported.nodes[2].mesh_index, Some(1));
	assert_eq!(imported.nodes[1].parent_node_index, Some(1));
	assert_eq!(imported.nodes[1].local_transform.position, [0.0, 2.0, 0.0]);
	let matrix = glam::Mat4::from_cols_array(&imported.nodes[1].world_matrix);
	assert_eq!(matrix.w_axis.truncate().to_array(), [4.0, 2.0, 0.0]);
	assert_eq!(imported.mesh_count, 2);
	assert_eq!(imported.mesh_bounds.len(), 2);
	let bounds = imported.mesh_bounds[1].unwrap();
	assert_eq!(bounds.min(), [0.0; 3]);
	assert_eq!(bounds.max(), [1.0, 1.0, 0.0]);
}

#[test]
fn missing_default_selects_first_scene_and_missing_scene_is_an_explicit_error() {
	let directory = Directory::new("gltf-default");
	let mut value = model();
	value.as_object_mut().unwrap().remove("scene");
	let imported = gltf_loader::load_scene(write(&directory, &value)).unwrap();
	assert_eq!(imported.scene_index, Some(0));
	assert_eq!(imported.nodes.len(), 1);
	assert_eq!(imported.nodes[0].name, "Unused");
	value.as_object_mut().unwrap().remove("scenes");
	assert!(gltf_loader::load_scene(write(&directory, &value)).is_err());
}

#[test]
fn static_import_rejects_cycles_shared_children_and_unrepresentable_node_transforms() {
	let directory = Directory::new("gltf-invalid");
	for nodes in [
		json!([{"children":[1]},{"children":[0]}]),
		json!([{"children":[1,2]},{"children":[2]},{}]),
		json!([{"matrix":[1,0,0,0,0.5,1,0,0,0,0,1,0,0,0,0,1]}]),
		json!([{"matrix":[0,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]}]),
		json!([{"rotation":[0,0,0,2]}]),
	] {
		let value =
			json!({"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],"nodes":nodes});
		assert!(
			gltf_loader::load_scene(write(&directory, &value)).is_err(),
			"accepted {value}"
		);
	}
}

#[test]
fn static_import_bounds_depth_and_node_count_before_recursive_or_unbounded_work() {
	let directory = Directory::new("gltf-budget");
	let nodes = (0..130)
		.map(|i| {
			if i == 129 {
				json!({})
			} else {
				json!({"children":[i+1]})
			}
		})
		.collect::<Vec<_>>();
	let mut value = json!({"asset":{"version":"2.0"},"scenes":[{"nodes":[0]}],"nodes":nodes});
	assert!(
		gltf_loader::load_scene(write(&directory, &value))
			.unwrap_err()
			.to_string()
			.contains("128")
	);
	value["nodes"] = json!(vec![json!({}); 10001]);
	assert!(
		gltf_loader::load_scene(write(&directory, &value))
			.unwrap_err()
			.to_string()
			.contains("10000")
	);
}

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

fn metadata_result(
	service: &mut runtime::assets::AssetImportService,
) -> runtime::asset_jobs::AssetJobResult<runtime::assets::ImportedScene> {
	let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
	loop {
		if let Some(result) = service.try_drain() {
			return result;
		}
		assert!(
			std::time::Instant::now() < deadline,
			"metadata worker did not finish"
		);
		std::thread::sleep(std::time::Duration::from_millis(1));
	}
}

#[test]
fn real_cpu_import_jobs_notify_return_bounded_hierarchy_metadata_and_release_reservations() {
	use runtime::{
		asset_jobs::AssetJobLimits, asset_registry::AssetRef, assets::AssetImportService,
	};
	let directory = Directory::new("metadata-jobs");
	write(&directory, &model());
	let (notify, ready) = std::sync::mpsc::channel();
	let mut service =
		AssetImportService::with_notifier(&directory.0, AssetJobLimits::default(), move || {
			let _ = notify.send(());
		})
		.unwrap();
	let id = service
		.try_submit(AssetRef::new("scene.gltf", None).unwrap(), 1024 * 1024)
		.unwrap();
	ready
		.recv_timeout(std::time::Duration::from_secs(3))
		.unwrap();
	let result = metadata_result(&mut service);
	assert_eq!(result.id, id);
	let asset = result.result.unwrap();
	assert_eq!(asset.value.nodes.len(), 3);
	assert_eq!(asset.bytes, asset.value.retained_bytes());
	assert!(asset.bytes < 1024 * 1024);
	assert!(asset.metadata.estimated_bytes > 0);
	let bounds = asset.metadata.bounds.unwrap();
	assert_eq!(bounds.min(), [4.0, 0.0, 0.0]);
	assert_eq!(bounds.max(), [5.0, 3.0, 3.0]);
	let stats = service.try_stats().unwrap();
	assert_eq!((stats.pending_jobs, stats.reserved_bytes), (0, 0));
}

#[test]
fn cpu_metadata_jobs_enforce_requested_peak_budget_and_allow_explicit_mesh_selection() {
	use runtime::{
		asset_jobs::AssetJobLimits, asset_registry::AssetRef, assets::AssetImportService,
	};
	let directory = Directory::new("metadata-budget");
	write(&directory, &model());
	let mut service = AssetImportService::new(&directory.0, AssetJobLimits::default()).unwrap();
	service
		.try_submit(AssetRef::new("scene.gltf", None).unwrap(), 16)
		.unwrap();
	let error = metadata_result(&mut service).result.err().unwrap();
	assert!(format!("{error:?}").contains("reservation"));
	assert_eq!(service.try_stats().unwrap().reserved_bytes, 0);
	service
		.try_submit(AssetRef::new("scene.gltf", Some(1)).unwrap(), 1024 * 1024)
		.unwrap();
	let asset = metadata_result(&mut service).result.unwrap();
	assert!(asset.value.nodes.is_empty());
	assert_eq!(asset.value.meshes.len(), 1);
	assert_eq!(asset.value.meshes[0].mesh_index, 1);
	assert_eq!(asset.metadata.bounds.unwrap().min(), [0.0; 3]);
	service
		.try_submit(AssetRef::new("scene.gltf", Some(999)).unwrap(), 1024 * 1024)
		.unwrap();
	assert!(metadata_result(&mut service).result.is_err());
}

#[test]
fn cpu_metadata_worker_rejects_escaping_resource_references_and_survives_a_failed_job() {
	use runtime::{
		asset_jobs::AssetJobLimits, asset_registry::AssetRef, assets::AssetImportService,
	};
	let directory = Directory::new("metadata-containment");
	let root = directory.0.join("assets");
	fs::create_dir(&root).unwrap();
	fs::write(directory.0.join("outside.bin"), [0_u8; 42]).unwrap();
	let mut invalid = model();
	invalid["buffers"][0]["uri"] = json!("../outside.bin");
	fs::write(root.join("scene.gltf"), invalid.to_string()).unwrap();
	let mut service = AssetImportService::new(&root, AssetJobLimits::default()).unwrap();
	service
		.try_submit(AssetRef::new("scene.gltf", None).unwrap(), 1024 * 1024)
		.unwrap();
	let error = metadata_result(&mut service).result.err().unwrap();
	assert!(format!("{error:?}").contains("escapes asset root"));
	fs::write(root.join("scene.gltf"), model().to_string()).unwrap();
	service
		.try_submit(AssetRef::new("scene.gltf", None).unwrap(), 1024 * 1024)
		.unwrap();
	assert!(metadata_result(&mut service).result.is_ok());
}

#[test]
fn metadata_worker_skips_unused_images_and_retains_no_decoded_pixel_arrays() {
	use runtime::{
		asset_jobs::AssetJobLimits, asset_registry::AssetRef, assets::AssetImportService,
	};
	let directory = Directory::new("metadata-images");
	let mut value = model();
	value["images"] = json!([{"uri":"unused-missing.png"}]);
	write(&directory, &value);
	let mut service = AssetImportService::new(&directory.0, AssetJobLimits::default()).unwrap();
	service
		.try_submit(AssetRef::new("scene.gltf", None).unwrap(), 1024 * 1024)
		.unwrap();
	let asset = metadata_result(&mut service).result.unwrap();
	assert_eq!(asset.value.nodes.len(), 3);
	assert!(asset.value.referenced_bytes > 0);
	assert!(asset.bytes < 1024 * 1024);
}
