use super::*;
use editor::documents::SaveOutcome;

fn rename(workspace: &mut EditorWorkspace, name: &str) {
	run(
		workspace,
		Command::Rename {
			key: "a".into(),
			name: name.into(),
		},
	);
}

fn checkpoint_path(fixture: &Fixture) -> PathBuf {
	fixture
		.0
		.parent()
		.unwrap()
		.join(".engine-draft-recovery.json")
}

fn write_checkpoint(workspace: &mut EditorWorkspace) {
	let candidate = workspace
		.capture_recovery()
		.unwrap()
		.unwrap()
		.stage()
		.unwrap();
	assert!(workspace.publish_recovery(candidate).unwrap());
}

#[test]
fn save_detects_external_edits_and_deletion_before_writing_any_planned_file() {
	let fixture = Fixture::new();
	let manifest = fixture.write_project("scenes/main.json");
	let mut workspace = fixture.open();
	let scene = workspace.scene_path.clone();
	let original_scene = fs::read(&scene).unwrap();
	let original_manifest = fs::read(&manifest).unwrap();
	let mut externally_changed = original_scene.clone();
	externally_changed.push(b' ');
	fs::write(&scene, &externally_changed).unwrap();
	rename(&mut workspace, "Retained edit");
	let mut project = workspace.backend.session().project().clone();
	project.name = "Renamed project".into();
	run(&mut workspace, Command::SetProject { project });
	let revision = workspace.backend.session().revision();
	assert!(workspace
		.save(revision)
		.unwrap_err()
		.contains("changed on disk"));
	let report = workspace.save_all_report(revision).unwrap();
	assert_eq!(report.files[0].outcome, SaveOutcome::Conflict);
	assert_eq!(report.files[1].outcome, SaveOutcome::Blocked);
	assert!(report.snapshot.dirty);
	assert_eq!(fs::read(&scene).unwrap(), externally_changed);
	assert_eq!(fs::read(&manifest).unwrap(), original_manifest);

	fs::write(&scene, &original_scene).unwrap();
	fs::remove_file(&manifest).unwrap();
	assert!(workspace
		.save_project(revision)
		.unwrap_err()
		.contains("changed on disk"));
	let report = workspace.save_all_report(revision).unwrap();
	assert_eq!(report.files[0].outcome, SaveOutcome::Blocked);
	assert_eq!(report.files[1].outcome, SaveOutcome::Conflict);
	assert_eq!(fs::read(&scene).unwrap(), original_scene);

	fs::write(&manifest, &original_manifest).unwrap();
	let report = workspace.save_all_report(revision).unwrap();
	assert!(report
		.files
		.iter()
		.all(|file| file.outcome == SaveOutcome::Written));
	assert!(!report.snapshot.dirty);
}

#[test]
fn register_and_save_as_capture_destination_bytes_and_reject_later_modification() {
	let fixture = Fixture::new();
	fixture.write_project("scenes/main.json");
	let mut workspace = fixture.open();
	let file = fixture.0.join("scenes/registered.json");
	fs::copy(&workspace.scene_path, &file).unwrap();
	let registered = workspace
		.register_scene(
			"scenes/registered.json",
			workspace.backend.session().revision(),
		)
		.unwrap();
	let scene_id = registered
		.scenes
		.iter()
		.find(|s| s.asset_path == "scenes/registered.json")
		.unwrap()
		.id
		.clone();
	run(&mut workspace, Command::SwitchScene { scene_id });
	let mut modified = fs::read(&file).unwrap();
	modified.push(b' ');
	fs::write(&file, &modified).unwrap();
	assert!(workspace
		.save(workspace.backend.session().revision())
		.unwrap_err()
		.contains("changed on disk"));
	assert_eq!(fs::read(&file).unwrap(), modified);
	workspace
		.save_scene_as("scenes/copied.json", workspace.backend.session().revision())
		.unwrap();
	let copied = fixture.0.join("scenes/copied.json");
	let copied_base = fs::read(&copied).unwrap();
	let mut modified_copy = copied_base.clone();
	modified_copy.push(b' ');
	fs::write(&copied, &modified_copy).unwrap();
	assert!(workspace
		.save(workspace.backend.session().revision())
		.unwrap_err()
		.contains("changed on disk"));
	assert_eq!(fs::read(&copied).unwrap(), modified_copy);
	fs::write(&copied, copied_base).unwrap();
	workspace
		.save(workspace.backend.session().revision())
		.unwrap();
}

#[test]
fn checkpoints_restore_unsaved_scenes_and_project_catalog_without_writing_disk() {
	let fixture = Fixture::new();
	let manifest = fixture.write_project("scenes/main.json");
	let mut workspace = fixture.open();
	let scene = workspace.scene_path.clone();
	let original_scene = fs::read(&scene).unwrap();
	let original_manifest = fs::read(&manifest).unwrap();
	rename(&mut workspace, "Draft before recovery");
	let new_scene = SceneId::new("unsaved").unwrap();
	let document = (*workspace.backend.session().authored_document()).clone();
	run(
		&mut workspace,
		Command::AddScene {
			id: new_scene.clone(),
			asset_path: "scenes/new.json".into(),
			document,
		},
	);
	run(
		&mut workspace,
		Command::SwitchScene {
			scene_id: new_scene.clone(),
		},
	);
	rename(&mut workspace, "New scene draft");
	write_checkpoint(&mut workspace);
	drop(workspace);
	assert_eq!(fs::read(&scene).unwrap(), original_scene);
	assert_eq!(fs::read(&manifest).unwrap(), original_manifest);
	assert!(!fixture.0.join("scenes/new.json").exists());

	let mut reopened = fixture.open();
	let offer = reopened.review_recovery().unwrap().unwrap();
	assert!(offer.issue.is_none());
	assert_eq!(offer.files.len(), 3);
	let stale_token = [0; 32];
	assert!(reopened
		.apply_recovery(reopened.backend.session().revision(), stale_token, true)
		.is_err());
	let recovered = reopened
		.apply_recovery(reopened.backend.session().revision(), offer.token, true)
		.unwrap();
	assert!(recovered.project_dirty);
	assert!(recovered.scenes.iter().all(|scene| scene.dirty));
	assert_eq!(recovered.active_scene_id, new_scene);
	assert_eq!(recovered.document.entities[0].name, "New scene draft");
	assert!(!recovered.can_undo);
	assert_eq!(fs::read(&scene).unwrap(), original_scene);
	assert_eq!(fs::read(&manifest).unwrap(), original_manifest);
	assert!(!fixture.0.join("scenes/new.json").exists());
	assert!(
		!reopened
			.save_all(reopened.backend.session().revision())
			.unwrap()
			.dirty
	);
	assert!(!checkpoint_path(&fixture).exists());
}

#[test]
fn recovery_recognizes_an_interrupted_save_whose_scene_already_matches_the_draft() {
	let fixture = Fixture::new();
	fixture.write_project("scenes/main.json");
	let mut workspace = fixture.open();
	rename(&mut workspace, "Scene written before interruption");
	let mut project = workspace.backend.session().project().clone();
	project.name = "Project that remains unsaved".into();
	run(&mut workspace, Command::SetProject { project });
	write_checkpoint(&mut workspace);
	workspace
		.save(workspace.backend.session().revision())
		.unwrap();
	drop(workspace);

	let mut reopened = fixture.open();
	let offer = reopened.review_recovery().unwrap().unwrap();
	assert!(offer.issue.is_none(), "{:?}", offer.issue);
	let recovered = reopened
		.apply_recovery(reopened.backend.session().revision(), offer.token, true)
		.unwrap();
	assert!(recovered.project_dirty);
	assert!(recovered.scenes.iter().all(|scene| !scene.dirty));
	assert_eq!(
		recovered.document.entities[0].name,
		"Scene written before interruption"
	);
	assert_eq!(recovered.project.name, "Project that remains unsaved");
}

#[test]
fn changed_and_wrong_scope_checkpoints_are_preserved_for_explicit_review() {
	let fixture = Fixture::new();
	fixture.write_project("scenes/main.json");
	let mut workspace = fixture.open();
	rename(&mut workspace, "Draft");
	write_checkpoint(&mut workspace);
	let checkpoint = checkpoint_path(&fixture);
	let original = fs::read(&checkpoint).unwrap();
	rename(&mut workspace, "New draft");
	let staged = workspace
		.capture_recovery()
		.unwrap()
		.unwrap()
		.stage()
		.unwrap();
	fs::write(&checkpoint, b"corrupt external recovery file").unwrap();
	assert!(workspace
		.publish_recovery(staged)
		.unwrap_err()
		.contains("requires review"));
	assert_eq!(
		fs::read(&checkpoint).unwrap(),
		b"corrupt external recovery file"
	);
	let offer = workspace.review_recovery().unwrap().unwrap();
	assert!(offer.issue.unwrap().contains("corrupt"));

	let wrong_project = Fixture::new();
	wrong_project.write_project("scenes/main.json");
	fs::write(checkpoint_path(&wrong_project), original).unwrap();
	let mut reopened = wrong_project.open();
	let offer = reopened.review_recovery().unwrap().unwrap();
	assert!(offer.issue.unwrap().contains("different project location"));
	assert!(reopened
		.apply_recovery(reopened.backend.session().revision(), offer.token, true)
		.is_err());
	assert!(
		!reopened
			.apply_recovery(reopened.backend.session().revision(), offer.token, false)
			.unwrap()
			.dirty
	);
	assert!(!checkpoint_path(&wrong_project).exists());
}

#[test]
fn recovery_is_bounded_and_skips_grouped_edits() {
	let fixture = Fixture::new();
	fixture.write_project("scenes/main.json");
	let mut workspace = fixture.open();
	run(
		&mut workspace,
		Command::BeginTransaction {
			label: "drag".into(),
		},
	);
	rename(&mut workspace, "Preview only");
	assert!(workspace.capture_recovery().unwrap().is_none());
	run(&mut workspace, Command::CancelTransaction);
	assert!(workspace.capture_recovery().unwrap().is_none());
	drop(workspace);
	let file = fs::File::create(checkpoint_path(&fixture)).unwrap();
	file.set_len(32 * 1024 * 1024 + 1).unwrap();
	let mut reopened = fixture.open();
	let offer = reopened.review_recovery().unwrap().unwrap();
	assert!(offer.issue.unwrap().contains("byte limit"));
	assert!(reopened
		.apply_recovery(reopened.backend.session().revision(), offer.token, true)
		.is_err());
	drop(file);
	assert!(
		!reopened
			.apply_recovery(reopened.backend.session().revision(), offer.token, false)
			.unwrap()
			.dirty
	);
	assert!(!checkpoint_path(&fixture).exists());
}

#[test]
fn missing_clean_catalog_scene_prevents_manifest_publication() {
	let (fixture, _, _) = two_scene_project();
	let mut workspace = fixture.open();
	let manifest = fs::read(&workspace.project_path).unwrap();
	let main = fs::read(&workspace.scene_path).unwrap();
	let mut project = workspace.backend.session().project().clone();
	let missing = project
		.scenes
		.iter()
		.find(|entry| entry.id() != &project.startup_scene_id)
		.unwrap()
		.asset_path()
		.to_owned();
	fs::remove_file(fixture.0.join(&missing)).unwrap();
	project.name = "Must remain in memory".into();
	run(&mut workspace, Command::SetProject { project });
	rename(&mut workspace, "Must remain unsaved");
	let report = workspace
		.save_all_report(workspace.backend.session().revision())
		.unwrap();
	assert!(report.snapshot.dirty && report.snapshot.project_dirty);
	assert!(report
		.files
		.iter()
		.any(|file| file.path == missing && file.outcome == SaveOutcome::Conflict));
	assert!(report
		.files
		.iter()
		.any(|file| file.path == "project.json" && file.outcome == SaveOutcome::Blocked));
	assert_eq!(fs::read(&workspace.project_path).unwrap(), manifest);
	assert_eq!(fs::read(&workspace.scene_path).unwrap(), main);
}
