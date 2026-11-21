use super::*;
use std::path::PathBuf;
fn fixture() -> PathBuf {
	let root = std::env::temp_dir().join(format!(
		"runtime-prefab-{}",
		engine_core::asset_catalog::AssetId::generate().as_str()
	));
	fs::create_dir(&root).unwrap();
	root
}
#[test]
fn shared_loader_hashes_the_same_validated_bytes_and_rejects_escaping_or_wrong_kind() {
	let root = fixture();
	let source = r#"{"schema_version":1,"entities":[{"key":"prop","name":"Prop","model_path":"props/cube.glb","transform":{"position":[0,0,0],"rotation":[0,0,0],"scale":[1,1,1]}}]}"#;
	fs::write(root.join("item.json"), source).unwrap();
	let (prefab, fingerprint) = load(&root, "item.json").unwrap();
	assert_eq!(prefab.entities[0].key.as_str(), "prop");
	assert_eq!(
		fingerprint,
		format!("{:x}", Sha256::digest(source.as_bytes()))
	);
	for path in ["../item.json", "/item.json", "item.glb", "missing.json"] {
		assert!(load(&root, path).is_err());
	}
	fs::write(root.join("invalid.json"), "{}").unwrap();
	assert!(load(&root, "invalid.json").is_err());
	assert!(load(&root, ".").is_err());
	fs::remove_dir_all(root).unwrap();
}
#[test]
fn shared_loader_rejects_oversized_sources_before_parsing() {
	let root = fixture();
	let file = fs::File::create(root.join("large.json")).unwrap();
	file.set_len(PrefabData::MAX_JSON_BYTES as u64 + 1).unwrap();
	drop(file);
	assert!(
		load(&root, "large.json")
			.unwrap_err()
			.contains("byte limit")
	);
	fs::remove_dir_all(root).unwrap();
}
#[test]
fn shared_loader_resolves_reserved_prefab_move_aliases_without_retargeting_identity() {
	let root = fixture();
	let source = r#"{"schema_version":1,"entities":[{"key":"prop","name":"Prop","model_path":"props/cube.glb","transform":{"position":[0,0,0],"rotation":[0,0,0],"scale":[1,1,1]}}]}"#;
	fs::write(root.join("moved.json"), source).unwrap();
	let asset = engine_core::asset_catalog::PersistentAsset {
		id: engine_core::asset_catalog::AssetId::generate(),
		path: "moved.json".into(),
		kind: AssetKind::Prefab,
		aliases: vec!["original.json".into()],
		revision: 2,
		fingerprint: format!("{:x}", Sha256::digest(source.as_bytes())),
		bytes: source.len() as u64,
		source_path: None,
		meshes: vec![],
		dependencies: vec![],
	};
	let catalog = AssetCatalog {
		schema_version: 1,
		assets: vec![asset],
	};
	fs::write(root.join(CATALOG_FILE), catalog.to_json().unwrap()).unwrap();
	let (prefab, hash) = load(&root, "ORIGINAL.json").unwrap();
	assert_eq!(prefab.entities[0].key.as_str(), "prop");
	assert_eq!(hash, format!("{:x}", Sha256::digest(source.as_bytes())));
	assert!(!root.join("original.json").exists());
	fs::remove_dir_all(root).unwrap();
}
#[cfg(unix)]
#[test]
fn shared_loader_rejects_symbolic_links_even_when_the_target_is_contained() {
	let root = fixture();
	fs::write(root.join("data.json"), "{}").unwrap();
	std::os::unix::fs::symlink(root.join("data.json"), root.join("linked.json")).unwrap();
	assert!(load(&root, "linked.json").unwrap_err().contains("symbolic"));
	fs::remove_dir_all(root).unwrap();
}
#[cfg(windows)]
#[test]
#[ignore = "requires permission to create a temporary Windows directory junction"]
fn shared_loader_rejects_directory_junctions_before_following_their_prefab() {
	use std::os::windows::process::CommandExt;
	let root = fixture();
	let target = root.join("actual");
	let junction = root.join("linked");
	fs::create_dir(&target).unwrap();
	fs::write(target.join("item.json"), "{}").unwrap();
	let output = std::process::Command::new("cmd")
		.args(["/C", "mklink", "/J"])
		.arg(&junction)
		.arg(&target)
		.creation_flags(0x08000000)
		.output()
		.unwrap();
	assert!(
		output.status.success(),
		"{}",
		String::from_utf8_lossy(&output.stderr)
	);
	assert!(
		load(&root, "linked/item.json")
			.unwrap_err()
			.contains("symbolic")
	);
	// Remove the junction itself before deleting the exclusively created fixture.
	fs::remove_dir(&junction).unwrap();
	assert_eq!(
		root.canonicalize().unwrap().parent(),
		Some(std::env::temp_dir().canonicalize().unwrap().as_path())
	);
	fs::remove_dir_all(root).unwrap();
}
