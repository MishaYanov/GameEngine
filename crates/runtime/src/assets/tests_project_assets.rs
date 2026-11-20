use super::*;

struct Fixture(PathBuf);
impl Fixture {
	fn new() -> Self {
		let path = std::env::temp_dir().join(format!(
			"engine-asset-tests-{}",
			AssetId::generate().as_str()
		));
		fs::create_dir(&path).unwrap();
		fs::create_dir(path.join("assets")).unwrap();
		Self(path)
	}
	fn root(&self) -> PathBuf {
		self.0.join("assets")
	}
	fn source(&self, name: &str, mesh_name: &str, extent: f32) -> PathBuf {
		let path = self.0.join(name);
		fs::write(&path, glb(mesh_name, extent)).unwrap();
		path
	}
}
impl Drop for Fixture {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.0);
	}
}
fn glb(name: &str, extent: f32) -> Vec<u8> {
	let json = serde_json::json!({"asset":{"version":"2.0"},"buffers":[{"byteLength":42}],"bufferViews":[{"buffer":0,"byteOffset":0,"byteLength":36},{"buffer":0,"byteOffset":36,"byteLength":6}],"accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[extent,extent,0]},{"bufferView":1,"componentType":5123,"count":3,"type":"SCALAR"}],"meshes":[{"name":name,"primitives":[{"attributes":{"POSITION":0},"indices":1,"mode":4}]}],"nodes":[{"mesh":0}],"scenes":[{"nodes":[0]}],"scene":0});
	let mut json = json.to_string().into_bytes();
	while !json.len().is_multiple_of(4) {
		json.push(b' ');
	}
	let mut data = Vec::new();
	for value in [0f32, 0., 0., extent, 0., 0., 0., extent, 0.] {
		data.extend_from_slice(&value.to_le_bytes());
	}
	for index in [0u16, 1, 2] {
		data.extend_from_slice(&index.to_le_bytes());
	}
	while !data.len().is_multiple_of(4) {
		data.push(0);
	}
	let mut bytes = b"glTF".to_vec();
	bytes.extend_from_slice(&2u32.to_le_bytes());
	bytes.extend_from_slice(&((28 + json.len() + data.len()) as u32).to_le_bytes());
	bytes.extend_from_slice(&(json.len() as u32).to_le_bytes());
	bytes.extend_from_slice(b"JSON");
	bytes.extend_from_slice(&json);
	bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
	bytes.extend_from_slice(b"BIN\0");
	bytes.extend_from_slice(&data);
	bytes
}
fn changed_json(bytes: &[u8], change: impl FnOnce(&mut serde_json::Value)) -> Vec<u8> {
	let size = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
	let mut value = serde_json::from_slice(&bytes[20..20 + size]).unwrap();
	change(&mut value);
	let mut json = serde_json::to_vec(&value).unwrap();
	while !json.len().is_multiple_of(4) {
		json.push(b' ');
	}
	let tail = &bytes[20 + size..];
	let mut result = bytes[..8].to_vec();
	result.extend_from_slice(&((20 + json.len() + tail.len()) as u32).to_le_bytes());
	result.extend_from_slice(&(json.len() as u32).to_le_bytes());
	result.extend_from_slice(b"JSON");
	result.extend_from_slice(&json);
	result.extend_from_slice(tail);
	result
}
#[test]
fn exclusive_publication_never_overwrites_an_occupied_target() {
	let f = Fixture::new();
	let source = f.0.join("source");
	let target = f.0.join("target");
	fs::write(&source, b"new").unwrap();
	fs::write(&target, b"original").unwrap();
	assert!(install_new(&source, &target).is_err());
	assert_eq!(fs::read(&target).unwrap(), b"original");
	assert_eq!(fs::read(&source).unwrap(), b"new");
}
#[test]
fn import_move_and_reopen_preserve_identity_and_legacy_aliases() {
	let f = Fixture::new();
	let source = f.source("source.glb", "Triangle", 1.);
	let root = f.root();
	let prepared = prepare_import(&root, &source, "Ä.glb").unwrap();
	let temp = prepared.candidate_path().to_path_buf();
	assert!(!root.join("Ä.glb").exists());
	let imported = prepared.publish().unwrap();
	assert!(!temp.exists());
	assert_eq!(
		fs::read(root.join("Ä.glb")).unwrap(),
		fs::read(&source).unwrap()
	);
	let moved = prepare_move(&root, &imported.id, "moved.glb")
		.unwrap()
		.publish()
		.unwrap();
	assert_eq!(moved.id, imported.id);
	assert!(!root.join("Ä.glb").exists());
	assert!(root.join("moved.glb").exists());
	assert_eq!(
		load_catalog(&root).unwrap().by_path("ä.glb").unwrap().id,
		imported.id
	);
	let reference = AssetRef::new("ä.glb", Some(0)).unwrap();
	let resolved = resolve_asset(&root, &reference).unwrap();
	assert_eq!(resolved.path, "moved.glb");
	assert_eq!(resolved.asset_id, Some(imported.id.clone()));
	assert!(prepare_import(&root, &source, "ä.glb").is_err());
}
#[test]
fn reimport_is_compatible_and_rolls_back_asset_and_catalog_on_publication_failure() {
	let f = Fixture::new();
	let source = f.source("source.glb", "Triangle", 1.);
	let root = f.root();
	let old = prepare_import(&root, &source, "model.glb")
		.unwrap()
		.publish()
		.unwrap();
	let before = fs::read(root.join("model.glb")).unwrap();
	let catalog_before = fs::read(root.join(CATALOG_FILE)).unwrap();
	let incompatible = f.source("incompatible.glb", "Renamed", 2.);
	let used = HashSet::from([0]);
	assert!(
		prepare_reimport(&root, &old.id, Some(&incompatible), &old.fingerprint, &used).is_err()
	);
	assert_eq!(fs::read(root.join("model.glb")).unwrap(), before);
	let replacement = f.source("replacement.glb", "Triangle", 2.);
	let candidate =
		prepare_reimport(&root, &old.id, Some(&replacement), &old.fingerprint, &used).unwrap();
	let mut calls = 0;
	let error = candidate
		.publish_with(|source, target| {
			calls += 1;
			if calls == 2 {
				Err("injected catalog publication failure".into())
			} else {
				install_new(source, target)
			}
		})
		.unwrap_err();
	assert!(error.contains("injected"));
	assert_eq!(fs::read(root.join("model.glb")).unwrap(), before);
	assert_eq!(fs::read(root.join(CATALOG_FILE)).unwrap(), catalog_before);
	let next = prepare_reimport(&root, &old.id, Some(&replacement), &old.fingerprint, &used)
		.unwrap()
		.publish()
		.unwrap();
	assert_eq!(next.id, old.id);
	assert_eq!(next.revision, 2);
	assert_ne!(next.fingerprint, old.fingerprint);
	assert_eq!(
		fs::read(root.join("model.glb")).unwrap(),
		fs::read(replacement).unwrap()
	);
}
#[test]
fn stale_candidate_conflicts_and_invalid_source_leave_authored_assets_untouched() {
	let f = Fixture::new();
	let source = f.source("source.glb", "Triangle", 1.);
	let root = f.root();
	let pending = prepare_import(&root, &source, "model.glb").unwrap();
	fs::write(root.join("model.glb"), b"concurrent writer").unwrap();
	assert!(pending.publish().is_err());
	assert_eq!(
		fs::read(root.join("model.glb")).unwrap(),
		b"concurrent writer"
	);
	assert!(!root.join(CATALOG_FILE).exists());
	fs::remove_file(root.join("model.glb")).unwrap();
	fs::write(&source, b"not a GLB").unwrap();
	assert!(prepare_import(&root, &source, "model.glb").is_err());
	assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
	let source = f.source("valid.glb", "Triangle", 1.);
	let first = prepare_import(&root, &source, "one.glb").unwrap();
	let abandoned = first.candidate_path().to_owned();
	drop(first);
	assert!(!abandoned.exists());
	let stale = prepare_import(&root, &source, "one.glb").unwrap();
	prepare_import(&root, &source, "two.glb")
		.unwrap()
		.publish()
		.unwrap();
	assert!(stale.publish().is_err());
	assert!(!root.join("one.glb").exists());
	assert!(root.join("two.glb").exists());
}
#[test]
fn migration_is_repeatable_and_prefab_catalog_retains_dependencies() {
	let f = Fixture::new();
	let root = f.root();
	fs::write(root.join("legacy.glb"), glb("Triangle", 1.)).unwrap();
	let first = migrate_catalog(&root).unwrap();
	let id = first.by_path("legacy.glb").unwrap().id.clone();
	assert_eq!(migrate_catalog(&root).unwrap(), first);
	let prefab = engine_core::editor::PrefabData {
		reference_slots: Default::default(),
		schema_version: 2,
		entities: vec![
			engine_core::scene_data::EntityData::new(
				engine_core::EntityKey::new("prefab-root").unwrap(),
				"Triangle",
				"legacy.glb",
			)
			.with_asset_id(Some(id.clone())),
		],
		gameplay: None,
	};
	let record = prepare_prefab(&root, "assembly.json", &prefab)
		.unwrap()
		.publish()
		.unwrap();
	assert_eq!(record.kind, AssetKind::Prefab);
	assert_eq!(record.dependencies, vec![id]);
	assert_eq!(
		migrate_catalog(&root)
			.unwrap()
			.by_path("assembly.json")
			.unwrap(),
		&record
	);
	let moved = prepare_move(&root, &record.id, "renamed-assembly.json")
		.unwrap()
		.publish()
		.unwrap();
	assert_eq!(moved.id, record.id);
	assert_eq!(moved.kind, AssetKind::Prefab);
	assert_eq!(moved.dependencies, record.dependencies);
	assert_eq!(moved.aliases, vec!["assembly.json"]);
	assert!(!root.join("assembly.json").exists());
	let reopened = load_catalog(&root).unwrap();
	assert_eq!(
		reopened.by_path("assembly.json").unwrap().path,
		"renamed-assembly.json"
	);
}
#[test]
fn external_glb_validation_checks_unused_images_geometry_dependencies_and_destination_without_publication()
 {
	let f = Fixture::new();
	let root = f.root();
	let valid = glb("Triangle", 1.);
	let external = changed_json(&valid, |value| {
		value["buffers"][0]["uri"] = serde_json::json!("../outside.bin")
	});
	let unused_bad_image = changed_json(&valid, |value| {
		value["images"] = serde_json::json!([{"bufferView":0,"mimeType":"image/png"}]);
	});
	let mut nonfinite = valid.clone();
	let json_size = u32::from_le_bytes(nonfinite[12..16].try_into().unwrap()) as usize;
	nonfinite[28 + json_size..32 + json_size].copy_from_slice(&f32::NAN.to_le_bytes());
	for (name, bytes) in [
		("external.glb", external),
		("image.glb", unused_bad_image),
		("geometry.glb", nonfinite),
	] {
		let source = f.0.join(name);
		fs::write(&source, bytes).unwrap();
		assert!(
			prepare_import(&root, &source, "model.glb").is_err(),
			"accepted {name}"
		);
		assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
	}
	let source = f.source("valid.glb", "Triangle", 1.);
	for destination in [
		"../outside.glb",
		"/absolute.glb",
		"wrong.json",
		"missing/model.glb",
	] {
		assert!(prepare_import(&root, &source, destination).is_err());
	}
	assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
}

#[test]
fn migration_charges_unrecognized_and_malformed_json_before_skipping_it() {
	let f = Fixture::new();
	let root = f.root();
	for (name, bytes) in [
		("ordinary.json", b"{}".as_slice()),
		("malformed.json", b"no".as_slice()),
		("other.json", b"[]".as_slice()),
	] {
		fs::write(root.join(name), bytes).unwrap();
	}
	let error = migrate_catalog_with_budget(&root, 5).unwrap_err();
	assert!(error.contains("source budget"));
	assert!(!root.join(CATALOG_FILE).exists());
	assert_eq!(fs::read_dir(&root).unwrap().count(), 3);
	assert_eq!(
		migrate_catalog_with_budget(&root, 6).unwrap(),
		AssetCatalog::default()
	);
}
