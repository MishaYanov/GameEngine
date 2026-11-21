use super::*;
#[test]
fn catalog_dto_carries_stable_identity_and_explicit_optional_metadata_for_models_and_prefabs() {
	let mut asset = runtime::asset_catalog::PersistentAsset {
		id: runtime::asset_catalog::AssetId::generate(),
		path: "model.glb".into(),
		kind: runtime::asset_catalog::AssetKind::Model,
		aliases: vec![],
		revision: 1,
		fingerprint: "a".repeat(64),
		bytes: 128,
		source_path: None,
		meshes: vec![runtime::asset_catalog::MeshSignature {
			index: 0,
			name: "Mesh".into(),
			primitives: 1,
		}],
		dependencies: vec![],
	};
	let model = serde_json::to_value(ModelAssetDto::from_asset(&asset)).unwrap();
	assert_eq!(model["asset_id"], asset.id.as_str());
	assert_eq!(model["kind"], "model");
	assert_eq!(model["fingerprint"], asset.fingerprint);
	assert_eq!(model["revision"], 1);
	assert_eq!(model["mesh_count"], 1);
	assert!(model["source_path"].is_null());
	asset.path = "assembly.json".into();
	asset.kind = runtime::asset_catalog::AssetKind::Prefab;
	asset.meshes.clear();
	let prefab = serde_json::to_value(ModelAssetDto::from_asset(&asset)).unwrap();
	assert_eq!(prefab["kind"], "prefab");
	assert_eq!(prefab["asset_id"], asset.id.as_str());
	assert_eq!(prefab["path"], "assembly.json");
	assert!(prefab["mesh_count"].is_null());
	assert!(prefab["source_path"].is_null());
}
