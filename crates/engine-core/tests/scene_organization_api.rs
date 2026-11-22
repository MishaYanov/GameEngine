mod support;

use engine_core::{
	SceneData,
	scene_data::{
		CURRENT_SCHEMA_VERSION, FolderId, FolderRecord, MAX_FOLDER_DEPTH, MAX_FOLDER_NAME_BYTES,
		MAX_SCENE_FOLDERS, SceneOrganization, parse_json,
	},
	world::RuntimeWorld,
};
use serde_json::{Value, json};
use std::sync::Arc;
use support::{entity, key, scene};

fn id(value: &str) -> FolderId {
	FolderId::new(value).unwrap()
}

fn folder(id_value: &str, name: &str, parent: Option<&str>) -> FolderRecord {
	FolderRecord {
		id: id(id_value),
		name: name.into(),
		parent: parent.map(id),
	}
}

fn organized_scene() -> SceneData {
	let mut scene = scene(vec![
		entity("floor1", [2.0, 0.0, 0.0]),
		entity("floor2", [3.0, 0.0, 2.0]),
		entity("support", [1.0, 0.0, 0.0]).with_parent(Some(key("floor1"))),
	]);
	scene.organization = SceneOrganization {
		folders: vec![
			folder("floor", "Floor", None),
			folder("spares", "Spare tiles", Some("floor")),
			folder("environment", "Environment", None),
		],
		root_membership: [(key("floor1"), id("floor")), (key("floor2"), id("floor"))].into(),
	};
	scene
}

#[test]
fn scene_organization_round_trips_in_disk_and_editor_formats() {
	let scene = organized_scene();
	let disk = scene.to_json().unwrap();
	assert_eq!(scene.serialized_size().unwrap(), disk.len());
	let canonical: Value = serde_json::from_str(&disk).unwrap();
	assert_eq!(canonical["schema_version"], CURRENT_SCHEMA_VERSION);
	assert_eq!(canonical["organization"]["folders"][0]["id"], "floor");
	assert_eq!(canonical["organization"]["folders"][1]["parent"], "floor");
	assert_eq!(
		canonical["organization"]["folders"][2]["name"],
		"Environment"
	);
	assert_eq!(
		canonical["organization"]["root_membership"]["floor2"],
		"floor"
	);
	assert!(canonical["entities"][0].get("components").is_some());
	assert_eq!(parse_json(&disk).unwrap().data, scene);

	let flat = serde_json::to_value(&scene).unwrap();
	assert!(flat["entities"][0].get("model_path").is_some());
	assert_eq!(serde_json::from_value::<SceneData>(flat).unwrap(), scene);
}

#[test]
fn folders_preserve_runtime_world_and_legacy_camera() {
	let scene = organized_scene();
	let mut legacy: Value = serde_json::from_str(&scene.to_json().unwrap()).unwrap();
	legacy["schema_version"] = json!(5);
	legacy.as_object_mut().unwrap().remove("organization");
	let migrated = parse_json(&legacy.to_string()).unwrap();
	assert_eq!(migrated.source_schema_version, 5);
	assert!(migrated.data.organization.is_empty());
	assert_eq!(migrated.data.camera, scene.camera);
	assert_eq!(
		&migrated.data.entities[..scene.entities.len()],
		scene.entities
	);
	assert_eq!(migrated.data.entities.len(), scene.entities.len() + 1);
	let left = RuntimeWorld::new(Arc::new(migrated.data)).unwrap();
	let right = RuntimeWorld::new(Arc::new(scene)).unwrap();
	assert_eq!(
		&left.world_matrices()[..right.world_matrices().len()],
		right.world_matrices()
	);
	assert!(!left.data().to_json().unwrap().contains("\"organization\""));

	legacy["organization"] = json!({});
	assert!(
		parse_json(&legacy.to_string())
			.unwrap_err()
			.contains("requires scene schema 6")
	);
}

#[test]
fn invalid_folder_references_and_memberships_are_rejected() {
	let scene = organized_scene();
	let invalid = |edit: fn(&mut SceneOrganization), expected: &str| {
		let mut candidate = scene.clone();
		edit(&mut candidate.organization);
		assert!(candidate.validate().unwrap_err().contains(expected));
		assert!(candidate.to_json().is_err());
	};
	invalid(
		|value| value.folders.push(folder("floor", "Duplicate", None)),
		"Duplicate folder ID",
	);
	invalid(
		|value| value.folders[0].parent = Some(id("unavailable")),
		"missing parent folder",
	);
	invalid(
		|value| value.folders[0].parent = Some(id("spares")),
		"parent cycle",
	);
	invalid(
		|value| {
			value.root_membership.insert(key("floor1"), id("missing"));
		},
		"missing folder",
	);
	invalid(
		|value| {
			value.root_membership.insert(key("support"), id("floor"));
		},
		"scene-root object",
	);
	invalid(
		|value| {
			value.root_membership.insert(key("deleted"), id("floor"));
		},
		"scene-root object",
	);
	assert!(serde_json::from_str::<FolderId>(r#""bad/name""#).is_err());
	let mut wire: Value = serde_json::from_str(&scene.to_json().unwrap()).unwrap();
	wire["organization"]["folders"][0]["parent"] = json!("spares");
	assert!(
		SceneData::from_json(&wire.to_string())
			.unwrap_err()
			.contains("parent cycle")
	);
}

#[test]
fn scene_folder_count_depth_and_name_are_bounded() {
	let mut data = scene(Vec::new());
	for i in 0..MAX_FOLDER_DEPTH {
		let parent = i.checked_sub(1).map(|parent| parent.to_string());
		data.organization
			.folders
			.push(folder(&i.to_string(), "Folder", parent.as_deref()));
	}
	data.validate().unwrap();
	data.organization.folders.push(folder(
		"too-deep",
		"Folder",
		Some(&(MAX_FOLDER_DEPTH - 1).to_string()),
	));
	assert!(
		data.validate()
			.unwrap_err()
			.contains("nesting exceeds depth")
	);

	data.organization.folders = (0..=MAX_SCENE_FOLDERS)
		.map(|i| folder(&i.to_string(), "Folder", None))
		.collect();
	assert!(data.validate().unwrap_err().contains("supports at most"));
	assert!(FolderRecord::validate_name(&"é".repeat(MAX_FOLDER_NAME_BYTES / 2)).is_ok());
	assert!(FolderRecord::validate_name(&"é".repeat(MAX_FOLDER_NAME_BYTES / 2 + 1)).is_err());
	assert!(FolderRecord::validate_name(" leading space").is_err());
	assert!(FolderRecord::validate_name("control\u{3}code").is_err());
}
