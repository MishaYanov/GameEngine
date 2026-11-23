use super::*;
use bridge::viewport::PropPreviewTargetDto;
use editor::prefab_sources::{PrefabSourceDraftDto, PropDraftEdit};
use editor::prop_preview::PropPreviewEnvironment;

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
	assert!(workspace
		.open_prop_preview(target, revision, generation, 1)
		.is_err());
	let frame = workspace.active_prop_preview().unwrap();
	assert_eq!(frame.frame_request, 0);
	assert!(frame.editable_source);
	assert!(workspace.prop_scene_environment(&preview.token).is_none());
	workspace
		.set_prop_preview_environment(&preview.token, PropPreviewEnvironment::Scene)
		.unwrap();
	let environment = workspace.prop_scene_environment(&preview.token).unwrap();
	assert!(environment.sky.is_none());
	assert!(environment.lights.is_empty());
	let selected = workspace
		.select_prop_preview(&preview.token, Some(EntityKey::new("a").unwrap()))
		.unwrap();
	assert_eq!(
		selected.selection.as_ref().map(EntityKey::as_str),
		Some("a")
	);
	assert!(workspace
		.select_prop_preview(&preview.token, Some(EntityKey::new("other").unwrap()))
		.is_err());
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
	let frame = workspace.active_prop_preview().unwrap();
	assert!(!frame.editable_source);
	let render_document = frame.document;
	assert_eq!(render_document.entities.len(), 2);
	assert!(render_document
		.entities
		.iter()
		.all(|entity| entity.linked_prefab.is_none()));
	let anchor = render_document
		.entities
		.iter()
		.find(|entity| entity.key == root)
		.unwrap();
	assert_eq!(anchor.transform.position, [0.0; 3]);
	assert!(render_document
		.entities
		.iter()
		.all(|entity| entity.key.as_str() != "a"));
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
	assert!(workspace
		.open_prop_preview(target.clone(), revision(&workspace), wrong_generation, 3)
		.is_err());
	assert!(workspace
		.open_prop_preview(target.clone(), 0, workspace.generation, 1)
		.is_err());
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
	assert!(workspace
		.open_prop_preview(target, revision(&workspace), workspace.generation, 3)
		.is_err());
	run(&mut workspace, Command::StopPlay);
	assert!(workspace.active_prop_preview().is_none());
	assert!(!workspace.close_prop_preview(&preview.token));
}

#[test]
fn a_published_primitive_keeps_its_model_and_bounds_in_prop_preview_and_play() {
	use runtime::{
		asset_registry::AssetRef,
		assets::{
			project_assets::{prepare_primitive, resolve_asset, PreparedPrimitive},
			AssetManager,
		},
		world::RuntimeWorld,
		Bounds3, PrimitiveKind,
	};

	let fixture = Fixture::new();
	fixture.write_project("scenes/main.json");
	let mut workspace = fixture.open();
	let previous_generation = workspace.generation;
	let bytes = include_bytes!("../../assets/primitives/cube.glb");
	let PreparedPrimitive::New(change) =
		prepare_primitive(&workspace.asset_root, PrimitiveKind::Cube, bytes).unwrap()
	else {
		panic!("this project has no primitive asset yet");
	};
	let asset = change.publish().unwrap();
	workspace.asset_published().unwrap();
	workspace
		.import_model(&asset.path, [0.0; 3], revision(&workspace))
		.unwrap();
	let draft = workspace
		.create_linked_prefab_source(PROP_PATH, Default::default(), revision(&workspace))
		.unwrap();
	let placement = workspace
		.instantiate_linked_source(
			PROP_PATH,
			&draft.draft_id,
			[8.0, 2.0, -4.0],
			Default::default(),
			revision(&workspace),
		)
		.unwrap();
	let root = EntityKey::new(&placement.selection[0]).unwrap();
	workspace.save_all(revision(&workspace)).unwrap();
	drop(workspace);

	let mut reopened = fixture.open();
	let authored = reopened.backend.session().authored_document();
	let reopened_source = reopened
		.open_prefab_source(PROP_PATH, revision(&reopened))
		.unwrap();
	assert_eq!(reopened_source.asset_id, draft.asset_id);
	assert_eq!(reopened_source.prefab, draft.prefab);

	let target = PropPreviewTargetDto::Instance {
		scene_id: placement.active_scene_id,
		root_key: root.clone(),
	};
	assert!(reopened
		.open_prop_preview(target.clone(), revision(&reopened), previous_generation, 1)
		.is_err());
	let preview = reopened
		.open_prop_preview(target, revision(&reopened), reopened.generation, 2)
		.unwrap();
	let frame = reopened.active_prop_preview().unwrap();
	assert!(!frame.editable_source);
	let preview_world = RuntimeWorld::new(frame.document).unwrap();
	let root_index = preview_world.entity_index(&root).unwrap();
	assert_eq!(
		preview_world.data().entities[root_index].transform.position,
		[0.0; 3]
	);
	let member = preview_world
		.data()
		.entities
		.iter()
		.find(|entity| entity.parent.as_ref() == Some(&root) && entity.renderable)
		.unwrap()
		.key
		.clone();
	reopened
		.select_prop_preview(&preview.token, Some(member.clone()))
		.unwrap();
	reopened.frame_prop_preview(&preview.token).unwrap();
	assert_eq!(reopened.active_prop_preview().unwrap().frame_request, 1);

	let mut assets = AssetManager::new(&reopened.asset_root).unwrap();
	let model_bounds = |world: &RuntimeWorld, assets: &mut AssetManager| {
		let index = world.entity_index(&member).unwrap();
		let entity = &world.data().entities[index];
		assert_eq!(entity.asset_id.as_ref(), Some(&asset.id));
		assert_eq!(entity.model_path, asset.path);
		let reference = AssetRef::new(&entity.model_path, entity.mesh_index)
			.unwrap()
			.with_asset_id(entity.asset_id.clone());
		let resolved = resolve_asset(&fixture.0, &reference).unwrap();
		let imported = assets.load_scene(&resolved.path).unwrap();
		imported.mesh_bounds[resolved.mesh_index.unwrap_or(0)]
			.unwrap()
			.transformed(world.world_matrices()[index])
			.unwrap()
	};
	assert_eq!(
		model_bounds(&preview_world, &mut assets),
		Bounds3::new([-0.5; 3], [0.5; 3]).unwrap()
	);

	run(&mut reopened, Command::StartPlay);
	assert!(reopened.active_prop_preview().is_none());
	let play_world = reopened.backend.session().render_world().unwrap();
	assert_eq!(
		model_bounds(play_world, &mut assets),
		Bounds3::new([7.5, 1.5, -4.5], [8.5, 2.5, -3.5]).unwrap()
	);
	run(&mut reopened, Command::StopPlay);
	assert_eq!(reopened.backend.session().authored_document(), authored);
}

#[test]
fn accepted_draft_edits_refresh_the_pinned_preview_and_keep_the_scene_and_disk_unchanged() {
	let fixture = Fixture::new();
	let mut workspace = fixture.open();
	let draft = create_source(&mut workspace);
	let path = fixture.0.join(PROP_PATH);
	let bytes = fs::read(&path).unwrap();
	let scene = serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap();
	let preview = workspace
		.open_prop_preview(
			PropPreviewTargetDto::Source {
				draft_id: draft.draft_id.clone(),
			},
			revision(&workspace),
			workspace.generation,
			1,
		)
		.unwrap();
	let mut candidate = draft.prefab.clone();
	candidate["entities"][0]["name"] = serde_json::json!("Edited local source");
	let applied = workspace
		.edit_prop_draft(
			&draft.draft_id,
			draft.draft_revision,
			PropDraftEdit::Apply { prefab: candidate },
		)
		.unwrap();
	assert!(applied.dirty && applied.can_undo && !applied.can_redo);
	let frame = workspace.active_prop_preview().unwrap();
	assert_eq!(frame.state.token, preview.token);
	assert_eq!(frame.content_revision, applied.draft_revision);
	assert_eq!(frame.document.entities[0].name, "Edited local source");
	assert!(!workspace.prop_preview_content_is_current(&preview.token, draft.draft_revision));
	assert!(workspace
		.edit_prop_draft(&draft.draft_id, draft.draft_revision, PropDraftEdit::Undo)
		.is_err());
	assert!(workspace.close_prefab_source(&draft.draft_id).is_err());
	assert!(workspace
		.reload_with_policy(revision(&workspace), ReloadPolicy::DiscardUnsaved)
		.is_err());
	assert_eq!(fs::read(&path).unwrap(), bytes);
	assert_eq!(
		serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap(),
		scene
	);
	let undone = workspace
		.edit_prop_draft(&draft.draft_id, applied.draft_revision, PropDraftEdit::Undo)
		.unwrap();
	assert!(!undone.dirty && undone.can_redo);
	assert_eq!(undone.prefab, draft.prefab);
	let redone = workspace
		.edit_prop_draft(&draft.draft_id, undone.draft_revision, PropDraftEdit::Redo)
		.unwrap();
	assert!(redone.dirty);
	let discarded = workspace
		.edit_prop_draft(
			&draft.draft_id,
			redone.draft_revision,
			PropDraftEdit::Discard,
		)
		.unwrap();
	assert!(!discarded.dirty && !discarded.can_undo && !discarded.can_redo);
	assert_eq!(discarded.prefab, draft.prefab);
	assert_eq!(fs::read(&path).unwrap(), bytes);
	workspace.close_prefab_source(&draft.draft_id).unwrap();
}

#[test]
fn prop_save_updates_source_without_updating_existing_pins_and_refuses_external_changes() {
	let fixture = Fixture::new();
	let mut workspace = fixture.open();
	let draft = create_source(&mut workspace);
	let placed = workspace
		.instantiate_linked_source(
			PROP_PATH,
			&draft.draft_id,
			[4.0, 0.0, 0.0],
			Default::default(),
			revision(&workspace),
		)
		.unwrap();
	let root = &placed.selection[0];
	let mut candidate = draft.prefab.clone();
	candidate["entities"][0]["name"] = serde_json::json!("Saved Prop part");
	let applied = workspace
		.edit_prop_draft(
			&draft.draft_id,
			draft.draft_revision,
			PropDraftEdit::Apply { prefab: candidate },
		)
		.unwrap();
	let saved = workspace
		.save_prop_draft(&draft.draft_id, applied.draft_revision)
		.unwrap();
	assert!(!saved.draft.dirty);
	assert!(saved.draft.can_undo);
	assert_eq!(saved.snapshot.revision, placed.revision);
	assert_eq!(saved.snapshot.document, placed.document);
	assert!(
		workspace
			.linked_prefab_status(root, revision(&workspace))
			.unwrap()
			.unwrap()
			.source_changed
	);
	let mut candidate = saved.draft.prefab.clone();
	candidate["entities"][0]["name"] = serde_json::json!("Local after save");
	let applied = workspace
		.edit_prop_draft(
			&draft.draft_id,
			applied.draft_revision,
			PropDraftEdit::Apply { prefab: candidate },
		)
		.unwrap();
	let path = fixture.0.join(PROP_PATH);
	let mut external = fs::read(&path).unwrap();
	external.extend_from_slice(b"\n");
	fs::write(&path, &external).unwrap();
	assert!(workspace
		.save_prop_draft(&draft.draft_id, applied.draft_revision)
		.err()
		.unwrap()
		.contains("changed on disk"));
	assert_eq!(fs::read(&path).unwrap(), external);
	let undo = workspace
		.edit_prop_draft(&draft.draft_id, applied.draft_revision, PropDraftEdit::Undo)
		.unwrap();
	assert_eq!(undo.prefab, saved.draft.prefab);
}

#[test]
fn position_gesture_is_one_owned_draft_edit_and_rejects_obsolete_releases() {
	let fixture = Fixture::new();
	let mut workspace = fixture.open();
	let draft = create_source(&mut workspace);
	let preview = workspace
		.open_prop_preview(
			PropPreviewTargetDto::Source {
				draft_id: draft.draft_id.clone(),
			},
			revision(&workspace),
			workspace.generation,
			1,
		)
		.unwrap();
	let child = EntityKey::new("a").unwrap();
	assert!(workspace
		.apply_prop_preview_position(&preview.token, draft.draft_revision, &child, [1.0; 3])
		.is_err());
	workspace
		.select_prop_preview(&preview.token, Some(child.clone()))
		.unwrap();
	let moved = workspace
		.apply_prop_preview_position(&preview.token, draft.draft_revision, &child, [1.0; 3])
		.unwrap();
	assert_eq!(moved.draft_revision, draft.draft_revision + 1);
	assert_eq!(
		workspace.active_prop_preview().unwrap().document.entities[0]
			.transform
			.position,
		[1.0; 3]
	);
	assert!(workspace
		.apply_prop_preview_position(&preview.token, draft.draft_revision, &child, [3.0; 3])
		.is_err());
	let undone = workspace
		.edit_prop_draft(&draft.draft_id, moved.draft_revision, PropDraftEdit::Undo)
		.unwrap();
	assert!(!undone.dirty && !undone.can_undo && undone.can_redo);
	assert_eq!(undone.prefab, draft.prefab);
	assert_eq!(revision(&workspace), preview.revision);
	workspace.close_prop_preview(&preview.token);
	assert!(workspace
		.apply_prop_preview_position(&preview.token, undone.draft_revision, &child, [1.0; 3])
		.is_err());
}
