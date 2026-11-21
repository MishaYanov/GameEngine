use super::*;

#[test]
fn game_input_profile_save_all_reopens_and_routes_remapped_input() {
	use runtime::{
		input::{GameInputBinding, InputProfile, Key, MovementBindings},
		project::BehaviorSpec,
	};
	use std::collections::HashSet;

	let fixture = Fixture::new();
	let manifest = fixture.write_project("scenes/main.json");
	let scene_bytes = fs::read(fixture.0.join("scenes/main.json")).unwrap();
	let mut workspace = fixture.open();
	let mut project = workspace.backend.session().project().clone();
	let profile = InputProfile {
		movement: MovementBindings::ARROWS,
		actions: vec![GameInputBinding {
			id: "interact".into(),
			key: Key::Space,
			animation_parameter: Some("IsInteracting".into()),
		}],
		..Default::default()
	};
	project.input_profile = Some(profile.clone());
	project.add_behavior(BehaviorSpec::ControlledMove {
		entity: EntityKey::new("a").unwrap(),
		speed: 2.0,
		space: Default::default(),
	});
	run(&mut workspace, Command::SetProject { project });
	assert!(workspace.backend.snapshot().unwrap().project_dirty);
	let saved = workspace
		.save_all(workspace.backend.session().revision())
		.unwrap();
	assert!(!saved.project_dirty);
	assert!(!saved.dirty);
	assert_eq!(
		runtime::project::ProjectData::load(&manifest)
			.unwrap()
			.input_profile,
		Some(profile.clone())
	);

	let mut reopened = EditorWorkspace::open_project_with_notifier(
		ProjectLocation::from_manifest(&manifest).unwrap(),
		|| {},
	)
	.unwrap();
	assert_eq!(
		reopened.backend.session().project().input_profile.as_ref(),
		Some(&profile)
	);
	run(&mut reopened, Command::StartPlay);
	let position = |workspace: &EditorWorkspace| {
		workspace.backend.session().render_document().entities[0]
			.transform
			.position
	};
	reopened
		.backend
		.set_game_keyboard_input_from_codes(&HashSet::from(["KeyD".into()]))
		.unwrap();
	reopened.backend.tick(0.1).unwrap();
	assert_eq!(position(&reopened), [0.0; 3]);
	reopened
		.backend
		.set_game_keyboard_input_from_codes(&HashSet::from(["ArrowRight".into(), "Space".into()]))
		.unwrap();
	reopened.backend.tick(0.1).unwrap();
	let moved = position(&reopened);
	assert!(moved[0] > 0.0);
	assert!(!reopened.backend.session().is_paused());
	reopened.backend.clear_play_input();
	reopened.backend.tick(0.1).unwrap();
	assert_eq!(position(&reopened), moved);
	run(&mut reopened, Command::StopPlay);
	assert_eq!(position(&reopened), [0.0; 3]);
	assert_eq!(
		fs::read(fixture.0.join("scenes/main.json")).unwrap(),
		scene_bytes
	);
}

#[test]
fn play_selects_the_current_gameplay_module_after_history_and_workspace_reopen() {
	use courtyard_gameplay::{CampaignProgress, COURTYARD_RUN_ID};
	use runtime::project::ProjectData;

	let fixture = Fixture::new();
	let manifest = fixture.write_project("scenes/main.json");
	let mut workspace = EditorWorkspace::open_project_with_notifier(
		ProjectLocation::from_manifest(&manifest).unwrap(),
		|| {},
	)
	.unwrap();
	let mut project = workspace.backend.session().project().clone();
	project.gameplay_module = Some(COURTYARD_RUN_ID.into());
	run(&mut workspace, Command::SetProject { project });

	for (edit, campaign) in [
		(None, true),
		(Some(Command::Undo), false),
		(Some(Command::Redo), true),
	] {
		if let Some(command) = edit {
			run(&mut workspace, command);
		}
		run(&mut workspace, Command::StartPlay);
		assert_eq!(
			workspace
				.backend
				.session()
				.play_progress::<CampaignProgress>()
				.is_some(),
			campaign
		);
		run(&mut workspace, Command::StopPlay);
	}

	workspace
		.save_all(workspace.backend.session().revision())
		.unwrap();
	assert_eq!(
		ProjectData::load(&manifest)
			.unwrap()
			.gameplay_module
			.as_deref(),
		Some(COURTYARD_RUN_ID)
	);
	workspace
		.reload_with_policy(
			workspace.backend.session().revision(),
			ReloadPolicy::RequireClean,
		)
		.unwrap();
	run(&mut workspace, Command::StartPlay);
	assert!(workspace
		.backend
		.session()
		.play_progress::<CampaignProgress>()
		.is_some());
	run(&mut workspace, Command::StopPlay);

	let mut project = workspace.backend.session().project().clone();
	project.gameplay_module = Some("missing_module".into());
	let session = workspace.backend.session();
	let (revision, scene_revision, cursor) = (
		session.revision(),
		session.scene_revision(),
		workspace.backend.change_cursor(),
	);
	let error = workspace
		.execute(EditorCommandRequest {
			expected_revision: revision,
			command: Command::SetProject { project },
		})
		.unwrap_err();
	assert!(error.contains("missing_module"));
	let session = workspace.backend.session();
	assert!(!session.is_playing());
	assert_eq!(session.revision(), revision);
	assert_eq!(session.scene_revision(), scene_revision);
	assert_eq!(workspace.backend.change_cursor(), cursor);
	assert_eq!(
		session.project().gameplay_module.as_deref(),
		Some(COURTYARD_RUN_ID)
	);
}

#[test]
fn editor_play_completes_in_the_retained_draft_and_restores_the_authoring_scene_on_stop() {
	use bridge::editor::SceneBindingTarget;
	use courtyard_gameplay::{CampaignProgress, COURTYARD_RUN_ID};
	use runtime::{
		events::BackendEventKind,
		project::{BehaviorSpace, BehaviorSpec, ProjectData, SceneBehavior},
		scene_data::TransformData,
	};
	use std::collections::HashSet;

	let fixture = Fixture::new();
	let scene_path = fixture.0.join("scenes/main.json");
	let mut data = runtime::scene_io::load_data(&scene_path).unwrap().data;
	let mut finish = data.clone();
	let finish_player = EntityKey::new("finish-player").unwrap();
	let finish_marker = EntityKey::new("finish-marker").unwrap();
	finish.entities[0].key = finish_player.clone();
	finish.entities[0].name = "Saved destination".into();
	finish.entities.push(
		EntityData::new(finish_marker.clone(), "Finish", "models/triangle.glb").with_transform(
			TransformData {
				position: [1.0, 0.0, 0.0],
				..Default::default()
			},
		),
	);
	finish
		.bindings
		.insert("player".into(), finish_player.clone());
	finish.bindings.insert("finish".into(), finish_marker);
	finish.camera.position = [1.0, 2.0, 5.0];
	let finish_id = SceneId::new("finish").unwrap();
	let finish_path = fixture.0.join("scenes/finish.json");
	let finish_bytes = finish.to_json().unwrap();
	fs::write(&finish_path, &finish_bytes).unwrap();
	for (key, x) in [("key-marker", -1.0), ("exit-marker", 1.0)] {
		data.entities.push(
			EntityData::new(EntityKey::new(key).unwrap(), key, "models/triangle.glb")
				.with_transform(TransformData {
					position: [x, 0.0, 0.0],
					..Default::default()
				}),
		);
	}
	let mut project =
		ProjectData::for_scene("Courtyard workspace", "scenes/main.json", &data).unwrap();
	project.gameplay_module = Some(COURTYARD_RUN_ID.into());
	project.add_behavior(BehaviorSpec::ControlledMove {
		entity: EntityKey::new("a").unwrap(),
		speed: 1.0,
		space: BehaviorSpace::World,
	});
	project
		.scenes
		.insert(SceneDescriptor::new(finish_id.clone(), "scenes/finish.json").unwrap())
		.unwrap();
	project.behaviors.push(SceneBehavior {
		scene_id: finish_id.clone(),
		behavior: BehaviorSpec::ControlledMove {
			entity: finish_player.clone(),
			speed: 1.0,
			space: BehaviorSpace::World,
		},
	});
	let scene_id = project.startup_scene_id.clone();
	let elapsed = 1.0 / f64::from(project.fixed_hz);
	let manifest = fixture.0.parent().unwrap().join("project.json");
	fs::write(&scene_path, data.to_json().unwrap()).unwrap();
	fs::write(&manifest, project.to_json().unwrap()).unwrap();
	let mut workspace = EditorWorkspace::open_project_with_notifier(
		ProjectLocation::from_manifest(&manifest).unwrap(),
		|| {},
	)
	.unwrap();
	for (name, key) in [
		("player", "a"),
		("key", "key-marker"),
		("exit", "exit-marker"),
	] {
		run(
			&mut workspace,
			Command::SetSceneBinding {
				scene_id: scene_id.clone(),
				name: name.into(),
				target: SceneBindingTarget::Assign { key: key.into() },
			},
		);
	}
	workspace
		.save_all(workspace.backend.session().revision())
		.unwrap();
	let saved_bytes = fs::read(&scene_path).unwrap();
	let authored = workspace.backend.session().authored_document();
	run(
		&mut workspace,
		Command::SwitchScene {
			scene_id: finish_id.clone(),
		},
	);
	run(
		&mut workspace,
		Command::Rename {
			key: finish_player.as_str().into(),
			name: "Unsaved destination".into(),
		},
	);
	let finish_draft = workspace.backend.session().authored_document();
	run(
		&mut workspace,
		Command::SwitchScene {
			scene_id: scene_id.clone(),
		},
	);

	run(&mut workspace, Command::StartPlay);
	let walk = |workspace: &mut EditorWorkspace, x: f32, ticks| {
		run(
			workspace,
			Command::SetPlayInput {
				movement: [x, 0.0, 0.0],
			},
		);
		let mut requested = None;
		for _ in 0..ticks {
			if let Some(target) = workspace.backend.tick(elapsed).unwrap().requested_scene {
				assert!(requested.replace(target).is_none());
			}
		}
		requested
	};
	let progress = |workspace: &EditorWorkspace| {
		workspace
			.backend
			.session()
			.play_progress::<CampaignProgress>()
			.unwrap()
			.clone()
	};

	assert!(walk(&mut workspace, 1.0, 60).is_none());
	let runtime = workspace.backend.session().render_document();
	assert!(runtime.entities[0].transform.position[0] > 0.9);
	assert!(!progress(&workspace).has_courtyard_key);
	assert!(!progress(&workspace).courtyard_exit_reached);
	assert!(walk(&mut workspace, -1.0, 120).is_none());
	assert!(progress(&workspace).has_courtyard_key);
	assert!(!progress(&workspace).courtyard_exit_reached);
	assert_eq!(walk(&mut workspace, 1.0, 120), Some(finish_id.clone()));
	assert!(progress(&workspace).courtyard_exit_reached);
	assert_eq!(progress(&workspace).score, 1);
	assert_eq!(workspace.backend.session().play_scene_id(), Some(&scene_id));
	assert_eq!(workspace.backend.session().authored_document(), authored);

	// Closing an unsuccessful GPU preparation leaves the original play scene active.
	let (candidate, document) = workspace.backend.prepare_pending_play_scene().unwrap();
	assert_eq!(document, finish_draft);
	drop(candidate);
	assert_eq!(workspace.backend.session().play_scene_id(), Some(&scene_id));
	let next_tick = workspace.backend.tick(elapsed).unwrap();
	assert!(next_tick.requested_scene.is_none());

	let (candidate, _) = workspace.backend.prepare_pending_play_scene().unwrap();
	let revision = workspace.backend.session().revision();
	let scene_revision = workspace.backend.session().scene_revision();
	let cursor = workspace.backend.change_cursor();
	workspace.backend.commit_play_scene(candidate).unwrap();
	let session = workspace.backend.session();
	assert_eq!(session.revision(), revision);
	assert_eq!(session.scene_revision(), scene_revision + 1);
	assert_eq!(session.active_scene_id(), &scene_id);
	assert_eq!(session.play_scene_id(), Some(&finish_id));
	assert_eq!(session.render_document(), finish_draft);
	assert_eq!(progress(&workspace).score, 1);
	assert!(progress(&workspace).courtyard_exit_reached);
	let changes = workspace.backend.changes_since(cursor).unwrap();
	assert!(changes.events.iter().any(|event| {
		matches!(&event.event, BackendEventKind::RuntimeSceneChanged { from, to }
			if from == &scene_id && to == &finish_id)
	}));
	assert_eq!(workspace.backend.tick(elapsed).unwrap().steps, 0);
	assert!(!progress(&workspace).completed);

	workspace
		.backend
		.set_game_keyboard_input_from_codes(&HashSet::from(["KeyD".into()]))
		.unwrap();
	let mut completion_changes = 0;
	for _ in 0..90 {
		let tick = workspace.backend.tick(elapsed).unwrap();
		completion_changes += usize::from(tick.progress_changed);
	}
	let completed = progress(&workspace);
	assert!(completed.completed);
	assert!(completed.courtyard_exit_reached);
	assert!(completed.has_courtyard_key);
	assert_eq!(completed.score, 1);
	assert_eq!(completion_changes, 1);
	let frozen = workspace.backend.session().render_document();
	assert_ne!(
		frozen.entities[0].transform,
		finish_draft.entities[0].transform
	);

	workspace
		.backend
		.set_game_keyboard_input_from_codes(&HashSet::from(["KeyA".into()]))
		.unwrap();
	assert!(!workspace.backend.session().needs_tick());
	let after_completion = workspace.backend.tick(elapsed).unwrap();
	assert_eq!(after_completion.steps, 0);
	assert!(!after_completion.progress_changed);
	assert_eq!(workspace.backend.session().render_document(), frozen);
	assert_eq!(workspace.backend.session().authored_document(), authored);

	workspace
		.execute(EditorCommandRequest {
			expected_revision: revision,
			command: Command::StopPlay,
		})
		.unwrap();
	let session = workspace.backend.session();
	assert_eq!(session.render_document(), authored);
	assert_eq!(session.scene_document(&finish_id).unwrap(), finish_draft);
	assert!(session.is_dirty());
	assert!(session.play_progress::<CampaignProgress>().is_none());
	assert_eq!(fs::read(&scene_path).unwrap(), saved_bytes);
	assert_eq!(fs::read_to_string(&finish_path).unwrap(), finish_bytes);
}

#[test]
fn save_all_rejects_grouped_edits_play_and_invalid_paths_before_any_writes() {
	let (fixture, _, _) = two_scene_project();
	let mut workspace = fixture.open();
	let original = fs::read(&workspace.scene_path).unwrap();
	run(
		&mut workspace,
		Command::Rename {
			key: "a".into(),
			name: "Keep dirty".into(),
		},
	);
	run(
		&mut workspace,
		Command::BeginTransaction {
			label: "In progress".into(),
		},
	);
	assert!(workspace
		.save_all(workspace.backend.session().revision())
		.is_err());
	run(&mut workspace, Command::CancelTransaction);
	run(&mut workspace, Command::StartPlay);
	assert!(workspace
		.save_all(workspace.backend.session().revision())
		.is_err());
	run(&mut workspace, Command::StopPlay);
	let other = fixture.0.join("scenes/second.json");
	fs::rename(&other, other.with_extension("held")).unwrap();
	fs::create_dir(&other).unwrap();
	assert!(workspace
		.save_all(workspace.backend.session().revision())
		.is_err());
	assert_eq!(fs::read(&workspace.scene_path).unwrap(), original);
	assert!(workspace.backend.session().is_dirty());
}

#[test]
fn saving_in_play_persists_authored_document_and_reloading_stops_simulation() {
	let fixture = Fixture::new();
	let mut w = fixture.open();
	let mut project = w.backend.session().project().clone();
	project.add_behavior(runtime::project::BehaviorSpec::ControlledMove {
		entity: EntityKey::new("a").unwrap(),
		speed: 2.0,
		space: Default::default(),
	});
	run(&mut w, Command::SetProject { project });
	run(
		&mut w,
		Command::Select {
			keys: vec!["a".into()],
		},
	);
	run(
		&mut w,
		Command::Rename {
			key: "a".into(),
			name: "Authored".into(),
		},
	);
	let authored = w.backend.session().authored_document();
	run(&mut w, Command::StartPlay);
	run(
		&mut w,
		Command::SetPlayInput {
			movement: [1.0, 0.0, 0.0],
		},
	);
	w.backend.tick(0.1).unwrap();
	assert_ne!(w.backend.session().render_document(), authored);
	let revision = w.backend.session().revision();
	w.save(revision).unwrap();
	let disk = runtime::scene_io::load_data(&w.scene_path).unwrap().data;
	assert_eq!(&disk, authored.as_ref());
	assert!(w.backend.session().is_playing());
	w.reload_with_policy(w.backend.session().revision(), ReloadPolicy::DiscardUnsaved)
		.unwrap();
	assert!(!w.backend.session().is_playing());
	assert_eq!(w.backend.session().render_document(), authored);
	assert!(!w.backend.session().snapshot().unwrap().can_undo);
}

#[test]
fn project_save_failure_play_and_grouped_edit_do_not_clear_unsaved_project_settings() {
	let fixture = Fixture::new();
	let mut w = fixture.open();
	let mut project = w.backend.session().project().clone();
	project.name = "Unsaved project".into();
	run(&mut w, Command::SetProject { project });
	let original_path = w.project_path.clone();
	w.project_path = fixture.0.join("missing/project.json");
	let revision = w.backend.session().revision();
	assert!(w.save_project(revision).is_err());
	assert_eq!(w.backend.session().revision(), revision);
	assert!(w.backend.session().is_dirty());
	w.project_path = original_path;
	run(
		&mut w,
		Command::BeginTransaction {
			label: "Draft".into(),
		},
	);
	assert!(w.save_project(w.backend.session().revision()).is_err());
	assert!(w.prepare_build(w.backend.session().revision()).is_err());
	run(&mut w, Command::CancelTransaction);
	run(&mut w, Command::StartPlay);
	assert!(w.save_project(w.backend.session().revision()).is_err());
	assert!(w.prepare_build(w.backend.session().revision()).is_err());
	assert!(w.backend.session().is_playing());
}
