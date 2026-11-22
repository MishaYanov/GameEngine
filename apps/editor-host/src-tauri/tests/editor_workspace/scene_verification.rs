use super::*;
use bridge::editor::SceneBindingTarget;
use editor::scene_verification::{SceneScope, SceneVerificationReport};
use std::path::Path;

#[test]
fn cinder_scene_verification_follows_retained_repairs_undo_and_stage() {
	let fixture = Fixture::new();
	let manifest = copy_cinder_project(fixture.0.parent().unwrap());
	let mut workspace = open(&manifest);

	let initial = check(&workspace);
	assert_eq!(initial.active_scene_id.as_str(), "intro");
	assert_eq!(initial.scenes.len(), 2);
	assert!(initial.scenes.iter().all(|scene| scene.checked));
	assert!(initial.issues.is_empty());

	clear_role(&mut workspace, "camera_track");
	let target = SceneId::new("arena").unwrap();
	run(
		&mut workspace,
		Command::SwitchScene {
			scene_id: target.clone(),
		},
	);
	clear_role(&mut workspace, "player");

	let captured_revision = workspace.backend.session().revision();
	let capture = workspace
		.capture_scene_verification(Some(workspace.generation), captured_revision, &target)
		.unwrap();
	let problems = capture.verify().unwrap();
	assert_eq!(problems.issues.len(), 2);
	assert!(problems.issues[0].scope == SceneScope::Intro);
	assert!(problems.issues[1].scope == SceneScope::Arena);
	assert_eq!(problems.issues[1].scene_id.as_ref(), Some(&target));

	let player = repair(&mut workspace, "player", &problems);
	assert!(workspace
		.check_scene_verification_current(Some(workspace.generation), captured_revision, &target)
		.is_err());
	assert_eq!(capture.verify().unwrap().issues.len(), 2);
	let partly_repaired = check(&workspace);
	assert_eq!(partly_repaired.issues.len(), 1);
	assert_eq!(
		partly_repaired.scenes[1].content_stamp,
		initial.scenes[1].content_stamp
	);

	let intro = SceneId::new("intro").unwrap();
	run(
		&mut workspace,
		Command::SwitchScene {
			scene_id: intro.clone(),
		},
	);
	let revisit = check(&workspace);
	let track = repair(&mut workspace, "camera_track", &revisit);
	assert!(check(&workspace).issues.is_empty());
	run(&mut workspace, Command::Undo);
	assert_eq!(check(&workspace).issues.len(), 1);
	run(&mut workspace, Command::Redo);
	assert!(check(&workspace).issues.is_empty());

	let revision = workspace.backend.session().revision();
	workspace.save_all(revision).unwrap();
	let revision = workspace.backend.session().revision();
	let prepared = workspace.prepare_build(revision).unwrap();
	let destination = fixture.0.parent().unwrap().join("export");
	fs::create_dir(&destination).unwrap();
	let staged =
		compiler::stage_prepared_project(&prepared, destination.to_str().unwrap()).unwrap();
	let staged_project =
		runtime::project::ProjectData::load(&staged.path().join("project.json")).unwrap();
	for (scene, role, expected) in [(intro, "camera_track", track), (target, "player", player)] {
		let path = staged
			.path()
			.join("assets")
			.join(staged_project.scene_path(&scene).unwrap());
		let loaded = runtime::scene_io::load_data(&path).unwrap();
		assert_eq!(loaded.data.bindings.get(role), Some(&expected));
	}
}

#[test]
fn invalid_cinder_route_blocks_scene_checks_and_rejects_replacement_generation() {
	let fixture = Fixture::new();
	let manifest = copy_cinder_project(fixture.0.parent().unwrap());
	let mut workspace = open(&manifest);
	let replacement = open(&manifest);
	let session = workspace.backend.session();
	assert_eq!(session.revision(), replacement.backend.session().revision());
	assert!(replacement
		.capture_scene_verification(
			Some(workspace.generation),
			session.revision(),
			session.active_scene_id(),
		)
		.is_err());

	let mut project = session.project().clone();
	project.gameplay_settings = None;
	run(&mut workspace, Command::SetProject { project });
	let report = check(&workspace);
	assert_eq!(report.issues.len(), 1);
	assert!(report.issues[0].scope == SceneScope::Project);
	assert!(report.issues[0].scene_id.is_none());
	assert!(report.scenes.iter().all(|scene| !scene.checked));
}

fn open(manifest: &Path) -> EditorWorkspace {
	EditorWorkspace::open_project_with_notifier(
		ProjectLocation::from_manifest(manifest).unwrap(),
		|| {},
	)
		.unwrap()
}

fn check(workspace: &EditorWorkspace) -> SceneVerificationReport {
	let session = workspace.backend.session();
	workspace
		.capture_scene_verification(
			Some(workspace.generation),
			session.revision(),
			session.active_scene_id(),
		)
		.unwrap()
		.verify()
		.unwrap()
}

fn clear_role(workspace: &mut EditorWorkspace, role: &str) {
	let scene_id = workspace.backend.session().active_scene_id().clone();
	run(
		workspace,
		Command::SetSceneBinding {
			scene_id,
			name: role.into(),
			target: SceneBindingTarget::Clear {},
		},
	);
}

fn repair(
	workspace: &mut EditorWorkspace,
	role: &str,
	report: &SceneVerificationReport,
) -> EntityKey {
	let scene_id = workspace.backend.session().active_scene_id().clone();
	let issue = report
		.issues
		.iter()
		.find(|issue| {
			issue.scene_id.as_ref() == Some(&scene_id) && issue.role.as_deref() == Some(role)
		})
		.unwrap();
	let key = issue.suggestion.as_ref().unwrap().entity_key.clone();
	run(
		workspace,
		Command::SetSceneBinding {
			scene_id,
			name: role.into(),
			target: SceneBindingTarget::Assign {
				key: key.as_str().to_owned(),
			},
		},
	);
	key
}

fn copy_cinder_project(destination: &Path) -> PathBuf {
	let project = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("../../..")
		.join(gameplay_catalog::BUNDLED_PROJECT_DIRECTORY);
	let destination = destination.join("cinder");
	copy_files(&project, &destination);
	destination.join("project.json")
}

fn copy_files(source: &Path, destination: &Path) {
	fs::create_dir(destination).unwrap();
	for entry in fs::read_dir(source).unwrap() {
		let entry = entry.unwrap();
		let target = destination.join(entry.file_name());
		if entry.file_type().unwrap().is_dir() {
			copy_files(&entry.path(), &target);
		} else {
			fs::copy(entry.path(), target).unwrap();
		}
	}
}
