use super::*;
use engine_core::asset_catalog::{AssetCatalog, CATALOG_FILE, MeshSignature};
use std::{borrow::Cow, cell::Cell, fs};

const CUBE: &[u8] = include_bytes!("../../../../../apps/game/assets/props/cube.glb");

fn adapted_cube(edit: impl FnOnce(&mut serde_json::Value)) -> Vec<u8> {
	let mut glb = gltf::binary::Glb::from_slice(CUBE).unwrap();
	let mut document = serde_json::from_slice(&glb.json).unwrap();
	edit(&mut document);
	glb.json = Cow::Owned(serde_json::to_vec(&document).unwrap());
	glb.to_vec().unwrap()
}

struct Scratch(PathBuf);

impl Scratch {
	fn new() -> Self {
		let nonce = AssetId::generate();
		let target = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
		fs::create_dir_all(&target).unwrap();
		let root = target.join(format!("warm-model-{}", nonce.as_str()));
		fs::create_dir(&root).unwrap();
		Self(root.canonicalize().unwrap())
	}
}

impl Drop for Scratch {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.0);
	}
}

#[test]
fn ready_charge_survives_while_another_reservation_uses_available_scratch() {
	let live = AtomicUsize::new(0);
	let mut first = Reservation::scratch(&live).unwrap();
	assert_eq!(first.bytes, MAX_WARM_BYTES);
	assert!(Reservation::scratch(&live).is_none());
	first.keep(MAX_BATCH_BYTES);
	let mut second = Reservation::scratch(&live).unwrap();
	assert_eq!(second.bytes, MAX_WARM_BYTES - MAX_BATCH_BYTES);
	second.keep(4096);
	assert_eq!(live.load(Ordering::Acquire), MAX_BATCH_BYTES + 4096);
	drop(first);
	assert_eq!(live.load(Ordering::Acquire), 4096);
	drop(second);
	assert_eq!(live.load(Ordering::Acquire), 0);
}

#[test]
fn aliases_share_exact_mesh_identity_and_cancelled_batches_discard_partial_models() {
	let scratch = Scratch::new();
	let model = scratch.0.join("cube.glb");
	let bytes = adapted_cube(|document| {
		let copy = document["meshes"][0].clone();
		document["meshes"].as_array_mut().unwrap().push(copy);
	});
	fs::write(&model, &bytes).unwrap();
	let id = AssetId::generate();
	let mut catalog = AssetCatalog {
		schema_version: 1,
		assets: vec![PersistentAsset {
			id: id.clone(),
			path: "cube.glb".into(),
			aliases: vec!["original.glb".into()],
			revision: 1,
			kind: AssetKind::Model,
			fingerprint: fingerprint(&bytes),
			bytes: bytes.len() as u64,
			source_path: None,
			meshes: (0..2)
				.map(|index| MeshSignature {
					index,
					name: format!("Cube {index}"),
					primitives: 1,
					identity: None,
				})
				.collect(),
			dependencies: Vec::new(),
		}],
	};
	let catalog_path = scratch.0.join(CATALOG_FILE);
	fs::write(&catalog_path, catalog.to_json().unwrap()).unwrap();
	let alias = AssetRef::new("original.glb", None).unwrap();
	let managed = AssetRef::new("cube.glb", Some(0))
		.unwrap()
		.with_asset_id(Some(id.clone()));
	let second = AssetRef::new("cube.glb", Some(1))
		.unwrap()
		.with_asset_id(Some(id));

	assert!(PreparedModelBatch::prepare(&scratch.0, [&alias].into_iter(), &|| true).is_none());
	let cancel = Cell::new(false);
	let interrupted = [&alias, &second]
		.into_iter()
		.enumerate()
		.map(|(index, reference)| {
			cancel.set(index == 1);
			reference
		});
	assert!(PreparedModelBatch::prepare(&scratch.0, interrupted, &|| cancel.get()).is_none());

	let mut batch =
		PreparedModelBatch::prepare(&scratch.0, [&alias, &managed, &second].into_iter(), &|| {
			false
		})
		.unwrap();
	assert_eq!(batch.prepared_count(), 2);
	assert!((BATCH_CONTAINER_BYTES..MAX_BATCH_BYTES).contains(&batch.retained_bytes()));
	let take_current = |batch: &mut PreparedModelBatch, reference: &AssetRef| {
		let (resolved, current) = crate::assets::project_assets::resolve_asset_with_model(
			&scratch.0,
			reference,
			&mut None,
			|asset| CurrentModel::from(asset.unwrap()),
		)
		.unwrap();
		batch.take_current(&scratch.0, &resolved, &current, &model)
	};

	catalog.assets[0].revision = 2;
	fs::write(&catalog_path, catalog.to_json().unwrap()).unwrap();
	assert!(take_current(&mut batch, &managed).is_none());
	catalog.assets[0].revision = 1;
	fs::write(&catalog_path, catalog.to_json().unwrap()).unwrap();
	let mut different = bytes.clone();
	*different.last_mut().unwrap() ^= 1;
	fs::write(&model, &different).unwrap();
	assert!(take_current(&mut batch, &managed).is_none());
	assert!(PreparedModelBatch::prepare(&scratch.0, [&alias].into_iter(), &|| false).is_none());

	fs::write(&model, bytes).unwrap();
	let (_, result) = take_current(&mut batch, &managed).unwrap();
	assert_eq!(result.len(), 1);
	assert!(!result[0].mesh.indices().is_empty());
	assert_eq!(batch.consumed_count(), 1);
	assert!(take_current(&mut batch, &managed).is_none());
	let (_, result) = take_current(&mut batch, &second).unwrap();
	assert_eq!(result[0].mesh_index, 1);
}

#[test]
fn glb_with_an_external_image_defers_to_ordinary_loading() {
	let bytes = adapted_cube(|document| {
		document["images"] = serde_json::json!([{"uri": "paint.png"}]);
	});
	let digest = fingerprint(&bytes);
	let mut budget = ImportBudget::warming(4 * 1024 * 1024, &|| false);
	let Err(error) = load_warm_mesh(bytes, digest, 0, &mut budget) else {
		panic!("warming should reject an external image");
	};
	assert!(
		error
			.to_string()
			.contains("requires embedded GLB resources")
	);
}
