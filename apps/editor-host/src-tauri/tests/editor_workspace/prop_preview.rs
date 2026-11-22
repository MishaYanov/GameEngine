use super::*;
use bridge::viewport::PropPreviewTargetDto;
use editor::prefab_sources::PrefabSourceDraftDto;

const PROP_PATH: &str = "prefabs/table.json";

fn revision(workspace: &EditorWorkspace) -> u64 {
	workspace.backend.session().revision()
}

fn create_source(workspace: &mut EditorWorkspace) -> PrefabSourceDraftDto {
	run(
		workspace,
		Command::Select {
			keys: vec!["a".into()],
		},
	);
	workspace
		.create_linked_prefab_source(PROP_PATH, Default::default(), revision(workspace))
		.unwrap()
}

#[test]
fn source_preview_selection_and_frame_are_scoped_without_editing_the_scene() {
	let fixture = Fixture::new();
	let mut workspace = fixture.open();
	let draft = create_source(&mut workspace);
	run(&mut workspace, Command::Select { keys: vec![] });
	let before = serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap();
	let target = PropPreviewTargetDto::Source {
		draft_id: draft.draft_id.clone(),
	};
	let generation = workspace.generation;
	let revision = revision(&workspace);
	let preview = workspace
		.open_prop_preview(target.clone(), revision, generation, 2)
		.unwrap();
	assert!(
		workspace
			.open_prop_preview(target, revision, generation, 1)
			.is_err()
	);
	assert_eq!(workspace.active_prop_preview().unwrap().frame_request, 0);
	let selected = workspace
		.select_prop_preview(&preview.token, Some(EntityKey::new("a").unwrap()))
		.unwrap();
	assert_eq!(
		selected.selection.as_ref().map(EntityKey::as_str),
		Some("a")
	);
	assert!(
		workspace
			.select_prop_preview(&preview.token, Some(EntityKey::new("other").unwrap()))
			.is_err()
	);
	workspace.frame_prop_preview(&preview.token).unwrap();
	assert_eq!(workspace.active_prop_preview().unwrap().frame_request, 1);
	assert_eq!(
		serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap(),
		before
	);
	workspace.close_prefab_source(&draft.draft_id).unwrap();
	assert!(workspace.discard_stale_prop_preview());
	assert!(workspace.frame_prop_preview(&preview.token).is_err());
	assert!(workspace.active_prop_preview().is_none());
}

#[test]
fn pinned_inspection_stays_available_without_its_source_and_invalidates_on_scene_edits() {
	let fixture = Fixture::new();
	let mut workspace = fixture.open();
	let draft = create_source(&mut workspace);
	let placement = workspace
		.instantiate_linked_source(
			PROP_PATH,
			&draft.draft_id,
			[12.0, 0.0, 0.0],
			Default::default(),
			revision(&workspace),
		)
		.unwrap();
	let root = EntityKey::new(&placement.selection[0]).unwrap();
	fs::remove_file(fixture.0.join(PROP_PATH)).unwrap();
	let target = PropPreviewTargetDto::Instance {
		scene_id: placement.active_scene_id.clone(),
		root_key: root.clone(),
	};
	let preview = workspace
		.open_prop_preview(
			target.clone(),
			revision(&workspace),
			workspace.generation,
			1,
		)
		.unwrap();
	let render_document = workspace.active_prop_preview().unwrap().document;
	assert_eq!(render_document.entities.len(), 2);
	assert!(
		render_document
			.entities
			.iter()
			.all(|entity| entity.linked_prefab.is_none())
	);
	let anchor = render_document
		.entities
		.iter()
		.find(|entity| entity.key == root)
		.unwrap();
	assert_eq!(anchor.transform.position, [0.0; 3]);
	assert!(
		render_document
			.entities
			.iter()
			.all(|entity| entity.key.as_str() != "a")
	);
	let newer = workspace
		.open_prop_preview(target, revision(&workspace), workspace.generation, 2)
		.unwrap();
	assert!(!workspace.close_prop_preview(&preview.token));
	assert!(workspace.prop_preview_is_current(&newer.token));
	run(
		&mut workspace,
		Command::Select {
			keys: vec!["a".into()],
		},
	);
	assert!(workspace.prop_preview_is_current(&newer.token));
	run(
		&mut workspace,
		Command::Rename {
			key: root.as_str().into(),
			name: "Changed instance".into(),
		},
	);
	assert!(workspace.active_prop_preview().is_none());
	assert!(workspace.select_prop_preview(&newer.token, None).is_err());
}

#[test]
fn opening_a_preview_checks_the_workspace_and_play_discards_the_preview() {
	let fixture = Fixture::new();
	let mut workspace = fixture.open();
	let draft = create_source(&mut workspace);
	let target = PropPreviewTargetDto::Source {
		draft_id: draft.draft_id,
	};
	let wrong_generation = workspace.generation + 1;
	assert!(
		workspace
			.open_prop_preview(target.clone(), revision(&workspace), wrong_generation, 3)
			.is_err()
	);
	assert!(
		workspace
			.open_prop_preview(target.clone(), 0, workspace.generation, 1)
			.is_err()
	);
	let preview = workspace
		.open_prop_preview(
			target.clone(),
			revision(&workspace),
			workspace.generation,
			2,
		)
		.unwrap();
	run(&mut workspace, Command::StartPlay);
	assert!(workspace.active_prop_preview().is_none());
	assert!(
		workspace
			.open_prop_preview(target, revision(&workspace), workspace.generation, 3)
			.is_err()
	);
	run(&mut workspace, Command::StopPlay);
	assert!(workspace.active_prop_preview().is_none());
	assert!(!workspace.close_prop_preview(&preview.token));
}
