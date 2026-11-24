use super::*;
#[test]
fn catalog_dto_carries_stable_identity_and_format_kind() {
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
			identity: Some(runtime::asset_catalog::MeshIdentity {
				id: runtime::asset_catalog::MeshSubassetId::generate(),
				geometry_sha256: "a".repeat(64),
				source_key: None,
			}),
		}],
		dependencies: vec![],
	};
	let model = serde_json::to_value(ModelAssetDto::from_asset(&asset)).unwrap();
	assert_eq!(model["asset_id"], asset.id.as_str());
	assert_eq!(model["kind"], "model");
	assert_eq!(model["fingerprint"], asset.fingerprint);
	assert_eq!(model["revision"], 1);
	assert_eq!(model["mesh_count"], 1);
	assert_eq!(
		model["meshes"],
		serde_json::json!([{
			"id": asset.meshes[0].identity.as_ref().unwrap().id.as_str(),
			"index": 0,
			"name": "Mesh",
		}])
	);
	assert!(model["source_path"].is_null());
	asset.path = "assembly.json".into();
	asset.kind = runtime::asset_catalog::AssetKind::Prefab;
	asset.meshes.clear();
	let prefab = serde_json::to_value(ModelAssetDto::from_asset(&asset)).unwrap();
	assert_eq!(prefab["kind"], "prefab");
	assert_eq!(prefab["asset_id"], asset.id.as_str());
	assert_eq!(prefab["path"], "assembly.json");
	assert!(prefab["mesh_count"].is_null());
	assert!(prefab.get("meshes").is_none());
	assert!(prefab["source_path"].is_null());
	asset.path = "audio/bell.wav".into();
	asset.kind = runtime::asset_catalog::AssetKind::AudioClip;
	let audio = serde_json::to_value(ModelAssetDto::from_asset(&asset)).unwrap();
	assert_eq!(audio["kind"], "audio_clip");
	assert_eq!(audio["asset_id"], asset.id.as_str());
	assert_eq!(audio["path"], "audio/bell.wav");
	assert!(audio["mesh_count"].is_null());
}
