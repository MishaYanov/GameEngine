mod support;

use engine_core::{
	SceneData,
	asset_catalog::AssetId,
	command::{EditorCommandDto as Command, EditorCommandRequest},
	components::{ComponentData, ComponentId, ComponentRecord, ParticleEmitterSettings},
	editor::{EditorSession, PrefabData},
	linked_prefab::{LinkedPrefabOverrideTarget, LinkedPrefabProperty, LinkedPrefabSource},
	scene_data::{CURRENT_SCHEMA_VERSION, LightKind, MAX_SCENE_LIGHTS},
};
use serde_json::json;
use support::{entity, key, scene};

fn execute(session: &mut EditorSession, command: Command) -> Result<(), String> {
	session
		.execute_command(EditorCommandRequest {
			expected_revision: session.revision(),
			command,
		})
		.map(|_| ())
}

fn emitter(settings: ParticleEmitterSettings) -> ComponentRecord {
	ComponentRecord {
		id: ComponentId::new("particle-emitter").unwrap(),
		schema_version: 1,
		enabled: true,
		data: ComponentData::ParticleEmitter { settings },
	}
}

#[test]
fn object_menu_commands_create_real_undoable_scene_objects() {
	let mut session = EditorSession::new(scene(vec![])).unwrap();
	execute(&mut session, Command::CreateEmpty).unwrap();
	let empty = session.selection()[0].clone();
	let item = &session.authored_document().entities[0];
	assert_eq!(item.key, empty);
	assert!(!item.has_mesh_component());
	assert!(item.component_records().is_empty());

	execute(
		&mut session,
		Command::CreateLight {
			light_kind: LightKind::Point,
		},
	)
	.unwrap();
	let with_light = session.authored_document();
	let light = &with_light.projected_lights().unwrap()[0];
	assert_eq!(light.kind, LightKind::Point);
	assert_eq!(session.selection(), std::slice::from_ref(&light.key));
	assert_ne!(light.key, empty);
	assert!(with_light.lights.is_empty());
	execute(&mut session, Command::Undo).unwrap();
	assert!(
		session
			.authored_document()
			.projected_lights()
			.unwrap()
			.is_empty()
	);
	execute(&mut session, Command::Redo).unwrap();
	assert_eq!(
		session.authored_document().projected_lights().unwrap(),
		vec![light.clone()]
	);

	for _ in 1..MAX_SCENE_LIGHTS {
		execute(
			&mut session,
			Command::CreateLight {
				light_kind: LightKind::Directional,
			},
		)
		.unwrap();
	}
	let before = session.revision();
	assert!(
		execute(
			&mut session,
			Command::CreateLight {
				light_kind: LightKind::Point
			}
		)
		.is_err()
	);
	assert_eq!(session.revision(), before);
	assert_eq!(
		session
			.authored_document()
			.projected_lights()
			.unwrap()
			.len(),
		MAX_SCENE_LIGHTS
	);
}

#[test]
fn legacy_scene_lights_keep_identity_and_rendered_pose_after_edit_and_save() {
	use engine_core::scene_data::LightData;

	let mut original = scene(vec![]);
	let mut point = LightData::new(key("room-lamp"), "Room Lamp", LightKind::Point);
	point.position = [3.0, 2.0, 1.0];
	point.color = [0.25, 0.5, 1.0];
	let mut sun = LightData::new(key("sun"), "Sun", LightKind::Directional);
	sun.direction = [2.0, -1.0, 3.0];
	original.lights = vec![point.clone(), sun.clone()];
	let mut loaded = SceneData::from_json(&original.to_json().unwrap()).unwrap();
	loaded.migrate_legacy_lights().unwrap();
	let mut session = EditorSession::new(loaded).unwrap();
	let snapshot = session.authored_document();
	assert_eq!(snapshot.entities[0].key, point.key);
	assert_eq!(snapshot.entities[1].key, sun.key);
	assert!(snapshot.lights.is_empty());
	let initial = snapshot.projected_lights().unwrap();
	assert_eq!(initial[0].position, point.position);
	assert_eq!(initial[0].color, point.color);
	let direction = glam::Vec3::from_array(initial[1].direction);
	assert!((glam::Vec3::from_array(sun.direction).normalize() - direction).length() < 1.0e-5);

	let mut changed = initial[0].clone();
	changed.color = [0.3, 0.6, 1.0];
	changed.position = [4.0, 4.0, 2.0];
	execute(
		&mut session,
		Command::UpsertLight {
			light: changed.clone(),
		},
	)
	.unwrap();
	let edited = session.authored_document();
	assert_eq!(edited.projected_lights().unwrap()[0], changed);
	let authored: serde_json::Value = serde_json::from_str(&edited.to_json().unwrap()).unwrap();
	assert_eq!(authored["lights"], json!([]));
	assert_eq!(
		authored["entities"][0]["components"][0]["data"]["kind"],
		"light"
	);
	assert_eq!(
		SceneData::from_json(&authored.to_string())
			.unwrap()
			.projected_lights()
			.unwrap()[0],
		changed
	);
	execute(&mut session, Command::Undo).unwrap();
	assert_eq!(
		session.authored_document().projected_lights().unwrap(),
		initial
	);
}

#[test]
fn particle_emitter_round_trips_in_v8_and_rejects_invalid_or_legacy_payloads() {
	let mut session = EditorSession::new(scene(vec![])).unwrap();
	execute(&mut session, Command::CreateParticleEmitter).unwrap();
	let selected = session.selection()[0].clone();
	let saved = session.authored_document();
	assert_eq!(saved.schema_version, CURRENT_SCHEMA_VERSION);
	assert!(!saved.entities[0].has_mesh_component());
	assert!(saved.entities[0].particle_emitter.as_ref().unwrap().enabled);
	let expected = saved.entities[0].component_records();
	let serialized = saved.to_json().unwrap();
	let mut value: serde_json::Value = serde_json::from_str(&serialized).unwrap();
	assert_eq!(value["schema_version"], 8);
	assert_eq!(
		value["entities"][0]["components"][0]["data"]["kind"],
		"particle_emitter"
	);
	assert!(value["entities"][0].get("particle_emitter").is_none());
	assert_eq!(
		SceneData::from_json(&serialized).unwrap().entities[0].component_records(),
		expected
	);
	let revision = session.revision();
	let invalid = emitter(ParticleEmitterSettings {
		rate_per_second: f32::NAN,
		..Default::default()
	});
	assert!(
		execute(
			&mut session,
			Command::ReplaceComponent {
				key: selected.as_str().into(),
				component: invalid
			}
		)
		.is_err()
	);
	assert_eq!(session.revision(), revision);

	value["schema_version"] = json!(7);
	assert!(
		SceneData::from_json(&value.to_string())
			.unwrap_err()
			.contains("requires scene schema 8")
	);
	value["entities"][0]["components"] = json!([]);
	assert!(SceneData::from_json(&value.to_string()).is_ok());

	let changed = emitter(ParticleEmitterSettings {
		speed: 10.,
		..Default::default()
	});
	execute(
		&mut session,
		Command::ReplaceComponent {
			key: selected.as_str().into(),
			component: changed.clone(),
		},
	)
	.unwrap();
	assert_eq!(
		session.authored_document().entities[0].component_records(),
		[changed]
	);
	execute(&mut session, Command::Undo).unwrap();
	assert_eq!(
		session.authored_document().entities[0].component_records(),
		expected
	);
}

#[test]
fn linked_light_values_round_trip_as_instance_overrides() {
	use engine_core::components::LightSettings;

	let mut source = entity("bulb", [0.; 3]);
	let original = ComponentRecord {
		id: ComponentId::new("bulb").unwrap(),
		schema_version: 1,
		enabled: true,
		data: ComponentData::Light {
			settings: LightSettings {
				kind: LightKind::Point,
				color: [1.; 3],
				intensity: 1.0,
				range: 5.0,
			},
		},
	};
	source.put_component(original.clone(), false).unwrap();
	let mut session = EditorSession::new(scene(vec![])).unwrap();
	session
		.instantiate_linked_prefab(
			LinkedPrefabSource {
				source_id: AssetId::generate(),
				path: "prefabs/lamp.json".into(),
				fingerprint: "1".repeat(64),
				prefab: PrefabData {
					schema_version: 4,
					entities: vec![source],
					composition: Default::default(),
					gameplay: None,
					reference_slots: Default::default(),
				},
			},
			[0.; 3],
			Default::default(),
		)
		.unwrap();
	let document = session.authored_document();
	let link = document
		.entities
		.iter()
		.find_map(|entity| entity.linked_prefab.as_ref())
		.unwrap();
	let member = &link.entity_map[&key("bulb")];
	let mut changed = original;
	let ComponentData::Light { settings } = &mut changed.data else {
		unreachable!()
	};
	settings.intensity = 4.0;
	execute(
		&mut session,
		Command::ReplaceComponent {
			key: member.as_str().into(),
			component: changed,
		},
	)
	.unwrap();
	let text = session.authored_document().to_json().unwrap();
	let reopened = SceneData::from_json(&text).unwrap();
	let link = reopened
		.entities
		.iter()
		.find_map(|entity| entity.linked_prefab.as_ref())
		.unwrap();
	assert!(link.overrides.contains(&LinkedPrefabOverrideTarget {
		source_entity: key("bulb"),
		property: LinkedPrefabProperty::Light,
	}));
	assert_eq!(reopened.projected_lights().unwrap()[0].intensity, 4.0);
}

#[test]
fn linked_particle_edits_persist_as_instance_overrides() {
	let mut object = entity("emitter", [0.; 3]);
	let original = emitter(ParticleEmitterSettings::default());
	object.put_component(original.clone(), false).unwrap();
	let mut session = EditorSession::new(scene(vec![])).unwrap();
	session
		.instantiate_linked_prefab(
			LinkedPrefabSource {
				source_id: AssetId::generate(),
				path: "prefabs/emitter.prefab.json".into(),
				fingerprint: "1".repeat(64),
				prefab: PrefabData {
					schema_version: 4,
					entities: vec![object],
					composition: Default::default(),
					gameplay: None,
					reference_slots: Default::default(),
				},
			},
			[0.; 3],
			Default::default(),
		)
		.unwrap();
	let initial = session.authored_document();
	let anchor = initial
		.entities
		.iter()
		.find(|item| item.linked_prefab.is_some())
		.unwrap();
	let owner = anchor.linked_prefab.as_ref().unwrap().entity_map[&key("emitter")].clone();
	let root = anchor.key.clone();
	let target = LinkedPrefabOverrideTarget {
		source_entity: key("emitter"),
		property: LinkedPrefabProperty::ParticleEmitter,
	};
	let changed = emitter(ParticleEmitterSettings {
		speed: 7.,
		..Default::default()
	});
	execute(
		&mut session,
		Command::ReplaceComponent {
			key: owner.as_str().into(),
			component: changed.clone(),
		},
	)
	.unwrap();
	let document = session.authored_document();
	let serialized = document.to_json().unwrap();
	let reopened = SceneData::from_json(&serialized).unwrap();
	assert!(
		reopened
			.entities
			.iter()
			.find(|item| item.key == root)
			.unwrap()
			.linked_prefab
			.as_ref()
			.unwrap()
			.overrides
			.contains(&target)
	);
	assert!(
		reopened
			.entities
			.iter()
			.find(|item| item.key == owner)
			.unwrap()
			.component_records()
			.contains(&changed)
	);
	execute(
		&mut session,
		Command::RevertLinkedPrefab {
			root_key: root.as_str().into(),
			property: Some(target),
		},
	)
	.unwrap();
	assert!(
		session
			.authored_document()
			.entities
			.iter()
			.find(|item| item.key == owner)
			.unwrap()
			.component_records()
			.contains(&original)
	);

	// The pin remains authored even when the last instance emitter is removed.
	execute(
		&mut session,
		Command::RemoveComponent {
			key: owner.as_str().into(),
			component_id: original.id.clone(),
		},
	)
	.unwrap();
	let without_live_emitters = session.authored_document();
	assert!(
		without_live_emitters
			.entities
			.iter()
			.all(|item| item.particle_emitter.is_none())
	);
	let saved = without_live_emitters.to_json().unwrap();
	assert!(SceneData::from_json(&saved).is_ok());

	let mut mislabeled: serde_json::Value = serde_json::from_str(&saved).unwrap();
	mislabeled["schema_version"] = json!(7);
	let index = without_live_emitters
		.entities
		.iter()
		.position(|item| item.key == root)
		.unwrap();
	assert!(
		SceneData::from_json(&mislabeled.to_string())
			.unwrap_err()
			.contains("requires scene schema 8")
	);
	mislabeled["entities"][index]["linked_prefab"]["overrides"] = json!([]);
	assert!(
		SceneData::from_json(&mislabeled.to_string())
			.unwrap_err()
			.contains("requires scene schema 8")
	);
}
