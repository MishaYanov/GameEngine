use super::support::{close, entity, key, scene};
use super::{run, select};
use bridge::editor::EditorCommandDto as Command;
use runtime::editor::{EditorSession, PrefabData};

#[test]
fn prefab_export_includes_subtrees_remaps_keys_and_undo_redo_restores_the_same_instance() {
	let child = entity("child", [2.0, 0.0, 0.0]).with_parent(Some(key("parent")));
	let mut s = EditorSession::new(scene(vec![entity("parent", [5.0, 0.0, 0.0]), child])).unwrap();
	select(&mut s, &["parent"]);
	let original = s.authored_document();
	let prefab = s.export_prefab().unwrap();
	assert_eq!(prefab.entities.len(), 2);
	assert_eq!(prefab.entities[0].transform.position, [0.0; 3]);
	let serialized = prefab.to_json().unwrap();
	let prefab = PrefabData::from_json(&serialized).unwrap();
	s.instantiate_prefab(prefab, [10.0, 0.0, 0.0]).unwrap();
	let instantiated = s.authored_document();
	assert_eq!(instantiated.entities.len(), 4);
	assert_ne!(instantiated.entities[2].key, key("parent"));
	assert_eq!(
		instantiated.entities[3].parent,
		Some(instantiated.entities[2].key.clone())
	);
	close(instantiated.world_matrices().unwrap()[3].w_axis.x, 12.0);
	assert_eq!(s.selection(), &[instantiated.entities[2].key.clone()]);
	run(&mut s, Command::Undo);
	assert_eq!(s.authored_document(), original);
	run(&mut s, Command::Redo);
	assert_eq!(s.authored_document(), instantiated);
}

#[test]
fn prefab_detaches_external_parent_and_invalid_prefabs_do_not_change_document_or_redo() {
	let child = entity("child", [2.0, 0.0, 0.0]).with_parent(Some(key("parent")));
	let mut s = EditorSession::new(scene(vec![entity("parent", [5.0, 0.0, 0.0]), child])).unwrap();
	select(&mut s, &["child"]);
	let exported = s.export_prefab().unwrap();
	assert_eq!(exported.entities.len(), 1);
	assert!(exported.entities[0].parent.is_none());
	run(
		&mut s,
		Command::Rename {
			key: "child".into(),
			name: "Redo".into(),
		},
	);
	run(&mut s, Command::Undo);
	let before = s.authored_document();
	for prefab in [
		PrefabData {
			reference_slots: Default::default(),
			schema_version: 1,
			gameplay: None,
			entities: vec![],
		},
		PrefabData {
			reference_slots: Default::default(),
			schema_version: u32::MAX,
			gameplay: None,
			entities: exported.entities.clone(),
		},
		PrefabData {
			reference_slots: Default::default(),
			schema_version: 1,
			gameplay: None,
			entities: vec![entity("child", [0.0; 3]).with_parent(Some(key("outside")))],
		},
	] {
		assert!(s.instantiate_prefab(prefab, [0.0; 3]).is_err());
		assert_eq!(s.authored_document(), before);
		assert!(s.snapshot().unwrap().can_redo);
	}
	assert!(s.instantiate_prefab(exported, [f32::NAN; 3]).is_err());
	run(&mut s, Command::Redo);
	assert_eq!(s.authored_document().entities[1].name, "Redo");
}
