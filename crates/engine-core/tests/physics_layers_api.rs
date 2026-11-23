use engine_core::project::ProjectData;
use serde_json::{Value, json};

const LEGACY: &str = r#"{
    "schema_version": 3,
    "name": "Legacy",
    "scenes": [{"id": "room", "asset_path": "content/room.scene.json"}],
    "startup_scene_id": "room"
}"#;

#[test]
fn legacy_round_trip_and_named_highest_bit() {
	let mut original = ProjectData::from_json(LEGACY).unwrap();
	assert!(original.physics_layers.is_none());
	assert!(!original.to_json().unwrap().contains("physics_layers"));

	let mut source: Value = serde_json::from_str(LEGACY).unwrap();
	source["physics_layers"] = json!({
		"layers": [{ "id": "land", "bit": 31, "label": "Land" }],
		"ignored_pairs": [[0, 31], [31, 31]]
	});
	let mut project = ProjectData::from_json(&source.to_string()).unwrap();
	let policy = project.physics_layers.as_mut().unwrap();
	policy.layers[0].label = "Terrain".into();
	policy.layers[0].archived = true;
	original.physics_layers = project.physics_layers.clone();
	assert_eq!(
		ProjectData::from_json(&project.to_json().unwrap()).unwrap(),
		original
	);
	let saved = original.physics_layers.unwrap();
	assert_eq!(saved.layers[0].bit, 31);
	assert_eq!(saved.layers[0].id, "land");
	assert_eq!(saved.ignored_pairs, [[0, 31], [31, 31]]);
}

#[test]
fn rejects_invalid_catalogs_and_pair_records() {
	for (policy, expected) in [
		(json!({ "ignored_pairs": [[31, 0]] }), "ascending bits"),
		(json!({ "ignored_pairs": [[0, 32]] }), "ascending bits"),
		(
			json!({ "ignored_pairs": [[2, 3], [2, 3]] }),
			"Duplicate ignored physics pair",
		),
		(
			json!({ "layers": [
				{"bit": 1, "id": "terrain", "label": "Terrain", "archived": true},
				{"bit": 1, "id": "world", "label": "World"}
			]}),
			"Duplicate physics layer bit",
		),
		(
			json!({ "layers": [
				{"bit": 1, "id": "terrain", "label": "Terrain"},
				{"bit": 2, "id": "terrain", "label": "Another layer"}
			]}),
			"Duplicate physics layer ID",
		),
	] {
		let mut source: Value = serde_json::from_str(LEGACY).unwrap();
		source["physics_layers"] = policy;
		assert!(
			ProjectData::from_json(&source.to_string())
				.unwrap_err()
				.contains(expected),
			"{source}"
		);
	}
}
