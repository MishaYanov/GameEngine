mod support;
use engine_core::project::ProjectData;
use support::*;

#[test]
fn unknown_native_module_payload_roundtrips_without_core_interpreting_it() {
	let mut project =
		ProjectData::for_scene("Settings", "scenes/main.json", &scene(vec![])).unwrap();
	project.gameplay_settings =
		Some(serde_json::json!({"module_specific": {"literal": "preserve me", "enabled": true}}));
	let encoded = serde_json::to_string(&project).unwrap();
	assert_eq!(ProjectData::from_json(&encoded).unwrap(), project);
	project.gameplay_settings = None;
	assert!(
		!serde_json::to_string(&project)
			.unwrap()
			.contains("gameplay_settings")
	);
}

#[test]
fn generic_payload_enforces_string_node_and_depth_limits_before_module_resolution() {
	let mut project =
		ProjectData::for_scene("Settings", "scenes/main.json", &scene(vec![])).unwrap();
	let mut nested = serde_json::Value::Null;
	for _ in 0..18 {
		nested = serde_json::json!([nested]);
	}
	for value in [
		serde_json::json!("x".repeat(256 * 1024 + 1)),
		serde_json::Value::Array(vec![serde_json::Value::Null; 16_384]),
		nested,
	] {
		project.gameplay_settings = Some(value);
		assert!(
			project
				.validate()
				.unwrap_err()
				.contains("Gameplay settings")
		);
	}
}

#[test]
fn prefab_scene_settings_share_the_project_payload_budget_before_clone_and_serialization() {
	use engine_core::{
		editor::{PrefabData, PrefabGameplay},
		scene_catalog::SceneId,
	};
	let mut nested = serde_json::Value::Null;
	for _ in 0..18 {
		nested = serde_json::json!([nested]);
	}
	let gameplay = PrefabGameplay {
		source_scene: SceneId::new("source").unwrap(),
		module: None,
		bindings: Default::default(),
		behaviors: vec![],
		definitions: vec![],
		scene_settings: None,
		entity_references: Default::default(),
		scene_references: Default::default(),
	};
	for value in [
		serde_json::json!("x".repeat(256 * 1024 + 1)),
		serde_json::Value::Array(vec![serde_json::Value::Null; 16_384]),
		nested,
	] {
		let mut prefab = PrefabData {
			composition: Default::default(),
			reference_slots: Default::default(),
			schema_version: 2,
			entities: vec![entity("actor", [0.; 3])],
			gameplay: Some(gameplay.clone()),
		};
		prefab.gameplay.as_mut().unwrap().scene_settings = Some(value.clone());
		assert!(
			engine_core::project::validate_gameplay_payload(&value)
				.unwrap_err()
				.contains("Gameplay settings")
		);
		assert!(prefab.validate().unwrap_err().contains("Gameplay settings"));
		assert!(prefab.to_json().is_err());
		let source = serde_json::to_string(&prefab).unwrap();
		assert!(
			PrefabData::from_json(&source)
				.unwrap_err()
				.contains("Gameplay settings")
		);
	}
	let mut valid = PrefabData {
		composition: Default::default(),
		reference_slots: Default::default(),
		schema_version: 2,
		entities: vec![entity("actor", [0.; 3])],
		gameplay: Some(gameplay),
	};
	valid.gameplay.as_mut().unwrap().scene_settings =
		Some(serde_json::json!({"custom_setting":"bounded"}));
	assert!(valid.to_json().is_ok());
}
