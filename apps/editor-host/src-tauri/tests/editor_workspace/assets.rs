use super::*;

#[test]
fn prefab_save_replaces_file_load_is_one_undo_and_stale_request_preserves_it() {
	let fixture = Fixture::new();
	let mut w = fixture.open();
	run(
		&mut w,
		Command::Select {
			keys: vec!["a".into()],
		},
	);
	let revision = w.backend.session().revision();
	w.save_prefab("prefabs/one.json", revision).unwrap();
	let path = fixture.0.join("prefabs/one.json");
	fs::write(&path, "old contents").unwrap();
	w.save_prefab("prefabs/one.json", revision).unwrap();
	let bytes = fs::read(&path).unwrap();
	assert!(w.save_prefab("prefabs/one.json", revision - 1).is_err());
	assert_eq!(fs::read(&path).unwrap(), bytes);
	let before = w.backend.session().authored_document();
	w.load_prefab("prefabs/one.json", [4.0, 0.0, 0.0], revision)
		.unwrap();
	assert_eq!(w.backend.session().authored_document().entities.len(), 2);
	let inserted = w.backend.session().selection()[0].clone();
	assert_ne!(inserted, EntityKey::new("a").unwrap());
	run(&mut w, Command::Undo);
	assert_eq!(w.backend.session().authored_document(), before);
	run(&mut w, Command::Redo);
	assert_eq!(w.backend.session().selection(), &[inserted]);
	assert_eq!(fs::read_dir(fixture.0.join("prefabs")).unwrap().count(), 1);
}

#[test]
fn persistent_assets_and_gameplay_prefab_copies_save_reopen_and_follow_supported_moves() {
	use runtime::{
		asset_registry::AssetRef,
		assets::project_assets,
		project::{BehaviorSpace, BehaviorSpec},
	};
	let fixture = Fixture::new();
	let manifest = fixture.write_project("scenes/main.json");
	let catalog = project_assets::migrate_catalog(&fixture.0).unwrap();
	let model = catalog.by_path("models/triangle.glb").unwrap().clone();
	let mut w = fixture.open();
	let before = w.backend.session().authored_document();
	w.import_model(
		"models/triangle.glb",
		[2., 0., 0.],
		w.backend.session().revision(),
	)
	.unwrap();
	assert!(w
		.backend
		.session()
		.authored_document()
		.entities
		.iter()
		.skip(before.entities.len())
		.all(|entity| entity.asset_id.as_ref() == Some(&model.id)));
	run(&mut w, Command::Undo);
	assert_eq!(w.backend.session().authored_document(), before);
	let mut project = w.backend.session().project().clone();
	project.add_behavior(BehaviorSpec::ControlledMove {
		entity: EntityKey::new("a").unwrap(),
		speed: 3.,
		space: BehaviorSpace::World,
	});
	run(&mut w, Command::SetProject { project });
	run(
		&mut w,
		Command::Select {
			keys: vec!["a".into()],
		},
	);
	w.save_prefab("prefabs/assembly.json", w.backend.session().revision())
		.unwrap();
	let saved_catalog = project_assets::load_catalog(&fixture.0).unwrap();
	let prefab = saved_catalog.by_path("prefabs/assembly.json").unwrap();
	assert_eq!(prefab.dependencies, vec![model.id.clone()]);
	let prefab_id = prefab.id.clone();
	assert_eq!(
		w.used_asset_meshes(&model.id).unwrap(),
		std::collections::HashSet::from([0])
	);
	let authored = w.backend.session().authored_document();
	let project = w.backend.session().project().clone();
	let dirty = w.backend.snapshot().unwrap();
	let revision = w.backend.session().revision();
	project_assets::prepare_move(&fixture.0, &model.id, "models/moved.glb")
		.unwrap()
		.publish()
		.unwrap();
	let refreshed = w.asset_published().unwrap();
	assert_eq!(refreshed.revision, revision + 1);
	assert_eq!(refreshed.dirty, dirty.dirty);
	assert_eq!(refreshed.project_dirty, dirty.project_dirty);
	assert!(refreshed.can_undo);
	assert_eq!(w.backend.session().authored_document(), authored);
	assert_eq!(w.backend.session().project(), &project);
	project_assets::prepare_move(&fixture.0, &prefab_id, "prefabs/moved.json")
		.unwrap()
		.publish()
		.unwrap();
	w.asset_published().unwrap();
	let mut copies = Vec::new();
	for position in [[4., 0., 0.], [8., 0., 0.]] {
		w.load_prefab(
			"prefabs/assembly.json",
			position,
			w.backend.session().revision(),
		)
		.unwrap();
		copies.push(w.backend.session().selection()[0].clone());
	}
	assert_ne!(copies[0], copies[1]);
	assert!(copies.iter().all(|key| key.as_str() != "a"));
	let targets: std::collections::HashSet<_> = w
		.backend
		.session()
		.project()
		.behaviors
		.iter()
		.map(|behavior| behavior.entity().clone())
		.collect();
	assert_eq!(
		targets,
		std::collections::HashSet::from([
			EntityKey::new("a").unwrap(),
			copies[0].clone(),
			copies[1].clone()
		])
	);
	run(
		&mut w,
		Command::Rename {
			key: copies[0].as_str().into(),
			name: "Independent copy".into(),
		},
	);
	let expected = w.backend.session().authored_document();
	assert_eq!(
		expected
			.entities
			.iter()
			.find(|entity| entity.key == copies[1])
			.unwrap()
			.name,
		"Original"
	);
	w.save_all(w.backend.session().revision()).unwrap();
	let reopened = EditorWorkspace::open_project_with_notifier(
		ProjectLocation::from_manifest(&manifest).unwrap(),
		|| {},
	)
	.unwrap();
	assert_eq!(reopened.backend.session().authored_document(), expected);
	assert_eq!(
		reopened.backend.session().project(),
		w.backend.session().project()
	);
	for entity in &reopened.backend.session().authored_document().entities {
		let reference = AssetRef::new(&entity.model_path, entity.mesh_index)
			.unwrap()
			.with_asset_id(entity.asset_id.clone());
		let resolved = project_assets::resolve_asset(&fixture.0, &reference).unwrap();
		assert_eq!(resolved.asset_id, Some(model.id.clone()));
		assert_eq!(resolved.path, "models/moved.glb");
	}
	assert!(!fixture.0.join("models/triangle.glb").exists());
	assert!(!fixture.0.join("prefabs/assembly.json").exists());
	assert_eq!(
		project_assets::load_catalog(&fixture.0)
			.unwrap()
			.by_id(&prefab_id)
			.unwrap()
			.path,
		"prefabs/moved.json"
	);
}

#[test]
fn reimport_compatibility_includes_hidden_inactive_scenes_and_uninstantiated_prefabs() {
	use runtime::assets::project_assets;
	let (fixture, project, second) = two_scene_project();
	let catalog = project_assets::migrate_catalog(&fixture.0).unwrap();
	let model = catalog.by_path("models/triangle.glb").unwrap().clone();
	let mut main = runtime::scene_io::load_data(&fixture.0.join("scenes/main.json"))
		.unwrap()
		.data;
	main.entities.clear();
	main.bindings.clear();
	fs::write(fixture.0.join("scenes/main.json"), main.to_json().unwrap()).unwrap();
	let mut secondary = runtime::scene_io::load_data(&fixture.0.join("scenes/second.json"))
		.unwrap()
		.data;
	secondary.entities[0].renderable = false;
	secondary.entities[0].asset_id = Some(model.id.clone());
	fs::write(
		fixture.0.join("scenes/second.json"),
		secondary.to_json().unwrap(),
	)
	.unwrap();
	let mut w = fixture.open();
	assert_eq!(
		w.backend.session().active_scene_id(),
		&project.startup_scene_id
	);
	assert!(w.backend.session().authored_document().entities.is_empty());
	assert_eq!(
		w.used_asset_meshes(&model.id).unwrap(),
		std::collections::HashSet::from([0])
	);
	let prefab = runtime::editor::PrefabData {
		reference_slots: Default::default(),
		schema_version: 2,
		entities: secondary.entities.clone(),
		gameplay: None,
	};
	let prefab_record = project_assets::prepare_prefab(&fixture.0, "prefabs/hidden.json", &prefab)
		.unwrap()
		.publish()
		.unwrap();
	assert_eq!(prefab_record.dependencies, vec![model.id.clone()]);
	run(&mut w, Command::SwitchScene { scene_id: second });
	run(
		&mut w,
		Command::Select {
			keys: vec!["a".into()],
		},
	);
	run(&mut w, Command::DeleteSelection);
	run(
		&mut w,
		Command::SwitchScene {
			scene_id: project.startup_scene_id,
		},
	);
	assert!(w.backend.session().project().scenes.iter().all(|entry| w
		.backend
		.session()
		.scene_document(entry.id())
		.unwrap()
		.entities
		.is_empty()));
	let used = w.used_asset_meshes(&model.id).unwrap();
	assert_eq!(
		used,
		std::collections::HashSet::from([0]),
		"hidden prefab remains a dependant after every scene stops using the model"
	);
	let model_path = fixture.0.join("models/triangle.glb");
	let original = fs::read(&model_path).unwrap();
	let json_bytes = u32::from_le_bytes(original[12..16].try_into().unwrap()) as usize;
	let mut value: serde_json::Value =
		serde_json::from_slice(&original[20..20 + json_bytes]).unwrap();
	value["meshes"][0]["name"] =
		serde_json::json!(format!("{}-Incompatible", model.meshes[0].name));
	let mut json = value.to_string().into_bytes();
	while !json.len().is_multiple_of(4) {
		json.push(b' ');
	}
	let tail = &original[20 + json_bytes..];
	let mut bytes = original[..8].to_vec();
	bytes.extend_from_slice(&((20 + json.len() + tail.len()) as u32).to_le_bytes());
	bytes.extend_from_slice(&(json.len() as u32).to_le_bytes());
	bytes.extend_from_slice(b"JSON");
	bytes.extend_from_slice(&json);
	bytes.extend_from_slice(tail);
	let source = fixture.0.parent().unwrap().join("incompatible.glb");
	fs::write(&source, bytes).unwrap();
	let revision = w.backend.session().revision();
	let cursor = w.backend.change_cursor();
	let document = w.backend.session().authored_document();
	let catalog_before = fs::read(fixture.0.join(runtime::asset_catalog::CATALOG_FILE)).unwrap();
	assert!(project_assets::prepare_reimport(
		&fixture.0,
		&model.id,
		Some(&source),
		&model.fingerprint,
		&used
	)
	.is_err());
	assert_eq!(fs::read(model_path).unwrap(), original);
	assert_eq!(
		fs::read(fixture.0.join(runtime::asset_catalog::CATALOG_FILE)).unwrap(),
		catalog_before
	);
	assert_eq!(w.backend.session().revision(), revision);
	assert_eq!(w.backend.change_cursor(), cursor);
	assert_eq!(w.backend.session().authored_document(), document);
}

#[test]
fn gltf_import_expands_transform_nodes_offsets_roots_and_undo_restores_all() {
	let fixture = Fixture::new();
	fs::create_dir_all(fixture.0.join("models")).unwrap();
	fs::write(fixture.0.join("models/triangle.gltf"), r#"{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],"nodes":[{"name":"Root","translation":[1,0,0],"children":[1,2]},{"name":"First","translation":[0,2,0]},{"name":"Second","translation":[0,0,3]}]}"#).unwrap();
	let mut w = fixture.open();
	let before = w.backend.session().authored_document();
	let revision = w.backend.session().revision();
	w.import_model("models/triangle.gltf", [4.0, 0.0, 0.0], revision)
		.unwrap();
	let imported = w.backend.session().authored_document();
	assert_eq!(imported.entities.len(), 4);
	assert_eq!(imported.entities[1].transform.position, [5.0, 0.0, 0.0]);
	assert!(!imported.entities[1].renderable);
	assert_eq!(
		imported.entities[2].parent,
		Some(imported.entities[1].key.clone())
	);
	assert_eq!(imported.entities[2].transform.position, [0.0, 2.0, 0.0]);
	assert_eq!(
		w.backend.session().selection(),
		&[imported.entities[1].key.clone()]
	);
	assert!(w
		.import_model("models/triangle.gltf", [0.0; 3], revision)
		.is_err());
	assert_eq!(w.backend.session().authored_document(), imported);
	run(&mut w, Command::Undo);
	assert_eq!(w.backend.session().authored_document(), before);
	run(&mut w, Command::Redo);
	assert_eq!(w.backend.session().authored_document(), imported);
}

#[test]
fn direct_prefab_callback_checks_revision_before_expanding_and_rejects_play_edits() {
	let fixture = Fixture::new();
	let mut w = fixture.open();
	let prefab = runtime::editor::PrefabData {
		reference_slots: Default::default(),
		schema_version: 1,
		gameplay: None,
		entities: vec![EntityData::new(
			EntityKey::new("prefab").unwrap(),
			"Prefab",
			"models/triangle.gltf",
		)],
	};
	assert!(w
		.instantiate_prefab(prefab.clone(), [0.0; 3], w.backend.session().revision() + 1)
		.is_err());
	assert_eq!(w.backend.session().authored_document().entities.len(), 1);
	w.instantiate_prefab(
		prefab.clone(),
		[3.0, 0.0, 0.0],
		w.backend.session().revision(),
	)
	.unwrap();
	assert_eq!(
		w.backend.session().authored_document().entities[1]
			.transform
			.position,
		[3.0, 0.0, 0.0]
	);
	run(&mut w, Command::StartPlay);
	let before = w.backend.session().authored_document();
	assert!(w
		.instantiate_prefab(prefab, [0.0; 3], w.backend.session().revision())
		.is_err());
	assert_eq!(w.backend.session().authored_document(), before);
}

#[test]
fn async_model_import_adopts_one_current_revision_edit_and_is_undoable() {
	let fixture = Fixture::new();
	let (mut w, ready) = async_workspace(&fixture);
	let before = w.backend.session().authored_document();
	let ticket: crate::editor::AssetTicket = w
		.submit_import(
			"models/hierarchy.gltf".into(),
			[3.0, 0.0, 0.0],
			w.backend.session().revision(),
			1024 * 1024,
		)
		.unwrap();
	assert_eq!(ticket.workspace_generation, w.generation);
	let completion = await_completion(&mut w, &ready, ticket.job_id);
	assert!(completion.accepted, "{:?}", completion.error);
	assert_eq!(completion.workspace_generation, w.generation);
	assert_eq!(w.backend.session().authored_document().entities.len(), 3);
	let data = w.backend.session().authored_document();
	assert_eq!(data.entities[1].transform.position, [4.0, 0.0, 0.0]);
	assert_eq!(data.entities[2].parent, Some(data.entities[1].key.clone()));
	run(&mut w, Command::Undo);
	assert_eq!(w.backend.session().authored_document(), before);
	run(&mut w, Command::Redo);
	assert_eq!(w.backend.session().authored_document(), data);
	assert!(w
		.recent_completions()
		.iter()
		.any(|c| c.job_id == ticket.job_id && c.accepted));
}

#[test]
fn async_import_rejects_stale_results_after_another_edit_without_overwriting_the_document() {
	let fixture = Fixture::new();
	let (mut w, ready) = async_workspace(&fixture);
	let ticket = w
		.submit_import(
			"models/hierarchy.gltf".into(),
			[0.0; 3],
			w.backend.session().revision(),
			1024 * 1024,
		)
		.unwrap();
	run(
		&mut w,
		Command::Rename {
			key: "a".into(),
			name: "Newer edit".into(),
		},
	);
	let before = w.backend.session().authored_document();
	let revision = w.backend.session().revision();
	let cursor = w.backend.change_cursor();
	let completion = await_completion(&mut w, &ready, ticket.job_id);
	assert!(!completion.accepted);
	assert!(completion.error.unwrap().contains("Revision conflict"));
	assert_eq!(w.backend.session().authored_document(), before);
	assert_eq!(w.backend.session().revision(), revision);
	assert_eq!(w.backend.change_cursor(), cursor);
}

#[test]
fn switching_scenes_rejects_pending_import_adoption_without_changing_either_draft() {
	let (fixture, project, second) = two_scene_project();
	let (mut workspace, ready) = async_workspace(&fixture);
	let ticket = workspace
		.submit_import(
			"models/hierarchy.gltf".into(),
			[0.0; 3],
			workspace.backend.session().revision(),
			1024 * 1024,
		)
		.unwrap();
	run(
		&mut workspace,
		Command::SwitchScene {
			scene_id: second.clone(),
		},
	);
	let original = workspace
		.backend
		.session()
		.scene_document(&project.startup_scene_id)
		.unwrap();
	let active = workspace.backend.session().authored_document();
	let revision = workspace.backend.session().revision();
	let cursor = workspace.backend.change_cursor();
	let completion = await_completion(&mut workspace, &ready, ticket.job_id);
	assert!(!completion.accepted);
	assert_eq!(completion.error_code, Some("conflict"));
	assert_eq!(workspace.backend.session().active_scene_id(), &second);
	assert_eq!(workspace.backend.session().authored_document(), active);
	assert_eq!(
		workspace
			.backend
			.session()
			.scene_document(&project.startup_scene_id)
			.unwrap(),
		original
	);
	assert_eq!(workspace.backend.session().revision(), revision);
	assert_eq!(workspace.backend.change_cursor(), cursor);
}

#[test]
fn async_job_cancellation_checks_workspace_scope_and_prevents_adoption_even_after_decode() {
	let fixture = Fixture::new();
	let (mut w, ready) = async_workspace(&fixture);
	let before = w.backend.session().authored_document();
	let ticket = w
		.submit_import(
			"models/hierarchy.gltf".into(),
			[0.0; 3],
			w.backend.session().revision(),
			1024 * 1024,
		)
		.unwrap();
	assert!(w
		.cancel_import(ticket.job_id, ticket.workspace_generation + 1)
		.is_err());
	assert!(w
		.cancel_import(ticket.job_id, ticket.workspace_generation)
		.unwrap());
	let completion = await_completion(&mut w, &ready, ticket.job_id);
	assert!(!completion.accepted);
	assert_eq!(completion.error_code, Some("cancelled"));
	assert!(completion.error.is_some());
	assert_eq!(w.backend.session().authored_document(), before);
	assert!(!w
		.cancel_import(ticket.job_id, ticket.workspace_generation)
		.unwrap());
}

#[test]
fn stale_or_failed_async_imports_preserve_registry_document_history_and_events() {
	for stale in [false, true] {
		let fixture = Fixture::new();
		let (mut w, ready) = metadata_import_workspace(&fixture, if stale { 0.0 } else { 3.0e38 });
		assert!(w.backend.assets().is_empty());
		let ticket = w
			.submit_import(
				"models/selected.gltf".into(),
				if stale { [0.0; 3] } else { [3.0e38, 0.0, 0.0] },
				w.backend.session().revision(),
				1024 * 1024,
			)
			.unwrap();
		if stale {
			run(
				&mut w,
				Command::Rename {
					key: "a".into(),
					name: "Newer edit".into(),
				},
			);
		}
		let before = serde_json::to_value(w.backend.snapshot().unwrap()).unwrap();
		let cursor = w.backend.change_cursor();
		let completion = await_completion(&mut w, &ready, ticket.job_id);
		assert!(!completion.accepted);
		assert_eq!(
			completion.error_code,
			Some(if stale {
				"conflict"
			} else {
				"adoption_rejected"
			})
		);
		assert!(w.backend.assets().is_empty());
		assert_eq!(w.backend.assets().metadata_bytes(), 0);
		assert_eq!(
			serde_json::to_value(w.backend.snapshot().unwrap()).unwrap(),
			before
		);
		assert_eq!(w.backend.change_cursor(), cursor);
	}
}

#[test]
fn accepted_async_import_registers_only_selected_meshes_and_reload_invalidates_their_handles() {
	let fixture = Fixture::new();
	let (mut w, ready) = metadata_import_workspace(&fixture, 0.0);
	let ticket = w
		.submit_import(
			"models/selected.gltf".into(),
			[0.0; 3],
			w.backend.session().revision(),
			1024 * 1024,
		)
		.unwrap();
	let completion = await_completion(&mut w, &ready, ticket.job_id);
	assert!(completion.accepted, "{:?}", completion.error);
	assert_eq!(completion.error_code, None);
	assert_eq!(w.backend.assets().len(), 1);
	let selected = runtime::asset_registry::AssetRef::new("models/selected.gltf", Some(1)).unwrap();
	let handle = w.backend.assets().find(&selected).unwrap();
	assert!(w
		.backend
		.assets()
		.find(&runtime::asset_registry::AssetRef::new("models/selected.gltf", Some(0)).unwrap())
		.is_none());
	assert_eq!(
		w.backend.session().authored_document().entities[1].mesh_index,
		Some(1)
	);
	w.reload_with_policy(w.backend.session().revision(), ReloadPolicy::DiscardUnsaved)
		.unwrap();
	assert!(w.backend.assets().get(handle).is_none());
	assert!(w.backend.assets().is_empty());
	assert_eq!(w.backend.session().authored_document().entities.len(), 1);
}
