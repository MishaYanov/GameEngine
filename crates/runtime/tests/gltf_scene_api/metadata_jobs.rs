use std::fs;

use serde_json::json;

use super::{Directory, model, write};

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
