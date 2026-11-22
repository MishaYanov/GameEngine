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
