use super::{SceneCatalog, SceneDescriptor, SceneDescriptorErrors as CatalogError, SceneId};
use std::collections::HashSet;

#[test]
fn default_scene_catalog_contains_no_descriptors() {
	assert!(SceneCatalog::default().scenes.is_empty());
}

#[test]
fn scene_ids_preserve_authored_values_including_unicode() {
	for value in ["main", "scene-1", "scene_2", "Scene:Intro", "סצנה", "🎬"] {
		assert_eq!(SceneId::new(value).unwrap().as_str(), value);
		assert_eq!(SceneId::new(value.to_owned()).unwrap().as_str(), value);
	}
}

#[test]
fn scene_ids_reject_empty_values_whitespace_and_control_characters_without_trimming() {
	for value in [
		"",
		" ",
		" leading",
		"trailing ",
		"two words",
		"tab\tname",
		"line\nname",
		"return\rname",
		"nonbreaking\u{a0}space",
		"unicode\u{2003}space",
		"line\u{2028}separator",
		"null\0name",
		"escape\u{1b}name",
		"delete\u{7f}name",
		"control\u{85}name",
	] {
		assert!(SceneId::new(value).is_err(), "accepted scene ID {value:?}");
	}
}

#[test]
fn cloned_and_reconstructed_ids_have_equal_hash_identity_but_distinct_case_is_preserved() {
	let original = SceneId::new("Main").unwrap();
	let mut ids = HashSet::new();
	assert!(ids.insert(original.clone()));
	assert!(!ids.insert(original));
	assert!(!ids.insert(SceneId::new("Main").unwrap()));
	assert!(ids.insert(SceneId::new("main").unwrap()));
	assert_eq!(ids.len(), 2);
}

#[test]
fn generated_ids_are_distinct_valid_uuid_v4_values() {
	let mut ids = HashSet::new();
	for _ in 0..32 {
		let id = SceneId::generate();
		let uuid = uuid::Uuid::parse_str(id.as_str()).unwrap();
		assert_eq!(uuid.get_version_num(), 4);
		assert_eq!(uuid.get_variant(), uuid::Variant::RFC4122);
		assert_eq!(SceneId::new(id.as_str()).unwrap(), id);
		assert!(ids.insert(id));
	}
}

#[test]
fn descriptors_preserve_scene_identity_and_valid_asset_relative_json_paths() {
	let id = SceneId::new("main").unwrap();
	for path in [
		"main.json",
		"scenes/main.json",
		"scenes/chapter.1/intro.JSON",
		"scenes/Intro.JsOn",
		"scenes/My Scene.json",
		"scenes/סצנה.json",
	] {
		let descriptor = SceneDescriptor::new(id.clone(), path.to_owned()).unwrap();
		assert_eq!(descriptor.id(), &id);
		assert_eq!(descriptor.asset_path(), path);
	}
}

#[test]
fn descriptors_reject_unsafe_or_malformed_asset_paths() {
	for path in [
		"",
		" ",
		"/main.json",
		"//server/share/main.json",
		"C:/main.json",
		"C:main.json",
		"scenes\\main.json",
		"../main.json",
		"scenes/../main.json",
		"./main.json",
		"scenes/./main.json",
		"scenes//main.json",
		"scenes/main.json/",
		"scenes/main\0.json",
		"scenes/main\n.json",
		"scenes/main\u{7f}.json",
	] {
		let error = SceneDescriptor::new(SceneId::new("main").unwrap(), path)
			.expect_err("unsafe scene path was accepted");
		assert!(error.starts_with("Scene path:"), "{path:?}: {error}");
	}
}

#[test]
fn descriptors_require_a_json_extension_at_the_end_of_the_path() {
	for path in [
		"scenes/main",
		"scenes/main.gltf",
		"scenes/main.json.bak",
		"scenes/main.json ",
		"scenes/main.json?version=1",
		"scenes/main.json#fragment",
		"scenes/json",
	] {
		let error = SceneDescriptor::new(SceneId::new("main").unwrap(), path)
			.expect_err("non-JSON scene path was accepted");
		assert!(error.contains("JSON scene"), "{path:?}: {error}");
	}
}

#[test]
fn scene_path_length_limit_is_4096_bytes_including_utf8_and_extension() {
	for path in [
		format!("{}.json", "a".repeat(4091)),
		format!("{}a.json", "é".repeat(2045)),
	] {
		assert_eq!(path.len(), 4096);
		let descriptor = SceneDescriptor::new(SceneId::new("main").unwrap(), &path).unwrap();
		assert_eq!(descriptor.asset_path(), path);
		let oversized = format!("a{path}");
		assert!(SceneDescriptor::new(SceneId::new("main").unwrap(), oversized).is_err());
	}
}

#[test]
fn updating_a_descriptor_changes_only_the_asset_path_and_accepts_a_noop() {
	let id = SceneId::new("persistent-id").unwrap();
	let mut descriptor = SceneDescriptor::new(id.clone(), "scenes/original.json").unwrap();
	for path in [
		"scenes/renamed.JSON",
		"scenes/renamed.JSON",
		"other/final.json",
	] {
		descriptor.set_asset_path(path.to_owned()).unwrap();
		assert_eq!(descriptor.asset_path(), path);
		assert_eq!(descriptor.id(), &id);
	}
}

#[test]
fn rejected_path_updates_preserve_identity_and_previous_path_and_allow_retry() {
	let id = SceneId::new("persistent-id").unwrap();
	let mut descriptor = SceneDescriptor::new(id.clone(), "scenes/original.json").unwrap();
	for path in [
		String::new(),
		"../other.json".into(),
		"C:/other.json".into(),
		"scenes\\other.json".into(),
		"other.gltf".into(),
		"other.json\0".into(),
		format!("{}.json", "a".repeat(4092)),
	] {
		assert!(descriptor.set_asset_path(path).is_err());
		assert_eq!(descriptor.id(), &id);
		assert_eq!(descriptor.asset_path(), "scenes/original.json");
	}
	assert!(descriptor.set_asset_path("scenes/retry.json").is_ok());
	assert_eq!(descriptor.id(), &id);
	assert_eq!(descriptor.asset_path(), "scenes/retry.json");
}

fn catalog_entries(catalog: &SceneCatalog) -> HashSet<(SceneId, String)> {
	catalog
		.iter()
		.map(|scene| (scene.id().clone(), scene.asset_path().to_owned()))
		.collect()
}

#[test]
fn catalog_registration_preserves_authored_and_generated_ids_and_borrows_descriptors() {
	let mut catalog = SceneCatalog::default();
	let authored = SceneId::new("intro").unwrap();
	let generated = SceneId::generate();
	catalog
		.insert(SceneDescriptor::new(authored.clone(), "scenes/intro.json").unwrap())
		.unwrap();
	catalog
		.insert(SceneDescriptor::new(generated.clone(), "scenes/game.json").unwrap())
		.unwrap();
	assert_eq!(catalog.get(&authored).unwrap().id(), &authored);
	assert_eq!(
		catalog.get(&generated).unwrap().asset_path(),
		"scenes/game.json"
	);
	let borrowed: Vec<&SceneDescriptor> = catalog.iter().collect();
	assert_eq!(borrowed.len(), 2);
	assert_eq!(
		catalog_entries(&catalog),
		HashSet::from([
			(authored, "scenes/intro.json".into()),
			(generated, "scenes/game.json".into()),
		])
	);
}

#[test]
fn duplicate_registration_rejects_ids_and_paths_without_replacing_existing_entries() {
	let mut catalog = SceneCatalog::default();
	let id = SceneId::new("main").unwrap();
	catalog
		.insert(SceneDescriptor::new(id.clone(), "scenes/main.json").unwrap())
		.unwrap();
	let before = catalog_entries(&catalog);
	assert_eq!(
		catalog.insert(SceneDescriptor::new(id.clone(), "scenes/replacement.json").unwrap()),
		Err(CatalogError::DuplicateSceneId)
	);
	assert_eq!(catalog_entries(&catalog), before);
	let other_id = SceneId::new("other").unwrap();
	assert_eq!(
		catalog.insert(SceneDescriptor::new(other_id.clone(), "scenes/main.json").unwrap()),
		Err(CatalogError::DuplicateScenePath)
	);
	assert_eq!(catalog_entries(&catalog), before);
	assert_eq!(
		catalog.get(&other_id).unwrap_err(),
		CatalogError::SceneNotFound
	);
}

#[test]
fn missing_catalog_entries_report_not_found_without_changing_existing_descriptors() {
	let mut catalog = SceneCatalog::default();
	let present = SceneId::new("present").unwrap();
	let absent = SceneId::new("absent").unwrap();
	catalog
		.insert(SceneDescriptor::new(present, "scenes/present.json").unwrap())
		.unwrap();
	let before = catalog_entries(&catalog);
	assert_eq!(
		catalog.get(&absent).unwrap_err(),
		CatalogError::SceneNotFound
	);
	assert_eq!(
		catalog.change_path(&absent, "scenes/new.json"),
		Err(CatalogError::SceneNotFound)
	);
	assert_eq!(catalog.remove(&absent), Err(CatalogError::SceneNotFound));
	assert_eq!(catalog_entries(&catalog), before);
}

#[test]
fn catalog_path_changes_preserve_identity_and_reject_invalid_or_conflicting_paths_atomically() {
	let mut catalog = SceneCatalog::default();
	let main = SceneId::new("main").unwrap();
	let other = SceneId::new("other").unwrap();
	catalog
		.insert(SceneDescriptor::new(main.clone(), "scenes/main.json").unwrap())
		.unwrap();
	catalog
		.insert(SceneDescriptor::new(other, "scenes/other.json").unwrap())
		.unwrap();
	let before = catalog_entries(&catalog);
	for invalid in [
		"",
		"../outside.json",
		"C:/outside.json",
		"scenes\\main.json",
		"scenes/main.glb",
	] {
		let error = catalog.change_path(&main, invalid).unwrap_err();
		assert!(
			matches!(error, CatalogError::InvalidScenePath(_)),
			"{error:?}"
		);
		assert!(error.to_string().starts_with("Scene path"));
		assert_eq!(catalog_entries(&catalog), before);
	}
	assert_eq!(
		catalog.change_path(&main, "scenes/other.json"),
		Err(CatalogError::DuplicateScenePath)
	);
	assert_eq!(catalog_entries(&catalog), before);
	catalog.change_path(&main, "scenes/main.json").unwrap();
	assert_eq!(catalog_entries(&catalog), before);
	catalog.change_path(&main, "scenes/renamed.JSON").unwrap();
	assert_eq!(catalog.get(&main).unwrap().id(), &main);
	assert_eq!(
		catalog.get(&main).unwrap().asset_path(),
		"scenes/renamed.JSON"
	);
	assert_eq!(catalog.iter().count(), 2);
}

#[test]
fn removal_preserves_other_scenes_and_releases_the_removed_path() {
	let mut catalog = SceneCatalog::default();
	let main = SceneId::new("main").unwrap();
	let other = SceneId::new("other").unwrap();
	catalog
		.insert(SceneDescriptor::new(main.clone(), "scenes/main.json").unwrap())
		.unwrap();
	catalog
		.insert(SceneDescriptor::new(other.clone(), "scenes/other.json").unwrap())
		.unwrap();
	catalog.remove(&main).unwrap();
	assert_eq!(catalog.get(&main).unwrap_err(), CatalogError::SceneNotFound);
	assert_eq!(
		catalog.get(&other).unwrap().asset_path(),
		"scenes/other.json"
	);
	let after_remove = catalog_entries(&catalog);
	assert_eq!(catalog.remove(&main), Err(CatalogError::SceneNotFound));
	assert_eq!(catalog_entries(&catalog), after_remove);
	let replacement = SceneId::new("replacement").unwrap();
	catalog
		.insert(SceneDescriptor::new(replacement.clone(), "scenes/main.json").unwrap())
		.unwrap();
	assert_eq!(
		catalog.get(&replacement).unwrap().asset_path(),
		"scenes/main.json"
	);
	assert_eq!(catalog.iter().count(), 2);
}

#[test]
fn scene_id_byte_limit_is_enforced_by_rust_and_json_constructors() {
	for value in ["a".repeat(256), "é".repeat(128)] {
		let id = SceneId::new(&value).unwrap();
		assert_eq!(id.as_str().len(), 256);
		let encoded = serde_json::to_string(&id).unwrap();
		assert_eq!(serde_json::from_str::<SceneId>(&encoded).unwrap(), id);
		let oversized = format!("{value}a");
		assert!(SceneId::new(&oversized).is_err());
		assert!(serde_json::from_value::<SceneId>(serde_json::json!(oversized)).is_err());
	}
	for value in [
		serde_json::json!(null),
		serde_json::json!(42),
		serde_json::json!("two words"),
		serde_json::json!("bad\u{7f}"),
	] {
		assert!(serde_json::from_value::<SceneId>(value).is_err());
	}
}

#[test]
fn descriptor_json_roundtrips_and_rejects_invalid_paths_ids_and_unknown_fields() {
	let descriptor =
		SceneDescriptor::new(SceneId::new("intro").unwrap(), "scenes/intro.JSON").unwrap();
	let encoded = serde_json::to_value(&descriptor).unwrap();
	assert_eq!(
		encoded,
		serde_json::json!({"id":"intro","asset_path":"scenes/intro.JSON"})
	);
	assert_eq!(
		serde_json::from_value::<SceneDescriptor>(encoded).unwrap(),
		descriptor
	);
	for source in [
		r#"{"id":"intro","asset_path":"../intro.json"}"#,
		r#"{"id":"intro","asset_path":"intro.glb"}"#,
		r#"{"id":"two words","asset_path":"intro.json"}"#,
		r#"{"id":"intro"}"#,
		r#"{"asset_path":"intro.json"}"#,
		r#"{"id":"intro","asset_path":"intro.json","unknown":true}"#,
		r#"{"id":"intro","id":"other","asset_path":"intro.json"}"#,
	] {
		assert!(
			serde_json::from_str::<SceneDescriptor>(source).is_err(),
			"accepted {source}"
		);
	}
}

#[test]
fn catalog_json_is_sorted_by_id_independent_of_insertion_order_and_roundtrips() {
	let mut forward = SceneCatalog::default();
	let mut reversed = SceneCatalog::default();
	for id in ["zeta", "alpha", "middle"] {
		forward
			.insert(
				SceneDescriptor::new(SceneId::new(id).unwrap(), format!("scenes/{id}.json"))
					.unwrap(),
			)
			.unwrap();
	}
	for descriptor in ["middle", "alpha", "zeta"] {
		reversed
			.insert(
				forward
					.get(&SceneId::new(descriptor).unwrap())
					.unwrap()
					.clone(),
			)
			.unwrap();
	}
	let encoded = serde_json::to_string(&forward).unwrap();
	assert_eq!(encoded, serde_json::to_string(&reversed).unwrap());
	assert_eq!(
		serde_json::from_str::<SceneCatalog>(&encoded).unwrap(),
		forward
	);
	let entries: serde_json::Value = serde_json::from_str(&encoded).unwrap();
	assert_eq!(
		entries
			.as_array()
			.unwrap()
			.iter()
			.map(|entry| entry["id"].as_str().unwrap())
			.collect::<Vec<_>>(),
		["alpha", "middle", "zeta"]
	);
	let mut cloned = forward.clone();
	cloned
		.change_path(&SceneId::new("alpha").unwrap(), "scenes/renamed.json")
		.unwrap();
	assert_eq!(serde_json::to_string(&forward).unwrap(), encoded);
}

#[test]
fn catalog_scene_limit_rejects_overflow_atomically_and_removal_restores_capacity() {
	let mut catalog = SceneCatalog::default();
	for index in 0..super::MAX_PROJECT_SCENES {
		catalog
			.insert(
				SceneDescriptor::new(
					SceneId::new(format!("scene-{index}")).unwrap(),
					format!("scenes/{index}.json"),
				)
					.unwrap(),
			)
			.unwrap();
	}
	let before = catalog.clone();
	let overflow =
		SceneDescriptor::new(SceneId::new("overflow").unwrap(), "scenes/overflow.json").unwrap();
	assert!(catalog.insert(overflow.clone()).is_err());
	assert_eq!(catalog, before);
	let mut encoded = serde_json::to_value(&catalog).unwrap();
	encoded
		.as_array_mut()
		.unwrap()
		.push(serde_json::to_value(&overflow).unwrap());
	assert!(serde_json::from_value::<SceneCatalog>(encoded).is_err());
	let valid = serde_json::to_string(&catalog).unwrap();
	assert_eq!(
		serde_json::from_str::<SceneCatalog>(&valid).unwrap(),
		catalog
	);
	catalog.remove(&SceneId::new("scene-0").unwrap()).unwrap();
	catalog.insert(overflow).unwrap();
	assert_eq!(catalog.iter().count(), super::MAX_PROJECT_SCENES);
}

#[test]
fn catalog_json_rejects_duplicate_ids_paths_and_non_array_shapes() {
	for source in [
		r#"[{"id":"same","asset_path":"one.json"},{"id":"same","asset_path":"two.json"}]"#,
		r#"[{"id":"one","asset_path":"same.json"},{"id":"two","asset_path":"same.json"}]"#,
		r#"{"scenes":[]}"#,
		"null",
		"[42]",
	] {
		assert!(
			serde_json::from_str::<SceneCatalog>(source).is_err(),
			"accepted {source}"
		);
	}
	assert_eq!(
		serde_json::from_str::<SceneCatalog>("[]").unwrap(),
		SceneCatalog::default()
	);
}
