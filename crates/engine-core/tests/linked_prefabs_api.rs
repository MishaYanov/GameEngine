mod support;
use engine_core::{
	EntityKey,
	asset_catalog::AssetId,
	command::{EditorCommandDto as Command, EditorCommandRequest},
	editor::{EditorSession, PrefabData},
	linked_prefab::{
		LinkedPrefabOverrideTarget, LinkedPrefabProperty, LinkedPrefabSource,
		PrefabConflictResolution,
	},
	project::ProjectData,
};
use std::collections::BTreeMap;
use support::*;

fn source() -> LinkedPrefabSource {
	LinkedPrefabSource {
		source_id: AssetId::generate(),
		path: "prefabs/assembly.json".into(),
		fingerprint: "1".repeat(64),
		prefab: PrefabData {
			schema_version: 3,
			entities: vec![
				entity("body", [0.0; 3]),
				entity("child", [1.0, 0.0, 0.0]).with_parent(Some(key("body"))),
			],
			gameplay: None,
			reference_slots: BTreeMap::new(),
		},
	}
}
fn session() -> EditorSession {
	let scene = scene(vec![]);
	let project = ProjectData::for_scene("Links", "main.json", &scene).unwrap();
	EditorSession::for_project(scene, project).unwrap()
}

#[test]
fn selected_prop_pivot_offsets_roots_and_preserves_source_local_children() {
	let document = scene(vec![
		entity("frame", [4.0, 0.0, 0.0]),
		entity("hinge", [2.0, 0.0, 0.0]).with_parent(Some(key("frame"))),
		entity("plank", [8.0, 0.0, 0.0]),
		entity("outside", [10.0, 0.0, 0.0]),
	]);
	let mut session = EditorSession::new(document.clone()).unwrap();
	execute(
		&mut session,
		Command::Select {
			keys: vec!["frame".into(), "plank".into()],
		},
	)
	.unwrap();
	let revision = session.revision();
	let source = session
		.export_prefab_with_slots_at(BTreeMap::new(), Some(&key("hinge")))
		.unwrap();
	assert_eq!(source.entities.len(), 3);
	assert_eq!(source.entities[0].transform.position, [-2.0, 0.0, 0.0]);
	assert_eq!(source.entities[1].parent, Some(key("frame")));
	assert_eq!(source.entities[1].transform.position, [2.0, 0.0, 0.0]);
	assert_eq!(source.entities[2].transform.position, [2.0, 0.0, 0.0]);
	let error = session
		.export_prefab_with_slots_at(BTreeMap::new(), Some(&key("outside")))
		.unwrap_err();
	assert!(error.contains("outside"));
	assert_eq!(session.revision(), revision);
	assert_eq!(session.authored_document().as_ref(), &document);
}
fn execute(session: &mut EditorSession, command: Command) -> Result<(), String> {
	session
		.execute_command(EditorCommandRequest {
			expected_revision: session.revision(),
			command,
		})
		.map(|_| ())
}
fn instantiate(session: &mut EditorSession, source: LinkedPrefabSource) -> (EntityKey, EntityKey) {
	execute(
		session,
		Command::InstantiateLinkedPrefab {
			source,
			position: [2.0, 0.0, 0.0],
			bindings: BTreeMap::new(),
		},
	)
	.unwrap();
	let data = session.authored_document();
	let anchor = data
		.entities
		.iter()
		.find(|entity| entity.linked_prefab.is_some())
		.unwrap();
	(
		anchor.key.clone(),
		anchor.linked_prefab.as_ref().unwrap().entity_map[&key("body")].clone(),
	)
}
fn update(
	session: &mut EditorSession,
	root: &EntityKey,
	source: LinkedPrefabSource,
	resolution: PrefabConflictResolution,
) -> Result<(), String> {
	execute(
		session,
		Command::UpdateLinkedPrefab {
			root_key: root.as_str().into(),
			source,
			resolution,
			bindings: None,
		},
	)
}

#[test]
fn persisted_override_intent_survives_matching_source_then_conflict_revert_undo_and_redo() {
	let mut session = session();
	let original = source();
	let (root, body) = instantiate(&mut session, original.clone());
	execute(
		&mut session,
		Command::Rename {
			key: body.as_str().into(),
			name: "Local".into(),
		},
	)
	.unwrap();
	let mut fresh = original;
	fresh.fingerprint = "2".repeat(64);
	fresh.prefab.entities[0].name = "Local".into();
	update(
		&mut session,
		&root,
		fresh.clone(),
		PrefabConflictResolution::Reject,
	)
	.unwrap();
	let pin = session.authored_document();
	let link = pin
		.entities
		.iter()
		.find_map(|entity| entity.linked_prefab.as_ref())
		.unwrap();
	assert_eq!(link.overrides.len(), 1);
	drop(pin);
	fresh.fingerprint = "3".repeat(64);
	fresh.prefab.entities[0].name = "Source changed again".into();
	let before = session.snapshot().unwrap();
	assert!(
		update(
			&mut session,
			&root,
			fresh.clone(),
			PrefabConflictResolution::Reject
		)
		.unwrap_err()
		.contains("conflicts")
	);
	assert_eq!(session.revision(), before.revision);
	update(
		&mut session,
		&root,
		fresh,
		PrefabConflictResolution::KeepLocal,
	)
	.unwrap();
	assert_eq!(
		session
			.authored_document()
			.entities
			.iter()
			.find(|entity| entity.key == body)
			.unwrap()
			.name,
		"Local"
	);
	let property = LinkedPrefabOverrideTarget {
		source_entity: key("body"),
		property: LinkedPrefabProperty::Name,
	};
	execute(
		&mut session,
		Command::RevertLinkedPrefab {
			root_key: root.as_str().into(),
			property: Some(property),
		},
	)
	.unwrap();
	assert_eq!(
		session
			.authored_document()
			.entities
			.iter()
			.find(|entity| entity.key == body)
			.unwrap()
			.name,
		"Source changed again"
	);
	execute(&mut session, Command::Undo).unwrap();
	assert_eq!(
		session
			.authored_document()
			.entities
			.iter()
			.find(|entity| entity.key == body)
			.unwrap()
			.name,
		"Local"
	);
	execute(&mut session, Command::Redo).unwrap();
	assert!(
		session
			.authored_document()
			.entities
			.iter()
			.find_map(|entity| entity.linked_prefab.as_ref())
			.unwrap()
			.overrides
			.is_empty()
	);
}

#[test]
fn whole_instance_duplicate_remaps_members_and_pins_and_member_structure_requires_unpack() {
	let mut session = session();
	let (root, body) = instantiate(&mut session, source());
	let original = session.authored_document();
	execute(
		&mut session,
		Command::Select {
			keys: vec![root.as_str().into()],
		},
	)
	.unwrap();
	execute(
		&mut session,
		Command::DuplicateSelection {
			offset: [3.0, 0.0, 0.0],
		},
	)
	.unwrap();
	let data = session.authored_document();
	let links: Vec<_> = data
		.entities
		.iter()
		.filter_map(|entity| entity.linked_prefab.as_ref())
		.collect();
	assert_eq!(links.len(), 2);
	assert_ne!(links[0].entity_map, links[1].entity_map);
	assert_eq!(links[0].source, links[1].source);
	assert!(links[1].overrides.is_empty());
	data.validate().unwrap();
	drop(data);
	execute(&mut session, Command::DeleteSelection).unwrap();
	assert_eq!(session.authored_document().as_ref(), original.as_ref());
	execute(&mut session, Command::Undo).unwrap();
	assert_eq!(
		session
			.authored_document()
			.entities
			.iter()
			.filter(|entity| entity.linked_prefab.is_some())
			.count(),
		2
	);
	execute(&mut session, Command::Undo).unwrap();
	assert_eq!(session.authored_document().as_ref(), original.as_ref());
	execute(
		&mut session,
		Command::Select {
			keys: vec![body.as_str().into()],
		},
	)
	.unwrap();
	let revision = session.revision();
	assert!(
		execute(&mut session, Command::DeleteSelection)
			.unwrap_err()
			.contains("Unpack")
	);
	assert_eq!(session.revision(), revision);
	assert!(
		execute(
			&mut session,
			Command::DuplicateSelection { offset: [0.0; 3] }
		)
		.is_err()
	);
	execute(
		&mut session,
		Command::UnpackLinkedPrefab {
			root_key: root.as_str().into(),
		},
	)
	.unwrap();
	execute(&mut session, Command::DeleteSelection).unwrap();
	assert!(
		session
			.authored_document()
			.entities
			.iter()
			.all(|entity| entity.linked_prefab.is_none())
	);
}

#[test]
fn canonical_pin_roundtrip_is_self_contained_and_scene_runtime_resolution_drops_authoring_links() {
	let mut session = session();
	let (_, body) = instantiate(&mut session, source());
	execute(
		&mut session,
		Command::Rename {
			key: body.as_str().into(),
			name: "Pinned local".into(),
		},
	)
	.unwrap();
	let data = session.authored_document();
	let json = data.to_json().unwrap();
	let value: serde_json::Value = serde_json::from_str(&json).unwrap();
	let pin = value["entities"]
		.as_array()
		.unwrap()
		.iter()
		.find_map(|entity| entity.get("linked_prefab"))
		.unwrap();
	let source_entity = &pin["source"]["prefab"]["entities"][0];
	assert!(source_entity.get("components").is_some());
	assert!(source_entity.get("component_views").is_none());
	assert!(source_entity.get("model_path").is_none());
	let loaded = engine_core::SceneData::from_json(&json).unwrap();
	assert_eq!(loaded, *data);
	assert_eq!(
		engine_core::linked_prefab::resolve_scene(&loaded)
			.entities
			.iter()
			.filter(|entity| entity.linked_prefab.is_some())
			.count(),
		0
	);
}

#[test]
fn source_structural_update_keeps_stable_member_ids_and_confirms_removed_overrides() {
	let mut session = session();
	let original = source();
	let (root, body) = instantiate(&mut session, original.clone());
	let mut next = original;
	next.fingerprint = "4".repeat(64);
	next.prefab.entities.push(entity("added", [0.0; 3]));
	update(
		&mut session,
		&root,
		next.clone(),
		PrefabConflictResolution::Reject,
	)
	.unwrap();
	assert!(
		session
			.authored_document()
			.entities
			.iter()
			.any(|entity| entity.key == body)
	);
	execute(
		&mut session,
		Command::Rename {
			key: body.as_str().into(),
			name: "Keep".into(),
		},
	)
	.unwrap();
	next.prefab
		.entities
		.retain(|entity| entity.key != key("body") && entity.key != key("child"));
	next.fingerprint = "5".repeat(64);
	assert!(
		update(
			&mut session,
			&root,
			next.clone(),
			PrefabConflictResolution::KeepLocal
		)
		.is_err()
	);
	update(
		&mut session,
		&root,
		next,
		PrefabConflictResolution::UseSource,
	)
	.unwrap();
	assert!(
		!session
			.authored_document()
			.entities
			.iter()
			.any(|entity| entity.key == body)
	);
}

#[test]
fn stale_requests_play_guard_and_repeated_noop_updates_preserve_history() {
	let mut session = session();
	let source = source();
	let (root, _) = instantiate(&mut session, source.clone());
	let revision = session.revision();
	update(
		&mut session,
		&root,
		source.clone(),
		PrefabConflictResolution::Reject,
	)
	.unwrap();
	assert_eq!(session.revision(), revision);
	let stale = EditorCommandRequest {
		expected_revision: revision - 1,
		command: Command::UnpackLinkedPrefab {
			root_key: root.as_str().into(),
		},
	};
	assert!(session.execute_command(stale).is_err());
	assert_eq!(session.revision(), revision);
	execute(&mut session, Command::StartPlay).unwrap();
	let playing = session.revision();
	assert!(
		execute(
			&mut session,
			Command::UnpackLinkedPrefab {
				root_key: root.as_str().into()
			}
		)
		.is_err()
	);
	assert_eq!(session.revision(), playing);
	execute(&mut session, Command::StopPlay).unwrap();
	assert!(
		session
			.authored_document()
			.entities
			.iter()
			.any(|entity| entity.linked_prefab.is_some())
	);
	execute(&mut session, Command::Undo).unwrap();
	assert!(session.authored_document().entities.is_empty());
}

#[test]
fn declared_reference_slots_cannot_be_used_as_external_prefab_parents() {
	let mut source = source();
	source
		.prefab
		.reference_slots
		.insert("actor".into(), key("external-actor"));
	source.prefab.entities[0].parent = Some(key("external-actor"));
	assert!(
		source
			.prefab
			.validate()
			.unwrap_err()
			.contains("parents must be owned")
	);
}

#[test]
fn linked_interactable_roundtrips_and_requires_source_or_unpack_to_change() {
	use engine_core::components::{
		ComponentData, ComponentId, ComponentRecord, InteractableSettings,
	};
	let id = ComponentId::new("use").unwrap();
	let authored = ComponentRecord {
		id: id.clone(),
		schema_version: 1,
		enabled: true,
		data: ComponentData::Interactable {
			settings: InteractableSettings {
				action: "interact".into(),
				reach: 1.25,
			},
		},
	};
	let mut source = source();
	source.prefab.entities[0]
		.put_component(authored, false)
		.unwrap();
	let encoded = source.prefab.to_json().unwrap();
	source.prefab = PrefabData::from_json(&encoded).unwrap();
	let mut session = session();
	let (root, body) = instantiate(&mut session, source);
	let before = session.authored_document();
	let reopened = engine_core::SceneData::from_json(&before.to_json().unwrap()).unwrap();
	let member = reopened
		.entities
		.iter()
		.find(|entity| entity.key == body)
		.unwrap();
	assert_eq!(member.interactable.as_ref().unwrap().settings.reach, 1.25);
	let disable = || Command::SetComponentEnabled {
		key: body.as_str().into(),
		component_id: id.clone(),
		enabled: false,
	};
	assert!(
		execute(&mut session, disable())
			.unwrap_err()
			.contains("prefab source")
	);
	assert_eq!(session.authored_document(), before);
	execute(
		&mut session,
		Command::UnpackLinkedPrefab {
			root_key: root.as_str().into(),
		},
	)
	.unwrap();
	execute(&mut session, disable()).unwrap();
	let document = session.authored_document();
	assert!(
		!document
			.entities
			.iter()
			.find(|entity| entity.key == body)
			.unwrap()
			.interactable
			.as_ref()
			.unwrap()
			.enabled
	);
}
