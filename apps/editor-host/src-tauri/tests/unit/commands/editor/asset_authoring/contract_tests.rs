use super::*;
#[test]
fn operation_result_and_progress_use_the_frontend_wire_field_names() {
	let scene = runtime::SceneData::from_json(include_str!(concat!(
		env!("CARGO_MANIFEST_DIR"),
		"/../../game/assets/scenes/robots.json"
	)))
	.unwrap();
	let project =
		runtime::project::ProjectData::for_scene("Asset DTO", "scenes/main.json", &scene).unwrap();
	let snapshot = runtime::Engine::new(scene, project)
		.unwrap()
		.snapshot()
		.unwrap();
	let revision = snapshot.revision;
	let asset = runtime::asset_catalog::PersistentAsset {
		id: AssetId::generate(),
		path: "assembly.json".into(),
		kind: runtime::asset_catalog::AssetKind::Prefab,
		aliases: vec![],
		revision: 1,
		fingerprint: "a".repeat(64),
		bytes: 128,
		source_path: None,
		meshes: vec![],
		dependencies: vec![],
	};
	let value = serde_json::to_value(AssetOperationDto {
		operation_id: "operation-1".into(),
		asset: ModelAssetDto::from_asset(&asset),
		snapshot,
	})
	.unwrap();
	assert_eq!(
		value
			.as_object()
			.unwrap()
			.keys()
			.map(String::as_str)
			.collect::<std::collections::BTreeSet<_>>(),
		std::collections::BTreeSet::from(["asset", "operation_id", "snapshot"])
	);
	assert_eq!(value["operation_id"], "operation-1");
	assert!(value.get("operationId").is_none());
	assert_eq!(value["snapshot"]["revision"], revision);
	assert_eq!(value["asset"]["asset_id"], asset.id.as_str());
	assert_eq!(value["asset"]["kind"], "prefab");
	assert!(value["asset"]["mesh_count"].is_null());
	let progress = serde_json::to_value(Progress {
		operation_id: "operation-1".into(),
		stage: "validating",
		completed: 0,
		total: 3,
		error: None,
	})
	.unwrap();
	assert_eq!(
		progress,
		serde_json::json!({"operation_id":"operation-1","stage":"validating","completed":0,"total":3,"error":null})
	);
}

#[test]
fn source_selections_are_confined_to_the_workspace_and_kind() {
	let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../game/assets");
	let mut first = EditorWorkspace::open(assets.to_str().unwrap(), "scenes/robots.json").unwrap();
	let second = EditorWorkspace::open(assets.to_str().unwrap(), "scenes/robots.json").unwrap();
	assert_eq!(
		first.backend.session().revision(),
		second.backend.session().revision()
	);
	assert!(check_asset_generation(&second, first.generation).is_err());
	let source = assets.join("sample.glb");
	assert!(first
		.asset_sources
		.require(AssetSourceKind::Glb, &source)
		.is_err());
	first
		.asset_sources
		.select(AssetSourceKind::Glb, source.clone());
	assert!(first
		.asset_sources
		.require(AssetSourceKind::Glb, &source)
		.is_ok());
	assert!(first
		.asset_sources
		.require(AssetSourceKind::Wav, &source)
		.is_err());
	assert!(second
		.asset_sources
		.require(AssetSourceKind::Glb, &source)
		.is_err());
}
