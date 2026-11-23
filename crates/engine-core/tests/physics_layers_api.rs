mod support;

use engine_core::{
	Key,
	character_controller::{CharacterControllerComponent, CharacterControllerSettings},
	collision::InteractionMatrix,
	components::{
		ComponentId, NativeBehaviorComponent, NativeBehaviorSpec, PlayerControllerComponent,
		PlayerControllerSettings,
	},
	input::{GameInputBinding, InputProfile},
	project::{PhysicsLayers, ProjectData, inspect_physics_contacts},
	scene_data::{ColliderData, ColliderInteraction},
	simulation_session::SimulationSession,
};
use serde_json::{Value, json};
use std::sync::Arc;
use support::{entity, key, scene};

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

#[test]
fn ignored_required_pair_blocks_session_but_optional_pair_only_warns() {
	let mut player = entity("player", [0.0, 0.9, 0.0]);
	player.collider = Some(ColliderData::Box {
		half_extents: [0.3, 0.9, 0.3],
		offset: [0.0; 3],
		interaction: ColliderInteraction::default(),
	});
	player.character_controller = Some(CharacterControllerComponent {
		id: ComponentId::new("character").unwrap(),
		enabled: true,
		settings: CharacterControllerSettings::default(),
	});
	player.player_controller = Some(Box::new(PlayerControllerComponent {
		id: ComponentId::new("controller").unwrap(),
		enabled: true,
		settings: PlayerControllerSettings::default(),
	}));
	let mut sensor = entity("sensor", [0.0, 0.9, 0.0]);
	sensor.collider = Some(ColliderData::Box {
		half_extents: [0.5; 3],
		offset: [0.0; 3],
		interaction: ColliderInteraction {
			sensor: true,
			layers: 1 << 31,
			..Default::default()
		},
	});
	let mut data = scene(vec![player, sensor]);
	let mut project = ProjectData::for_scene("Contacts", "scenes/main.json", &data).unwrap();
	data.entities[0]
		.player_controller
		.as_mut()
		.unwrap()
		.settings
		.local = true;
	project.input_profile = Some(InputProfile {
		actions: vec![GameInputBinding {
			id: "jump".into(),
			key: Key::Space,
			animation_parameter: None,
		}],
		..Default::default()
	});
	project.physics_layers = Some(PhysicsLayers {
		ignored_pairs: vec![[0, 31]],
		..Default::default()
	});
	let matrix = InteractionMatrix::compile(project.physics_layers.as_ref()).unwrap();
	let optional = inspect_physics_contacts(&data, &matrix, 8);
	assert_eq!(optional.len(), 1);
	assert_eq!(optional[0].sensor, &key("sensor"));
	assert_eq!(optional[0].actor, &key("player"));
	assert!(optional[0].component.is_none());
	assert!(optional[0].message().contains("project matrix"));
	assert!(SimulationSession::new(project.clone(), Arc::new(data.clone())).is_ok());

	let id = ComponentId::new("pickup").unwrap();
	data.entities[1]
		.native_behaviors
		.push(NativeBehaviorComponent {
			id: id.clone(),
			enabled: true,
			behavior: NativeBehaviorSpec::Collectible {
				actor: key("player"),
				trigger: key("sensor"),
				visual: None,
				despawn_owner: false,
			},
		});
	let required = inspect_physics_contacts(&data, &matrix, 8);
	assert_eq!(required.len(), 1);
	assert_eq!(required[0].component, Some(&id));
	assert_eq!(required[0].owner.key, key("sensor"));
	let expected = required[0].message();
	let document = Arc::new(data);
	let reason = SimulationSession::new(project.clone(), Arc::clone(&document)).err();
	assert_eq!(reason.as_deref(), Some(expected.as_str()));
	project.physics_layers = None;
	assert!(SimulationSession::new(project, document).is_ok());
}
