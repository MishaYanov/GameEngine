use super::*;

#[test]
fn component_roundtrip_and_disabled_parented_authoring_preserve_settings() {
	let mut data = room(vec![]);
	let record = data.entities[0]
		.component_records()
		.into_iter()
		.find(|r| matches!(r.data, ComponentData::CharacterController { .. }))
		.unwrap();
	assert_eq!(
		serde_json::from_str::<ComponentRecord>(&serde_json::to_string(&record).unwrap()).unwrap(),
		record
	);
	let json = data.to_json().unwrap();
	assert_eq!(engine_core::SceneData::from_json(&json).unwrap(), data);
	assert!(!json.contains("velocity"));
	data.entities[0].parent = Some(key("floor"));
	assert!(
		SimulationSession::new(project(&data), Arc::new(data.clone()))
			.err()
			.unwrap()
			.contains("detach")
	);
	data.entities[0]
		.character_controller
		.as_mut()
		.unwrap()
		.enabled = false;
	let disabled = session(data);
	assert!(disabled.character_motion().is_empty());
}

#[test]
fn second_movement_authority_and_invalid_body_are_rejected() {
	let data = room(vec![]);
	let mut p = project(&data);
	p.add_behavior(BehaviorSpec::ControlledMove {
		entity: key("player"),
		speed: 1.,
		space: BehaviorSpace::World,
	});
	assert!(
		SimulationSession::new(p, Arc::new(data.clone()))
			.err()
			.unwrap()
			.contains("owns")
	);
	let mut parented = data;
	parented.entities[0].transform.scale = [2.; 3];
	assert!(SimulationSession::new(project(&parented), Arc::new(parented)).is_err());
}

#[test]
fn linked_controller_needs_unpack_and_detach_and_disabled_record_is_preserved() {
	use engine_core::{
		asset_catalog::AssetId,
		command::{EditorCommandDto as C, EditorCommandRequest},
		editor::{EditorSession, PrefabData},
		linked_prefab::LinkedPrefabSource,
	};
	let data = scene(vec![box_at("floor", [0., -0.5, 0.], [10., 0.5, 10.])]);
	let mut editor = EditorSession::for_project(data.clone(), project(&data)).unwrap();
	fn run(editor: &mut EditorSession, command: C) -> Result<(), String> {
		editor
			.execute_command(EditorCommandRequest {
				expected_revision: editor.revision(),
				command,
			})
			.map(|_| ())
	}
	let source = LinkedPrefabSource {
		source_id: AssetId::generate(),
		path: "prefabs/player.json".into(),
		fingerprint: "a".repeat(64),
		prefab: PrefabData {
			schema_version: 3,
			composition: Default::default(),
			entities: vec![player()],
			gameplay: None,
			reference_slots: Default::default(),
		},
	};
	run(
		&mut editor,
		C::InstantiateLinkedPrefab {
			source,
			position: [0.; 3],
			bindings: Default::default(),
		},
	)
	.unwrap();
	let document = editor.authored_document();
	let anchor = document
		.entities
		.iter()
		.find(|e| e.linked_prefab.is_some())
		.unwrap()
		.key
		.clone();
	let body = document
		.entities
		.iter()
		.find(|e| e.character_controller.is_some())
		.unwrap()
		.key
		.clone();
	let json = document.to_json().unwrap();
	assert_eq!(
		&engine_core::SceneData::from_json(&json).unwrap(),
		document.as_ref()
	);
	assert!(
		run(&mut editor, C::StartPlay)
			.unwrap_err()
			.contains("detach")
	);
	run(
		&mut editor,
		C::UnpackLinkedPrefab {
			root_key: anchor.as_str().into(),
		},
	)
	.unwrap();
	assert!(
		run(&mut editor, C::StartPlay)
			.unwrap_err()
			.contains("detach")
	);
	run(
		&mut editor,
		C::Reparent {
			key: body.as_str().into(),
			parent: None,
			keep_world: true,
		},
	)
	.unwrap();
	run(&mut editor, C::StartPlay).unwrap();
	editor.tick(1. / 60.).unwrap();
	assert_eq!(
		editor
			.runtime_inspection(0, 128)
			.unwrap()
			.character_motion
			.len(),
		1
	);
	run(&mut editor, C::StopPlay).unwrap();
	assert_eq!(
		editor
			.authored_document()
			.entities
			.iter()
			.find(|e| e.key == body)
			.unwrap()
			.transform
			.position,
		[0., 0.901, 0.]
	);
}

#[test]
fn modules_cannot_enable_a_second_body_under_the_controller() {
	let mut child = box_at("child", [0.; 3], [0.1; 3]);
	child.parent = Some(key("player"));
	child.collision_enabled = false;
	let data = room(vec![child]);
	let script = BoundaryScript::default();
	let mut s = SimulationSession::with_factory(project(&data), Arc::new(data), &script).unwrap();
	tick(&mut s);
	let before = s.world().data().clone();
	*script.0.lock().unwrap() = Some(BoundaryAction::Collider(key("child"), true));
	assert!(s.tick(1. / 60.).unwrap_err().contains("owns"));
	assert_eq!(&before, s.world().data());
}
