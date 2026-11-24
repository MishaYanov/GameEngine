use super::*;

#[cfg(windows)]
#[test]
fn unsaved_scene_case_alias_is_rejected_without_mutation_or_files() {
	let fixture = Fixture::new();
	let mut workspace = fixture.open();
	let document = (*workspace.backend.session().authored_document()).clone();
	run(
		&mut workspace,
		Command::AddScene {
			id: SceneId::new("first").unwrap(),
			asset_path: "scenes/New.json".into(),
			document: document.clone(),
		},
	);
	let before = serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap();
	let error = workspace
		.execute(EditorCommandRequest {
			expected_revision: workspace.backend.session().revision(),
			command: Command::AddScene {
				id: SceneId::new("second").unwrap(),
				asset_path: "SCENES/new.json".into(),
				document,
			},
		})
		.unwrap_err();
	assert!(error.contains("reserved"), "{error}");
	assert_eq!(
		serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap(),
		before
	);
	assert!(!fixture.0.join("scenes/New.json").exists());
	assert!(!fixture.0.join("scenes/new.json").exists());
}

#[test]
fn saving_prefab_respects_registered_unsaved_scene_destinations_without_mutation() {
	let fixture = Fixture::new();
	let mut workspace = fixture.open();
	let document = (*workspace.backend.session().authored_document()).clone();
	run(
		&mut workspace,
		Command::AddScene {
			id: SceneId::new("reserved").unwrap(),
			asset_path: "scenes/Reserved.json".into(),
			document,
		},
	);
	run(
		&mut workspace,
		Command::Select {
			keys: vec!["a".into()],
		},
	);
	let before = serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap();
	let cursor = workspace.backend.change_cursor();
	let mut destinations = vec!["scenes/Reserved.json"];
	#[cfg(windows)]
	destinations.push("SCENES/reserved.json");
	for path in destinations {
		let error = workspace
			.save_prefab(path, workspace.backend.session().revision())
			.unwrap_err();
		assert!(error.contains("reserved"), "{error}");
		assert_eq!(
			serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap(),
			before
		);
		assert_eq!(workspace.backend.change_cursor(), cursor);
		assert!(!fixture.0.join(path).exists());
	}
	assert!(!fixture
		.0
		.join(runtime::asset_catalog::CATALOG_FILE)
		.exists());
	assert_eq!(fs::read_dir(fixture.0.join("prefabs")).unwrap().count(), 0);
}

#[cfg(windows)]
#[test]
fn save_rechecks_case_alias_ownership_and_preserves_existing_file() {
	let fixture = Fixture::new();
	let mut workspace = fixture.open();
	let original = fs::read(fixture.0.join("scenes/main.json")).unwrap();
	let document = (*workspace.backend.session().authored_document()).clone();
	// Exercise the save boundary even for catalog entries created outside the
	// host adapter, so creation-time checks alone cannot hide an unsafe save.
	workspace
		.backend
		.execute(EditorCommandRequest {
			expected_revision: workspace.backend.session().revision(),
			command: Command::AddScene {
				id: SceneId::new("alias").unwrap(),
				asset_path: "scenes/MAIN.json".into(),
				document,
			},
		})
		.unwrap();
	let before = serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap();
	let error = workspace
		.save(workspace.backend.session().revision())
		.unwrap_err();
	assert!(error.contains("reserved"), "{error}");
	assert_eq!(
		fs::read(fixture.0.join("scenes/main.json")).unwrap(),
		original
	);
	assert_eq!(
		serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap(),
		before
	);
}

#[test]
fn project_open_uses_canonical_selected_manifest_startup_scene_and_notifier() {
	let fixture = Fixture::new();
	let mut scene = runtime::scene_io::load_data(&fixture.0.join("scenes/main.json"))
		.unwrap()
		.data;
	scene.entities[0].name = "Configured startup".into();
	fs::write(
		fixture.0.join("scenes/configured.json"),
		scene.to_json().unwrap(),
	)
	.unwrap();
	let manifest = fixture.write_project("scenes/configured.json");
	let location =
		ProjectLocation::from_manifest(&fixture.0.join("scenes/../../project.json")).unwrap();
	assert_eq!(
		location.manifest().parent().unwrap(),
		manifest.parent().unwrap().canonicalize().unwrap()
	);
	let (notify, ready) = std::sync::mpsc::channel();
	let mut workspace = EditorWorkspace::open_project_with_notifier(location, move || {
		let _ = notify.send(());
	})
	.unwrap();
	assert_eq!(workspace.project_path, manifest.canonicalize().unwrap());
	assert_eq!(workspace.asset_root, fixture.0.canonicalize().unwrap());
	assert_eq!(
		workspace.scene_path,
		fixture
			.0
			.join("scenes/configured.json")
			.canonicalize()
			.unwrap()
	);
	assert_eq!(
		workspace.backend.session().project().name,
		"Selected project"
	);
	assert_eq!(
		workspace.backend.session().authored_document().entities[0].name,
		"Configured startup"
	);
	// Opening queues a bounds job for the startup model; its completion must wake
	// the supplied notifier so the host can drain and adopt it.
	ready
		.recv_timeout(std::time::Duration::from_secs(5))
		.unwrap();
	assert!(!workspace.drain_asset_jobs().is_empty());
}

#[test]
fn unavailable_saved_module_can_be_opened_edited_and_repaired_without_losing_bindings() {
	use bridge::editor::SceneBindingTarget;
	use runtime::project::ProjectData;
	let fixture = Fixture::new();
	let manifest = fixture.write_project("scenes/main.json");
	let mut project = ProjectData::load(&manifest).unwrap();
	project.gameplay_module = Some("unavailable".into());
	fs::write(&manifest, project.to_json().unwrap()).unwrap();
	let mut workspace = EditorWorkspace::open_project_with_notifier(
		ProjectLocation::from_manifest(&manifest).unwrap(),
		|| {},
	)
	.unwrap();
	let scene_id = workspace.backend.session().active_scene_id().clone();
	run(
		&mut workspace,
		Command::SetSceneBinding {
			scene_id,
			name: "player".into(),
			target: SceneBindingTarget::Assign { key: "a".into() },
		},
	);
	let mut project = workspace.backend.session().project().clone();
	project.name = "Editable unavailable module".into();
	run(&mut workspace, Command::SetProject { project });
	let revision = workspace.backend.session().revision();
	assert!(workspace
		.execute(EditorCommandRequest {
			expected_revision: revision,
			command: Command::StartPlay,
		})
		.unwrap_err()
		.contains("unavailable"));
	assert_eq!(workspace.backend.session().revision(), revision);
	let before = workspace.backend.snapshot().unwrap();
	let mut project = before.project.clone();
	project.gameplay_module = None;
	let update = workspace
		.execute_incremental(EditorCommandRequest {
			expected_revision: before.revision,
			command: Command::SetProject { project },
		})
		.unwrap();
	let EditorCommandUpdateDto::Snapshot { snapshot, receipt } = update else {
		panic!("Module selection must return the changed project");
	};
	assert!(receipt.project_changed);
	assert!(snapshot.project.gameplay_module.is_none());
	assert_eq!(snapshot.document, before.document);
	assert_eq!(snapshot.project.behaviors, before.project.behaviors);
	run(&mut workspace, Command::Undo);
	assert_eq!(
		workspace
			.backend
			.session()
			.project()
			.gameplay_module
			.as_deref(),
		Some("unavailable")
	);
	run(&mut workspace, Command::Redo);
	workspace
		.save_all(workspace.backend.session().revision())
		.unwrap();
	let mut reopened = fixture.open();
	assert!(reopened
		.backend
		.session()
		.project()
		.gameplay_module
		.is_none());
	assert_eq!(
		reopened.backend.session().authored_document().bindings,
		before.document.bindings
	);
	run(&mut reopened, Command::StartPlay);
	run(&mut reopened, Command::StopPlay);
}

#[test]
fn project_open_requires_a_manifest_and_rejects_invalid_manifest_data() {
	let fixture = Fixture::new();
	let manifest = fixture.0.parent().unwrap().join("project.json");
	assert!(matches!(
		ProjectLocation::from_manifest(&manifest),
		Err(ProjectOpenError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound
	));
	fs::create_dir(&manifest).unwrap();
	assert!(matches!(
		ProjectLocation::from_manifest(&manifest),
		Err(ProjectOpenError::InvalidManifest)
	));
	fs::remove_dir(&manifest).unwrap();
	for invalid in ["{bad json", r#"{"schema_version":999}"#] {
		fs::write(&manifest, invalid).unwrap();
		let location = ProjectLocation::from_manifest(&manifest).unwrap();
		assert!(EditorWorkspace::open_project_with_notifier(location, || {}).is_err());
	}
}

#[test]
fn project_location_rejects_wrong_manifest_names_before_accessing_the_filesystem() {
	let fixture = Fixture::new();
	for name in ["other.json", "project.JSON", "project.json.bak"] {
		let candidate = fixture.0.parent().unwrap().join(name);
		assert!(matches!(
			ProjectLocation::from_manifest(&candidate),
			Err(ProjectOpenError::InvalidManifestName)
		));
		assert!(!candidate.exists());
	}
}

#[test]
fn project_location_distinguishes_missing_assets_from_a_file_without_modifying_the_manifest() {
	let fixture = Fixture::new();
	let manifest = fixture.write_project("scenes/main.json");
	let before = fs::read(&manifest).unwrap();
	let original_assets = fixture.0.parent().unwrap().join("original-assets");
	fs::rename(&fixture.0, original_assets).unwrap();
	assert!(matches!(
		ProjectLocation::from_manifest(&manifest),
		Err(ProjectOpenError::MissingAssetsDirectory)
	));
	fs::write(&fixture.0, b"not a directory").unwrap();
	assert!(matches!(
		ProjectLocation::from_manifest(&manifest),
		Err(ProjectOpenError::AssetsNotDirectory)
	));
	assert_eq!(fs::read(manifest).unwrap(), before);
}

#[test]
fn project_open_rejects_missing_and_malformed_startup_scene() {
	let candidate_fixture = Fixture::new();
	let manifest = candidate_fixture.write_project("scenes/missing.json");
	for candidate_scene in [None, Some("{bad json")] {
		if let Some(source) = candidate_scene {
			fs::write(candidate_fixture.0.join("scenes/missing.json"), source).unwrap();
		}
		let location = ProjectLocation::from_manifest(&manifest).unwrap();
		assert!(EditorWorkspace::open_project_with_notifier(location, || {}).is_err());
	}
}

#[test]
fn save_all_persists_dirty_scene_drafts_and_project_without_switching() {
	let (fixture, _, second) = two_scene_project();
	let mut workspace = fixture.open();
	run(
		&mut workspace,
		Command::Rename {
			key: "a".into(),
			name: "First saved together".into(),
		},
	);
	run(
		&mut workspace,
		Command::SwitchScene {
			scene_id: second.clone(),
		},
	);
	run(
		&mut workspace,
		Command::Rename {
			key: "a".into(),
			name: "Second saved together".into(),
		},
	);
	let mut project = workspace.backend.session().project().clone();
	project.name = "Saved all settings".into();
	run(&mut workspace, Command::SetProject { project });
	let active_path = workspace.scene_path.clone();
	let scene_revision = workspace.backend.session().scene_revision();
	let saved = workspace
		.save_all(workspace.backend.session().revision())
		.unwrap();
	assert_eq!(saved.active_scene_id, second);
	assert_eq!(workspace.scene_path, active_path);
	assert_eq!(saved.scene_revision, scene_revision);
	assert!(!saved.dirty);
	assert!(!saved.project_dirty);
	assert!(saved.scenes.iter().all(|scene| !scene.dirty));
	for (path, name) in [
		("scenes/main.json", "First saved together"),
		("scenes/second.json", "Second saved together"),
	] {
		assert_eq!(
			runtime::scene_io::load_data(&fixture.0.join(path))
				.unwrap()
				.data
				.entities[0]
				.name,
			name
		);
	}
	assert_eq!(
		runtime::project::ProjectData::load(&workspace.project_path)
			.unwrap()
			.name,
		"Saved all settings"
	);
	let reopened = fixture.open();
	assert!(!reopened.backend.session().is_dirty());
	assert_eq!(
		reopened.backend.session().project().name,
		"Saved all settings"
	);
}

#[test]
fn save_all_keeps_successful_scene_baselines_when_manifest_write_fails_then_retries() {
	let fixture = Fixture::new();
	fixture.write_project("scenes/main.json");
	let mut workspace = fixture.open();
	run(
		&mut workspace,
		Command::Rename {
			key: "a".into(),
			name: "Scene already written".into(),
		},
	);
	let mut project = workspace.backend.session().project().clone();
	project.name = "Project pending retry".into();
	run(&mut workspace, Command::SetProject { project });
	// Exhaust the manifest writer's temporary slots to fail after scene writes.
	let blocked: Vec<_> = (0..128)
		.map(|id| {
			workspace
				.project_path
				.with_file_name(format!("project.json.tmp-{}-{id}", std::process::id()))
		})
		.collect();
	for path in &blocked {
		fs::write(path, b"held").unwrap();
	}
	let report = workspace
		.save_all_report(workspace.backend.session().revision())
		.unwrap();
	assert_eq!(
		report.files[0].outcome,
		editor::documents::SaveOutcome::Written
	);
	assert_eq!(
		report.files[1].outcome,
		editor::documents::SaveOutcome::Failed
	);
	assert!(report.files[1]
		.message
		.as_deref()
		.unwrap()
		.contains("Cannot allocate temporary file"));
	let partial = report.snapshot;
	assert!(partial.project_dirty);
	assert!(partial.dirty);
	assert!(partial.scenes.iter().all(|scene| !scene.dirty));
	assert_eq!(
		runtime::scene_io::load_data(&workspace.scene_path)
			.unwrap()
			.data
			.entities[0]
			.name,
		"Scene already written"
	);
	assert_eq!(
		runtime::project::ProjectData::load(&workspace.project_path)
			.unwrap()
			.name,
		"Selected project"
	);
	for path in blocked {
		fs::remove_file(path).unwrap();
	}
	assert!(
		!workspace
			.save_all(workspace.backend.session().revision())
			.unwrap()
			.dirty
	);
	assert_eq!(
		runtime::project::ProjectData::load(&workspace.project_path)
			.unwrap()
			.name,
		"Project pending retry"
	);
}

#[test]
fn workspace_switch_preserves_drafts_and_saves_only_active_scene_then_reopens_startup() {
	let (fixture, project, second) = two_scene_project();
	let mut workspace = fixture.open();
	let main_disk = fs::read(&workspace.scene_path).unwrap();
	run(
		&mut workspace,
		Command::Rename {
			key: "a".into(),
			name: "First saved".into(),
		},
	);
	assert!(workspace.scene_switch.is_none());
	let (_, preview_closed) = workspace.with_viewport_update(|workspace| {
		run(
			workspace,
			Command::SwitchScene {
				scene_id: second.clone(),
			},
		);
	});
	assert!(!preview_closed);
	let (revision, requested, installed) = workspace.scene_switch.unwrap();
	assert_eq!(revision, workspace.backend.session().scene_revision());
	assert!(requested <= installed);
	assert_eq!(
		workspace.scene_path,
		fixture.0.join("scenes/second.json").canonicalize().unwrap()
	);
	run(
		&mut workspace,
		Command::Rename {
			key: "a".into(),
			name: "Second saved".into(),
		},
	);
	workspace
		.save(workspace.backend.session().revision())
		.unwrap();
	assert_eq!(
		fs::read(fixture.0.join("scenes/main.json")).unwrap(),
		main_disk
	);
	assert!(workspace.backend.snapshot().unwrap().dirty);
	run(
		&mut workspace,
		Command::SwitchScene {
			scene_id: project.startup_scene_id.clone(),
		},
	);
	assert_eq!(
		workspace.backend.session().authored_document().entities[0].name,
		"First saved"
	);
	workspace
		.save(workspace.backend.session().revision())
		.unwrap();
	assert!(!workspace.backend.snapshot().unwrap().dirty);
	run(
		&mut workspace,
		Command::SetStartupScene {
			scene_id: second.clone(),
		},
	);
	assert_eq!(
		workspace.backend.snapshot().unwrap().active_scene_id,
		project.startup_scene_id
	);
	workspace
		.save_project(workspace.backend.session().revision())
		.unwrap();
	let reopened = EditorWorkspace::open_project_with_notifier(
		ProjectLocation::from_manifest(&workspace.project_path).unwrap(),
		|| {},
	)
	.unwrap();
	assert_eq!(reopened.backend.snapshot().unwrap().active_scene_id, second);
	assert_eq!(
		reopened.backend.session().authored_document().entities[0].name,
		"Second saved"
	);
}

#[test]
fn opening_a_project_rejects_invalid_inactive_scene_before_constructing_workspace() {
	let (fixture, _, _) = two_scene_project();
	fs::write(fixture.0.join("scenes/second.json"), "{malformed").unwrap();
	let manifest = fixture.0.parent().unwrap().join("project.json");
	let location = ProjectLocation::from_manifest(&manifest).unwrap();
	assert!(EditorWorkspace::open_project_with_notifier(location, || {}).is_err());
}

#[test]
fn save_scene_as_keeps_identity_and_old_file_then_persists_new_path_on_project_save() {
	let fixture = Fixture::new();
	let manifest = fixture.write_project("scenes/main.json");
	let mut workspace = fixture.open();
	let id = workspace.backend.snapshot().unwrap().active_scene_id;
	let original = fs::read(&workspace.scene_path).unwrap();
	run(
		&mut workspace,
		Command::Rename {
			key: "a".into(),
			name: "Saved at new location".into(),
		},
	);
	let saved = workspace
		.save_scene_as(
			"scenes/renamed.json",
			workspace.backend.session().revision(),
		)
		.unwrap();
	assert_eq!(saved.active_scene_id, id);
	assert_eq!(
		saved.project.scene_path(&id).unwrap(),
		"scenes/renamed.json"
	);
	assert!(saved.project_dirty);
	assert!(
		!saved
			.scenes
			.iter()
			.find(|entry| entry.id == id)
			.unwrap()
			.dirty
	);
	assert_eq!(
		fs::read(fixture.0.join("scenes/main.json")).unwrap(),
		original
	);
	assert_eq!(
		runtime::scene_io::load_data(&fixture.0.join("scenes/renamed.json"))
			.unwrap()
			.data
			.entities[0]
			.name,
		"Saved at new location"
	);
	run(&mut workspace, Command::Undo);
	let undone = workspace.backend.snapshot().unwrap();
	assert_eq!(undone.project.scene_path(&id).unwrap(), "scenes/main.json");
	assert!(
		undone
			.scenes
			.iter()
			.find(|entry| entry.id == id)
			.unwrap()
			.dirty,
		"The saved baseline belongs to the new file; returning to the old path must be dirty"
	);
	assert_eq!(
		workspace.scene_path,
		fixture.0.join("scenes/main.json").canonicalize().unwrap()
	);
	run(&mut workspace, Command::Redo);
	assert!(
		!workspace
			.backend
			.snapshot()
			.unwrap()
			.scenes
			.iter()
			.find(|entry| entry.id == id)
			.unwrap()
			.dirty
	);
	workspace
		.save_project(workspace.backend.session().revision())
		.unwrap();
	let reopened = EditorWorkspace::open_project_with_notifier(
		ProjectLocation::from_manifest(&manifest).unwrap(),
		|| {},
	)
	.unwrap();
	assert_eq!(reopened.backend.snapshot().unwrap().active_scene_id, id);
	assert_eq!(
		reopened.scene_path,
		fixture
			.0
			.join("scenes/renamed.json")
			.canonicalize()
			.unwrap()
	);
}

#[test]
fn failed_save_scene_as_preserves_catalog_history_paths_revision_and_disk() {
	let fixture = Fixture::new();
	let mut workspace = fixture.open();
	run(
		&mut workspace,
		Command::Rename {
			key: "a".into(),
			name: "Keep draft".into(),
		},
	);
	let before = serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap();
	let cursor = workspace.backend.change_cursor();
	let original = fs::read(&workspace.scene_path).unwrap();
	let path = workspace.scene_path.clone();
	assert!(workspace
		.save_scene_as(
			"scenes/stale.json",
			workspace.backend.session().revision() - 1
		)
		.is_err());
	assert!(!fixture.0.join("scenes/stale.json").exists());
	fs::write(fixture.0.join("scenes/existing.json"), "do not overwrite").unwrap();
	for invalid in [
		"../outside.json",
		"missing/scene.json",
		"scenes/existing.json",
	] {
		assert!(workspace
			.save_scene_as(invalid, workspace.backend.session().revision())
			.is_err());
		assert_eq!(
			serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap(),
			before
		);
		assert_eq!(workspace.backend.change_cursor(), cursor);
		assert_eq!(workspace.scene_path, path);
		assert_eq!(fs::read(&workspace.scene_path).unwrap(), original);
		assert_eq!(
			fs::read_to_string(fixture.0.join("scenes/existing.json")).unwrap(),
			"do not overwrite"
		);
	}
}

#[test]
fn workspace_revision_and_command_validation_precede_filesystem_preflight() {
	let fixture = Fixture::new();
	let mut workspace = fixture.open();
	run(
		&mut workspace,
		Command::Rename {
			key: "a".into(),
			name: "Keep current edit".into(),
		},
	);
	let before = serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap();
	let revision = workspace.backend.session().revision();
	let error = workspace
		.execute_receipt(EditorCommandRequest {
			expected_revision: revision - 1,
			command: Command::ChangeScenePath {
				scene_id: SceneId::new("missing").unwrap(),
				asset_path: "scenes/missing.json".into(),
			},
		})
		.unwrap_err();
	assert_eq!(error.code, runtime::BackendErrorCode::Conflict);
	let error = workspace
		.execute_receipt(EditorCommandRequest {
			expected_revision: revision,
			command: Command::ChangeScenePath {
				scene_id: SceneId::new("missing").unwrap(),
				asset_path: "../outside.json".into(),
			},
		})
		.unwrap_err();
	assert_eq!(error.code, runtime::BackendErrorCode::InvalidCommand);
	assert_eq!(
		serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap(),
		before
	);
}

#[test]
fn registering_existing_scene_marks_it_saved_and_catalog_removal_keeps_its_file() {
	let fixture = Fixture::new();
	let mut workspace = fixture.open();
	let first = workspace.backend.snapshot().unwrap().active_scene_id;
	let registered_path = fixture.0.join("scenes/registered.json");
	fs::copy(&workspace.scene_path, &registered_path).unwrap();
	let snapshot = workspace
		.register_scene(
			"scenes/registered.json",
			workspace.backend.session().revision(),
		)
		.unwrap();
	let added = snapshot
		.scenes
		.iter()
		.find(|entry| entry.asset_path == "scenes/registered.json")
		.unwrap();
	assert!(added.loaded);
	assert!(!added.dirty);
	assert!(snapshot.project_dirty);
	let added = added.id.clone();
	run(&mut workspace, Command::SwitchScene { scene_id: first });
	run(
		&mut workspace,
		Command::RemoveScene {
			scene_id: added.clone(),
		},
	);
	assert!(registered_path.exists());
	assert!(workspace
		.backend
		.session()
		.project()
		.scenes
		.get(&added)
		.is_err());
	run(&mut workspace, Command::Undo);
	run(&mut workspace, Command::SwitchScene { scene_id: added });
	assert_eq!(
		workspace.backend.session().authored_document().entities[0].name,
		"Original"
	);
	let before = serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap();
	for invalid in ["scenes/registered.json", "scenes/missing.json"] {
		assert!(workspace
			.register_scene(invalid, workspace.backend.session().revision())
			.is_err());
		assert_eq!(
			serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap(),
			before
		);
	}
}

#[test]
fn reload_policy_protects_all_drafts_and_discard_validates_every_scene_before_mutation() {
	let (fixture, project, second) = two_scene_project();
	let mut workspace = fixture.open();
	let second_path = fixture.0.join("scenes/second.json");
	let second_disk = fs::read(&second_path).unwrap();
	run(
		&mut workspace,
		Command::Rename {
			key: "a".into(),
			name: "First draft".into(),
		},
	);
	run(
		&mut workspace,
		Command::SwitchScene {
			scene_id: second.clone(),
		},
	);
	run(
		&mut workspace,
		Command::Rename {
			key: "a".into(),
			name: "Second draft".into(),
		},
	);
	let before = serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap();
	assert!(workspace
		.reload_with_policy(
			workspace.backend.session().revision(),
			ReloadPolicy::RequireClean
		)
		.is_err());
	assert_eq!(
		serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap(),
		before
	);
	fs::write(&second_path, "{bad scene").unwrap();
	assert!(workspace
		.reload_with_policy(
			workspace.backend.session().revision(),
			ReloadPolicy::DiscardUnsaved
		)
		.is_err());
	assert_eq!(
		serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap(),
		before
	);
	fs::write(&second_path, second_disk).unwrap();
	let reloaded = workspace
		.reload_with_policy(
			workspace.backend.session().revision(),
			ReloadPolicy::DiscardUnsaved,
		)
		.unwrap();
	assert_eq!(reloaded.active_scene_id, project.startup_scene_id);
	assert_eq!(reloaded.document.entities[0].name, "Original");
	assert!(!reloaded.dirty);
	assert!(!reloaded.can_undo);
	run(&mut workspace, Command::SwitchScene { scene_id: second });
	assert_eq!(
		workspace.backend.session().authored_document().entities[0].name,
		"Second original"
	);
}

#[cfg(any(unix, windows))]
#[test]
fn linked_assets_do_not_reconstruct_the_manifest_from_the_resolved_assets_parent() {
	let fixture = Fixture::new();
	let manifest = fixture.write_project("scenes/main.json");
	let storage = fixture.0.parent().unwrap().join("storage");
	fs::create_dir(&storage).unwrap();
	let resolved_assets = storage.join("assets");
	fs::rename(&fixture.0, &resolved_assets).unwrap();
	// A manifest beside the physical assets must not override the selected one.
	fs::write(storage.join("project.json"), "{wrong manifest").unwrap();
	#[cfg(windows)]
	let linked = std::os::windows::fs::symlink_dir(&resolved_assets, &fixture.0);
	#[cfg(unix)]
	let linked = std::os::unix::fs::symlink(&resolved_assets, &fixture.0);
	if let Err(error) = linked {
		if matches!(
			error.kind(),
			std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::Unsupported
		) {
			eprintln!("SKIPPED linked-assets regression: directory symlinks unavailable: {error}");
			return;
		}
		panic!("Cannot create test directory link: {error}");
	}
	let location = ProjectLocation::from_manifest(&manifest).unwrap();
	let workspace = EditorWorkspace::open_project_with_notifier(location, || {}).unwrap();
	assert_eq!(workspace.project_path, manifest.canonicalize().unwrap());
	assert_eq!(
		workspace.asset_root,
		resolved_assets.canonicalize().unwrap()
	);
	assert_eq!(
		workspace.backend.session().project().name,
		"Selected project"
	);
}

#[test]
fn workspace_save_reload_revision_and_generation_preserve_consistent_baselines() {
	let fixture = Fixture::new();
	let mut w = fixture.open();
	let second = fixture.open();
	assert_ne!(w.generation, second.generation);
	let original = fs::read(&w.scene_path).unwrap();
	run(
		&mut w,
		Command::Rename {
			key: "a".into(),
			name: "Saved".into(),
		},
	);
	let revision = w.backend.session().revision();
	assert!(w.save(revision - 1).is_err());
	assert_eq!(fs::read(&w.scene_path).unwrap(), original);
	let saved = w.save(revision).unwrap();
	assert!(!saved.dirty);
	assert!(saved.revision > revision);
	run(
		&mut w,
		Command::Rename {
			key: "a".into(),
			name: "Unsaved".into(),
		},
	);
	let reloaded = w
		.reload_with_policy(w.backend.session().revision(), ReloadPolicy::DiscardUnsaved)
		.unwrap();
	assert_eq!(reloaded.document.entities[0].name, "Saved");
	assert!(!reloaded.can_undo);
	assert!(!reloaded.dirty);
	let source = w.backend.session().authored_document();
	fs::write(&w.scene_path, "{bad json").unwrap();
	let revision = w.backend.session().revision();
	assert!(w
		.reload_with_policy(revision, ReloadPolicy::DiscardUnsaved)
		.is_err());
	assert_eq!(w.backend.session().authored_document(), source);
	assert_eq!(w.backend.session().revision(), revision);
}

#[test]
fn grouped_edits_block_save_reload_and_failed_save_does_not_mark_document_clean() {
	let fixture = Fixture::new();
	let mut w = fixture.open();
	let original = fs::read(&w.scene_path).unwrap();
	run(
		&mut w,
		Command::BeginTransaction {
			label: "Editing".into(),
		},
	);
	run(
		&mut w,
		Command::Rename {
			key: "a".into(),
			name: "Preview".into(),
		},
	);
	let revision = w.backend.session().revision();
	assert!(w.save(revision).is_err());
	assert!(w
		.reload_with_policy(revision, ReloadPolicy::DiscardUnsaved)
		.is_err());
	assert_eq!(fs::read(&w.scene_path).unwrap(), original);
	assert!(w.backend.session().has_transaction());
	run(&mut w, Command::CommitTransaction);
	// Save resolves the authoritative catalog path, so fail the write at that
	// actual destination rather than altering the workspace's cached path.
	fs::rename(&w.scene_path, fixture.0.join("scenes/main.saved.json")).unwrap();
	fs::create_dir(&w.scene_path).unwrap();
	let revision = w.backend.session().revision();
	assert!(w.save(revision).is_err());
	assert_eq!(w.backend.session().revision(), revision);
	assert!(w.backend.session().snapshot().unwrap().dirty);
}

#[test]
fn workspace_paths_are_asset_relative_and_rejected_prefabs_do_not_touch_document_or_disk() {
	let fixture = Fixture::new();
	for path in [
		"../outside.json",
		"/absolute.json",
		"C:/outside.json",
		"scenes\\main.json",
		"scenes//main.json",
		"scenes",
	] {
		assert!(
			EditorWorkspace::open(fixture.0.to_str().unwrap(), path).is_err(),
			"accepted {path:?}"
		);
	}
	let mut w = fixture.open();
	run(
		&mut w,
		Command::Select {
			keys: vec!["a".into()],
		},
	);
	let before = w.backend.session().authored_document();
	let revision = w.backend.session().revision();
	for path in [
		"../outside.json",
		"prefabs/name.txt",
		"missing/name.json",
		"C:/name.json",
	] {
		assert!(w.save_prefab(path, revision).is_err(), "accepted {path:?}");
	}
	assert_eq!(fs::read_dir(fixture.0.join("prefabs")).unwrap().count(), 0);
	fs::write(fixture.0.join("prefabs/bad.json"), "{bad json").unwrap();
	assert!(w
		.load_prefab("prefabs/bad.json", [0.0; 3], revision)
		.is_err());
	assert_eq!(w.backend.session().authored_document(), before);
	assert_eq!(w.backend.session().revision(), revision);
}

#[test]
fn authored_project_and_scene_save_independently_and_prepare_build_requires_both_current_disk_inputs(
) {
	let fixture = Fixture::new();
	let mut w = fixture.open();
	assert!(!w.project_path.exists());
	assert!(w.prepare_build(w.backend.session().revision()).is_err());
	let mut project = w.backend.session().project().clone();
	project.name = "Derived QA game".into();
	project.add_behavior(runtime::project::BehaviorSpec::ControlledMove {
		entity: EntityKey::new("a").unwrap(),
		speed: 3.0,
		space: Default::default(),
	});
	run(
		&mut w,
		Command::SetProject {
			project: project.clone(),
		},
	);
	run(
		&mut w,
		Command::Rename {
			key: "a".into(),
			name: "Authored actor".into(),
		},
	);
	w.save(w.backend.session().revision()).unwrap();
	assert!(w.backend.session().is_dirty());
	assert!(w.prepare_build(w.backend.session().revision()).is_err());
	w.save_project(w.backend.session().revision()).unwrap();
	assert!(!w.backend.session().is_dirty());
	assert_eq!(
		runtime::project::ProjectData::load(&w.project_path).unwrap(),
		project
	);
	let prepared = w.prepare_build(w.backend.session().revision()).unwrap();
	assert_eq!(prepared.project(), &project);
	assert!(prepared.asset_count() >= 2);
	let mut external = project;
	external.name = "External edits".into();
	fs::write(&w.project_path, external.to_json().unwrap()).unwrap();
	assert!(w.prepare_build(w.backend.session().revision()).is_err());
	w.reload_with_policy(w.backend.session().revision(), ReloadPolicy::DiscardUnsaved)
		.unwrap();
	assert_eq!(w.backend.session().project().name, "External edits");
	assert!(!w.backend.session().is_dirty());
}

#[test]
fn project_reload_failure_preserves_open_workspace_project_scene_and_history() {
	let fixture = Fixture::new();
	let mut w = fixture.open();
	w.save_project(w.backend.session().revision()).unwrap();
	run(
		&mut w,
		Command::Rename {
			key: "a".into(),
			name: "Keep this edit".into(),
		},
	);
	let before = serde_json::to_value(w.backend.snapshot().unwrap()).unwrap();
	let path = w.scene_path.clone();
	fs::write(&w.project_path, "{bad json").unwrap();
	assert!(w
		.reload_with_policy(w.backend.session().revision(), ReloadPolicy::DiscardUnsaved)
		.is_err());
	assert_eq!(
		serde_json::to_value(w.backend.snapshot().unwrap()).unwrap(),
		before
	);
	assert_eq!(w.scene_path, path);
}

#[test]
fn manifest_backed_reload_rejects_missing_manifest_without_mutating_workspace() {
	for explicit_project_open in [true, false] {
		let fixture = Fixture::new();
		let manifest = fixture.write_project("scenes/main.json");
		let mut workspace = if explicit_project_open {
			EditorWorkspace::open_project_with_notifier(
				ProjectLocation::from_manifest(&manifest).unwrap(),
				|| {},
			)
			.unwrap()
		} else {
			fixture.open()
		};
		assert_missing_manifest_reload_preserves_edits(&mut workspace);
	}
}

#[test]
fn standalone_reload_works_without_manifest_and_failed_save_does_not_promote_it() {
	let fixture = Fixture::new();
	let mut workspace = fixture.open();
	let manifest = workspace.project_path.clone();
	assert!(!manifest.exists());
	run(
		&mut workspace,
		Command::Rename {
			key: "a".into(),
			name: "Discard this edit".into(),
		},
	);
	workspace.project_path = fixture.0.join("missing/project.json");
	assert!(workspace
		.save_project(workspace.backend.session().revision())
		.is_err());
	workspace.project_path = manifest;
	let reloaded = workspace
		.reload_with_policy(
			workspace.backend.session().revision(),
			ReloadPolicy::DiscardUnsaved,
		)
		.unwrap();
	assert_eq!(reloaded.document.entities[0].name, "Original");
	assert!(!reloaded.dirty);
	assert!(!reloaded.can_undo);
	assert!(!workspace.project_path.exists());
}

#[test]
fn saving_standalone_project_establishes_manifest_requirement() {
	let fixture = Fixture::new();
	let mut workspace = fixture.open();
	workspace
		.save_project(workspace.backend.session().revision())
		.unwrap();
	assert_missing_manifest_reload_preserves_edits(&mut workspace);
}

#[test]
fn standalone_adopts_discovered_manifest_only_after_successful_reload() {
	let fixture = Fixture::new();
	let mut workspace = fixture.open();
	fixture.write_project("scenes/missing.json");
	assert!(workspace
		.reload_with_policy(
			workspace.backend.session().revision(),
			ReloadPolicy::DiscardUnsaved
		)
		.is_err());
	fs::remove_file(&workspace.project_path).unwrap();
	workspace
		.reload_with_policy(
			workspace.backend.session().revision(),
			ReloadPolicy::DiscardUnsaved,
		)
		.unwrap();
	fixture.write_project("scenes/main.json");
	workspace
		.reload_with_policy(
			workspace.backend.session().revision(),
			ReloadPolicy::DiscardUnsaved,
		)
		.unwrap();
	assert_eq!(
		workspace.backend.session().project().name,
		"Selected project"
	);
	assert_missing_manifest_reload_preserves_edits(&mut workspace);
}

#[test]
fn successful_reload_renews_generation_and_rejects_old_job_tickets_at_zero_scene_revision() {
	let fixture = Fixture::new();
	let (mut workspace, _) = async_workspace(&fixture);
	assert_eq!(workspace.backend.snapshot().unwrap().scene_revision, 0);
	let old_generation = workspace.generation;
	let ticket = workspace
		.submit_import(
			"models/hierarchy.gltf".into(),
			[0.0; 3],
			workspace.backend.session().revision(),
			1024 * 1024,
		)
		.unwrap();
	let reloaded = workspace
		.reload_with_policy(
			workspace.backend.session().revision(),
			ReloadPolicy::DiscardUnsaved,
		)
		.unwrap();
	assert_eq!(reloaded.scene_revision, 0);
	assert_ne!(workspace.generation, old_generation);
	assert!(workspace
		.cancel_import(ticket.job_id, ticket.workspace_generation)
		.is_err());
	assert!(workspace
		.drain_asset_jobs()
		.iter()
		.all(|completion| completion.job_id != ticket.job_id));
	assert_eq!(
		workspace
			.backend
			.session()
			.authored_document()
			.entities
			.len(),
		1
	);
}

#[test]
fn scene_binding_edits_return_authoritative_snapshots_and_persist_after_reopen() {
	use bridge::editor::SceneBindingTarget;

	let fixture = Fixture::new();
	let path = fixture.0.join("scenes/main.json");
	let mut document = runtime::scene_io::load_data(&path).unwrap().data;
	let mut second = document.entities[0].clone();
	second.key = EntityKey::new("b").unwrap();
	second.name = "Second".into();
	document.entities.push(second);
	fs::write(&path, document.to_json().unwrap()).unwrap();
	let manifest = fixture.write_project("scenes/main.json");

	let open = || {
		let location = ProjectLocation::from_manifest(&manifest).unwrap();
		EditorWorkspace::open_project_with_notifier(location, || {}).unwrap()
	};
	let mut workspace = open();
	let scene_id = workspace.backend.session().active_scene_id().clone();
	let bind = |target| Command::SetSceneBinding {
		scene_id: scene_id.clone(),
		name: "player".into(),
		target,
	};
	let assign = |key: &str| bind(SceneBindingTarget::Assign { key: key.into() });

	for (command, expected) in [
		(assign("a"), Some("a")),
		(assign("b"), Some("b")),
		(bind(SceneBindingTarget::Clear {}), None),
		(Command::Undo, Some("b")),
		(Command::Undo, Some("a")),
		(Command::Redo, Some("b")),
		(Command::Redo, None),
		(Command::Undo, Some("b")),
	] {
		let update = incremental(&mut workspace, command);
		let EditorCommandUpdateDto::Snapshot { receipt, snapshot } = update else {
			panic!("A binding map change must use the authoritative snapshot");
		};
		assert!(receipt.state_changed);
		assert!(!receipt.project_changed);
		assert_eq!(
			snapshot
				.document
				.bindings
				.get("player")
				.map(EntityKey::as_str),
			expected,
		);
		assert_eq!(
			serde_json::to_value(snapshot).unwrap(),
			serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap(),
		);
	}

	let draft = workspace.backend.snapshot().unwrap();
	assert!(draft.dirty);
	assert!(!draft.project_dirty);
	assert!(runtime::scene_io::load_data(&path)
		.unwrap()
		.data
		.bindings
		.is_empty());

	let saved = workspace.save(draft.revision).unwrap();
	assert!(!saved.dirty);
	assert_eq!(saved.document.bindings["player"].as_str(), "b");

	let reopened = open().backend.snapshot().unwrap();
	assert_eq!(reopened.document.bindings["player"].as_str(), "b");
	assert!(!reopened.dirty);
	assert!(!reopened.can_undo);
}
