mod support;
use engine_core::{
	Engine,
	command::{EditorCommandDto as Command, EditorCommandRequest},
	components::{
		COLLIDER_TYPE, ComponentData, ComponentId, ComponentRecord, MESH_TYPE,
		NativeBehaviorComponent, NativeBehaviorSpec,
	},
	project::ProjectData,
	scene_data::{ColliderData, SceneData},
};
use std::collections::HashMap;
use support::*;

#[test]
fn script_component_round_trips_undoes_and_keeps_unknown_ids_for_repair() {
	let original = scene(vec![entity("door", [0., 0., 0.])]);
	let legacy = original.to_json().unwrap();
	assert!(SceneData::from_json(&legacy).unwrap().entities[0].scripts.is_empty());
	let project = ProjectData::for_scene("Scripts", "scenes/main.json", &original).unwrap();
	let mut engine = Engine::new(original, project).unwrap();
	let script: ComponentRecord = serde_json::from_value(serde_json::json!({
		"id": "door-control",
		"schema_version": 1,
		"enabled": true,
		"data": {
			"kind": "script",
			"settings": {
				"script_id": "project.unregistered",
				"config_version": 1,
				"properties": {"speed": 2.0, "on": true},
				"references": {"sensor": {"slot": "door_sensor"}}
			}
		}
	})).unwrap();
	run(&mut engine, Command::AddComponent { key: "door".into(), component: script.clone() });
	let saved = engine.session().authored_document().to_json().unwrap();
	let reopened = SceneData::from_json(&saved).unwrap();
	assert!(reopened.entities[0].component_records().contains(&script));
	run(&mut engine, Command::Undo);
	assert!(engine.session().authored_document().entities[0].scripts.is_empty());
	run(&mut engine, Command::Redo);
	assert_eq!(engine.session().authored_document().entities[0].scripts[0].settings.script_id, "project.unregistered");
}
fn run(engine: &mut Engine, command: Command) {
	engine
		.execute(EditorCommandRequest {
			expected_revision: engine.session().revision(),
			command,
		})
		.unwrap();
}
fn collider(id: &str, enabled: bool, offset: [f32; 3]) -> ComponentRecord {
	ComponentRecord {
		id: ComponentId::new(id).unwrap(),
		schema_version: 1,
		enabled,
		data: ComponentData::Collider {
			collider: ColliderData::Box {
				half_extents: [0.5; 3],
				offset,
				interaction: Default::default(),
			},
		},
	}
}

#[test]
fn sky_assignment_validates_and_undoes_edits_without_changing_legacy_clear() {
	let data = scene(vec![]);
	assert!(data.active_sky().is_none());
	assert!(!data.to_json().unwrap().contains("\"environment\""));
	let project = ProjectData::for_scene("Skies", "scenes/main.json", &data).unwrap();
	let mut engine = Engine::new(data, project).unwrap();
	run(&mut engine, Command::CreateSky);
	let first = engine.session().selection()[0].clone();
	run(&mut engine, Command::CreateSky);
	let second = engine.session().selection()[0].clone();
	run(
		&mut engine,
		Command::SetActiveSky {
			key: Some(first.clone()),
		},
	);
	let before = engine.session().authored_document();
	let receipt = engine
		.execute(EditorCommandRequest {
			expected_revision: engine.session().revision(),
			command: Command::SetActiveSky {
				key: Some(second.clone()),
			},
		})
		.unwrap();
	assert!(receipt.changes.environment_changed);
	let after = engine.session().authored_document();
	assert_eq!(after.environment.active_sky.as_ref(), Some(&second));
	assert_eq!(after.active_sky(), Some(&Default::default()));
	assert_eq!(
		SceneData::from_json(&after.to_json().unwrap()).unwrap(),
		*after
	);
	assert!(
		after
			.entities
			.iter()
			.all(|entity| entity.collider.is_none())
	);
	run(&mut engine, Command::Undo);
	assert_eq!(
		engine.session().authored_document().environment,
		before.environment
	);
	let missing = engine_core::EntityKey::new("missing-sky").unwrap();
	assert!(
		engine
			.execute(EditorCommandRequest {
				expected_revision: engine.session().revision(),
				command: Command::SetActiveSky { key: Some(missing) },
			})
			.is_err()
	);
	run(
		&mut engine,
		Command::Select {
			keys: vec![first.as_str().into()],
		},
	);
	run(&mut engine, Command::DeleteSelection);
	assert!(engine.session().authored_document().active_sky().is_none());
	run(&mut engine, Command::Undo);
	assert_eq!(
		engine.session().authored_document().environment.active_sky,
		Some(first)
	);
}

#[test]
fn scene_lighting_updates_keep_sky_identity_and_round_trip_separately() {
	let scene = scene(vec![]);
	let project = ProjectData::for_scene("Environment", "scenes/main.json", &scene).unwrap();
	let mut engine = Engine::new(scene, project).unwrap();
	run(&mut engine, Command::CreateSky);
	let sky = engine.session().selection()[0].clone();
	run(
		&mut engine,
		Command::SetActiveSky {
			key: Some(sky.clone()),
		},
	);
	let baseline = engine.session().authored_document();
	run(
		&mut engine,
		Command::SetSceneEnvironment {
			ambient_linear: [0.05, 0.10, 0.20],
			sky_exposure_ev: 2.0,
		},
	);
	let adjusted = engine.session().authored_document();
	assert_eq!(adjusted.environment.active_sky, Some(sky));
	assert_eq!(adjusted.environment.ambient_linear, [0.05, 0.10, 0.20]);
	assert_eq!(adjusted.environment.sky_exposure_ev, 2.0);
	assert_eq!(adjusted.active_sky(), baseline.active_sky());
	assert_eq!(adjusted.lights, baseline.lights);
	assert_eq!(
		SceneData::from_json(&adjusted.to_json().unwrap()).unwrap(),
		*adjusted
	);
	let revision = engine.session().revision();
	assert!(
		engine
			.execute(EditorCommandRequest {
				expected_revision: revision,
				command: Command::SetSceneEnvironment {
					ambient_linear: [f32::NAN; 3],
					sky_exposure_ev: 0.0
				},
			})
			.is_err()
	);
	assert_eq!(engine.session().revision(), revision);
	run(&mut engine, Command::Undo);
	assert_eq!(
		engine.session().authored_document().environment,
		baseline.environment
	);
}

#[test]
fn audio_source_create_update_undo_duplication_and_save_are_atomic() {
	use engine_core::{
		asset_catalog::AssetId, audio_clip::AudioClipRef, components::AudioSourceSettings,
	};
	let data = scene(vec![]);
	let project = ProjectData::for_scene("Audio Sources", "scenes/main.json", &data).unwrap();
	let mut engine = Engine::new(data, project).unwrap();
	run(&mut engine, Command::CreateAudioSource);
	let key = engine.session().selection()[0].clone();
	let mut component = engine.session().authored_document().entities[0]
		.component_records()
		.remove(0);
	let clip = Some(AudioClipRef {
		asset: AssetId::generate(),
	});
	component.data = ComponentData::AudioSource {
		settings: AudioSourceSettings {
			clip: clip.clone(),
			gain: 0.4,
			r#loop: true,
			..Default::default()
		},
	};
	run(
		&mut engine,
		Command::ReplaceComponent {
			key: key.as_str().into(),
			component,
		},
	);
	run(&mut engine, Command::Undo);
	assert!(
		engine.session().authored_document().entities[0]
			.audio_source
			.as_ref()
			.unwrap()
			.settings
			.clip
			.is_none()
	);
	run(&mut engine, Command::Redo);
	run(
		&mut engine,
		Command::DuplicateSelection {
			offset: [1., 0., 0.],
		},
	);
	let document = engine.session().authored_document();
	assert_ne!(document.entities[0].key, document.entities[1].key);
	assert!(
		document
			.entities
			.iter()
			.all(|entity| entity.audio_source.as_ref().unwrap().settings.clip == clip)
	);
	let saved = document.to_json().unwrap();
	assert_eq!(SceneData::from_json(&saved).unwrap(), *document);
	let mut invalid: serde_json::Value = serde_json::from_str(&saved).unwrap();
	invalid["entities"][0]["components"][0]["data"]["settings"]["gain"] = 1.5.into();
	assert!(
		SceneData::from_json(&invalid.to_string())
			.unwrap_err()
			.contains("Audio Source gain")
	);
}
#[test]
fn canonical_scene_has_one_typed_payload_and_flat_transport_is_a_read_projection() {
	let mut actor = entity("actor", [0.; 3]);
	actor
		.put_component(collider("collision", false, [0.1, 0., 0.]), false)
		.unwrap();
	let data = scene(vec![actor]);
	let source = data.to_json().unwrap();
	let value: serde_json::Value = serde_json::from_str(&source).unwrap();
	assert_eq!(
		value["schema_version"],
		engine_core::scene_data::CURRENT_SCHEMA_VERSION
	);
	let saved = &value["entities"][0];
	assert!(saved.get("model_path").is_none());
	assert!(saved.get("collider").is_none());
	assert!(saved.get("component_views").is_none());
	assert_eq!(saved["components"].as_array().unwrap().len(), 2);
	assert_eq!(SceneData::from_json(&source).unwrap(), data);
	assert_eq!(serde_json::from_str::<SceneData>(&source).unwrap(), data);
	let mut projection = serde_json::to_value(&data).unwrap();
	assert_eq!(
		projection["entities"][0]["model_path"],
		"models/triangle.gltf"
	);
	assert_eq!(
		projection["entities"][0]["component_views"][1]["id"],
		"collision"
	);
	// The inspector's read projection cannot become a second mutation authority.
	projection["entities"][0]["component_views"][0]["data"]["model_path"] =
		serde_json::json!("models/other.gltf");
	assert_eq!(
		serde_json::from_value::<SceneData>(projection).unwrap(),
		data
	);
	let mut mixed = value;
	mixed["entities"][0]["model_path"] = serde_json::json!("models/other.gltf");
	assert!(SceneData::from_json(&mixed.to_string()).is_err());
}
#[test]
fn version_three_migration_preserves_ids_assets_flags_and_optional_values() {
	let mut actor = entity("actor", [1., 2., 3.]).with_collider(Some(ColliderData::Sphere {
		radius: 0.5,
		offset: [0.2, 0., 0.],
		interaction: Default::default(),
	}));
	actor.renderable = false;
	actor.collision_enabled = false;
	let data = scene(vec![actor]);
	let mut legacy = serde_json::to_value(&data).unwrap();
	legacy["schema_version"] = serde_json::json!(3);
	legacy["entities"][0]
		.as_object_mut()
		.unwrap()
		.remove("component_views");
	let migrated = SceneData::from_json(&legacy.to_string()).unwrap();
	assert_eq!(migrated.entities[0], data.entities[0]);
	assert_eq!(migrated.entities.len(), 2);
	let camera = &migrated.entities[1];
	assert_eq!(migrated.default_game_camera.as_ref(), Some(&camera.key));
	assert!(camera.camera.as_ref().is_some_and(|component| component.enabled));
	assert_eq!(migrated.camera, data.camera);
	let saved = migrated.to_json().unwrap();
	assert_eq!(SceneData::from_json(&saved).unwrap(), migrated);
	assert_eq!(
		migrated.entities[0].component_id(MESH_TYPE).as_str(),
		"mesh"
	);
	assert_eq!(
		migrated.entities[0].component_id(COLLIDER_TYPE).as_str(),
		"collider"
	);
}
#[test]
fn component_commands_are_undoable_atomic_and_retain_attachment_identity() {
	let data = scene(vec![entity("actor", [0.; 3])]);
	let project = ProjectData::for_scene("Components", "scenes/main.json", &data).unwrap();
	let mut engine = Engine::new(data.clone(), project).unwrap();
	let id = ComponentId::new("collision").unwrap();
	run(
		&mut engine,
		Command::AddComponent {
			key: "actor".into(),
			component: collider(id.as_str(), false, [0.; 3]),
		},
	);
	run(
		&mut engine,
		Command::ReplaceComponent {
			key: "actor".into(),
			component: collider(id.as_str(), false, [1., 0., 0.]),
		},
	);
	run(
		&mut engine,
		Command::SetComponentEnabled {
			key: "actor".into(),
			component_id: id.clone(),
			enabled: true,
		},
	);
	let authored = engine.session().authored_document();
	assert_eq!(authored.entities[0].component_id(COLLIDER_TYPE), id);
	assert!(authored.entities[0].collision_enabled);
	let revision = engine.session().revision();
	let cursor = engine.change_cursor();
	assert!(
		engine
			.execute(EditorCommandRequest {
				expected_revision: revision,
				command: Command::AddComponent {
					key: "actor".into(),
					component: collider("other", true, [0.; 3])
				}
			})
			.is_err()
	);
	assert_eq!(engine.session().authored_document(), authored);
	assert_eq!(engine.change_cursor(), cursor);
	assert_eq!(engine.session().revision(), revision);
	run(
		&mut engine,
		Command::RemoveComponent {
			key: "actor".into(),
			component_id: id.clone(),
		},
	);
	assert!(
		engine.session().authored_document().entities[0]
			.collider
			.is_none()
	);
	run(&mut engine, Command::Undo);
	assert_eq!(engine.session().authored_document(), authored);
	for _ in 0..3 {
		run(&mut engine, Command::Undo);
	}
	assert_eq!(*engine.session().authored_document(), data);
	for _ in 0..3 {
		run(&mut engine, Command::Redo);
	}
	assert_eq!(engine.session().authored_document(), authored);
	run(&mut engine, Command::StartPlay);
	let revision = engine.session().revision();
	assert!(
		engine
			.execute(EditorCommandRequest {
				expected_revision: revision,
				command: Command::SetComponentEnabled {
					key: "actor".into(),
					component_id: id,
					enabled: false
				}
			})
			.is_err()
	);
	assert_eq!(engine.session().authored_document(), authored);
}
#[test]
fn component_preview_cancel_restores_data_ids_and_redo() {
	let data = scene(vec![entity("actor", [0.; 3])]);
	let project = ProjectData::for_scene("Components", "scenes/main.json", &data).unwrap();
	let mut engine = Engine::new(data.clone(), project).unwrap();
	run(
		&mut engine,
		Command::Rename {
			key: "actor".into(),
			name: "Later".into(),
		},
	);
	run(&mut engine, Command::Undo);
	assert!(engine.snapshot().unwrap().can_redo);
	run(
		&mut engine,
		Command::BeginTransaction {
			label: "Add shape".into(),
		},
	);
	run(
		&mut engine,
		Command::AddComponent {
			key: "actor".into(),
			component: collider("preview", true, [0.; 3]),
		},
	);
	run(&mut engine, Command::CancelTransaction);
	assert_eq!(*engine.session().authored_document(), data);
	assert!(engine.snapshot().unwrap().can_redo);
	run(&mut engine, Command::Redo);
	assert_eq!(
		engine.session().authored_document().entities[0].name,
		"Later"
	);
}
#[test]
fn native_reference_remap_includes_self_and_bound_slots_without_rewriting_external_targets() {
	let mut owner = entity("owner", [0.; 3]);
	owner.native_behaviors.push(NativeBehaviorComponent {
		id: ComponentId::new("door-behavior").unwrap(),
		enabled: true,
		behavior: NativeBehaviorSpec::KeyDoor {
			actor: key("actor"),
			key_trigger: key("owner"),
			door: key("door"),
			key_visual: Some(key("visual")),
		},
	});
	owner.native_behaviors.push(NativeBehaviorComponent {
		id: ComponentId::new("spawner").unwrap(),
		enabled: true,
		behavior: NativeBehaviorSpec::TimedSpawner {
			prefab_path: "prefabs/item.json".into(),
			interval_ticks: 60,
			max_alive: 4,
			lifetime_ticks: 300,
			spawn_offset: [0.; 3],
			bindings: std::collections::BTreeMap::from([("actor".into(), key("actor"))]),
		},
	});
	let remap = HashMap::from([
		(key("owner"), key("copy-owner")),
		(key("actor"), key("copy-actor")),
	]);
	owner.remap_component_references(&remap).unwrap();
	let refs = owner.native_behaviors[0].behavior.entity_references();
	assert_eq!(
		refs,
		vec![
			&key("copy-actor"),
			&key("copy-owner"),
			&key("door"),
			&key("visual")
		]
	);
	assert_eq!(
		owner.native_behaviors[1].behavior.entity_references(),
		vec![&key("copy-actor")]
	);
	assert_eq!(owner.native_behaviors[0].id.as_str(), "door-behavior");
}
#[test]
fn malformed_component_versions_ids_payloads_and_dangling_references_are_rejected() {
	for id in ["", "has space", "x.y"] {
		assert!(ComponentId::new(id).is_err());
	}
	let mut invalid = collider("shape", true, [0.; 3]);
	invalid.schema_version = 2;
	assert!(invalid.validate().is_err());
	let mut owner = entity("owner", [0.; 3]);
	owner.native_behaviors.push(NativeBehaviorComponent {
		id: ComponentId::new("behavior").unwrap(),
		enabled: true,
		behavior: NativeBehaviorSpec::Collectible {
			actor: key("actor"),
			trigger: key("missing"),
			visual: None,
			despawn_owner: false,
		},
	});
	assert!(
		scene(vec![owner, entity("actor", [0.; 3])])
			.validate()
			.is_err()
	);
	for (interval, max_alive, lifetime) in [(0, 1, 1), (1, 129, 1), (1, 1, 0), (1_000_001, 1, 1)] {
		assert!(
			NativeBehaviorSpec::TimedSpawner {
				prefab_path: "item.json".into(),
				interval_ticks: interval,
				max_alive,
				lifetime_ticks: lifetime,
				spawn_offset: [0.; 3],
				bindings: Default::default()
			}
			.validate()
			.is_err()
		);
	}
	let source = serde_json::to_value(collider("shape", true, [0.; 3])).unwrap();
	let mut unknown = source.clone();
	unknown["data"]["extra"] = serde_json::json!(true);
	assert!(serde_json::from_value::<ComponentRecord>(unknown).is_err());
}

#[test]
fn enabled_behaviors_validate_real_capabilities_and_only_explicit_slots_defer_them() {
	use engine_core::components::{NO_MESH_PATH, validate_scene_components_with_slots};
	use std::collections::HashSet;
	let shape = |sensor| ColliderData::Sphere {
		radius: 0.5,
		offset: [0.; 3],
		interaction: engine_core::collider::ColliderInteraction {
			sensor,
			..Default::default()
		},
	};
	let actor = entity("actor", [0.; 3]).with_collider(Some(shape(false)));
	let trigger = entity("trigger", [0.; 3]).with_collider(Some(shape(true)));
	let door = entity("door", [2., 0., 0.]).with_collider(Some(shape(false)));
	let visual = entity("visual", [0.; 3]);
	let mut owner = entity("owner", [0.; 3]);
	owner.native_behaviors.push(NativeBehaviorComponent {
		id: ComponentId::new("key-door").unwrap(),
		enabled: true,
		behavior: NativeBehaviorSpec::KeyDoor {
			actor: key("actor"),
			key_trigger: key("trigger"),
			door: key("door"),
			key_visual: Some(key("visual")),
		},
	});
	let valid = scene(vec![owner, actor, trigger, door, visual]);
	valid.validate().unwrap();
	for target in [1, 2, 3] {
		let mut invalid = valid.clone();
		invalid.entities[target].collider = None;
		assert!(invalid.validate().is_err());
	}
	let mut invalid = valid.clone();
	invalid.entities[2].collider = Some(shape(false));
	assert!(invalid.validate().unwrap_err().contains("Sensor"));
	let mut invalid = valid.clone();
	invalid.entities[3].collider = Some(shape(true));
	assert!(invalid.validate().unwrap_err().contains("blocking"));
	let mut invalid = valid.clone();
	invalid.entities[4].model_path = NO_MESH_PATH.into();
	invalid.entities[4].renderable = false;
	assert!(invalid.validate().unwrap_err().contains("mesh renderer"));
	let mut invalid = valid.clone();
	invalid.entities[1].collider = None;
	invalid.entities[0].native_behaviors[0].enabled = false;
	invalid.validate().unwrap();
	let mut placeholder = valid.clone();
	placeholder.entities[1].collider = None;
	placeholder.entities[1].model_path = NO_MESH_PATH.into();
	placeholder.entities[1].renderable = false;
	assert!(placeholder.validate().is_err());
	validate_scene_components_with_slots(&placeholder, &HashSet::from([key("actor")])).unwrap();
	// Deferring the external actor does not conceal a broken internal sensor.
	placeholder.entities[2].collider = Some(shape(false));
	assert!(
		validate_scene_components_with_slots(&placeholder, &HashSet::from([key("actor")])).is_err()
	);
}
#[test]
fn authored_scene_enforces_the_combined_active_native_behavior_budget() {
	let mut prototype = entity("prototype", [0.; 3]);
	for index in 0..8 {
		prototype.native_behaviors.push(NativeBehaviorComponent {
			id: ComponentId::new(format!("timer-{index}")).unwrap(),
			enabled: true,
			behavior: NativeBehaviorSpec::TimedSpawner {
				prefab_path: "item.json".into(),
				interval_ticks: 60,
				max_alive: 1,
				lifetime_ticks: 300,
				spawn_offset: [0.; 3],
				bindings: Default::default(),
			},
		});
	}
	let mut entities = Vec::new();
	for index in 0..512 {
		let mut copy = prototype.clone();
		copy.key = key(&format!("owner-{index}"));
		entities.push(copy);
	}
	let mut overflow = prototype;
	overflow.key = key("overflow");
	overflow.native_behaviors.truncate(1);
	entities.push(overflow);
	let mut data = scene(entities);
	assert!(data.validate().unwrap_err().contains("4096 active"));
	data.entities.last_mut().unwrap().native_behaviors[0].enabled = false;
	data.validate().unwrap();
}

#[test]
fn interactable_records_roundtrip_with_identity_and_reject_invalid_settings() {
	let id = ComponentId::new("inspect").unwrap();
	let component = |action: &str, reach, enabled| ComponentRecord {
		id: id.clone(),
		schema_version: 1,
		enabled,
		data: ComponentData::Interactable {
			settings: engine_core::components::InteractableSettings {
				action: action.into(),
				reach,
			},
		},
	};
	let mut lever = entity("lever", [0.; 3]);
	let legacy = scene(vec![lever.clone()]).to_json().unwrap();
	assert!(
		SceneData::from_json(&legacy).unwrap().entities[0]
			.interactable
			.is_none()
	);
	for invalid in [
		component("Wrong", 1.0, true),
		component("interact", 0.0, true),
		component("interact", 3.1, true),
		component("interact", f32::NAN, true),
		ComponentRecord {
			schema_version: 2,
			..component("interact", 1.0, true)
		},
	] {
		assert!(lever.put_component(invalid, false).is_err());
		assert!(lever.interactable.is_none());
	}
	lever
		.put_component(component("interact", 1.0, true), false)
		.unwrap();
	let mut duplicate = component("interact", 2.0, true);
	duplicate.id = ComponentId::new("different-id").unwrap();
	assert!(lever.put_component(duplicate, false).is_err());
	lever
		.put_component(component("interact", 1.25, true), true)
		.unwrap();
	lever.set_component_enabled(&id, false).unwrap();
	let data = scene(vec![lever]);
	let saved = data.to_json().unwrap();
	let mut json: serde_json::Value = serde_json::from_str(&saved).unwrap();
	assert!(json["entities"][0].get("interactable").is_none());
	let mut reopened = SceneData::from_json(&saved).unwrap();
	assert_eq!(reopened, data);
	assert_eq!(
		reopened.entities[0]
			.interactable
			.as_ref()
			.unwrap()
			.settings
			.reach,
		1.25
	);
	assert!(!reopened.entities[0].interactable.as_ref().unwrap().enabled);
	reopened.entities[0].remove_component(&id).unwrap();
	assert!(reopened.entities[0].interactable.is_none());

	let records = json["entities"][0]["components"].as_array_mut().unwrap();
	let authored = records
		.iter_mut()
		.find(|record| record["id"] == "inspect")
		.unwrap();
	authored["data"]["settings"]["effect"] = serde_json::json!("unknown");
	assert!(SceneData::from_json(&json.to_string()).is_err());
}
