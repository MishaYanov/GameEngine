use super::*;

#[test]
fn blank_project_creates_selects_and_exports_an_organized_scene() {
	let fixture = Fixture::new();
	let sandbox = fixture.0.parent().unwrap();
	let blank =
		compiler::create_blank_project("Object Workshop Baseline", sandbox.to_str().unwrap())
			.unwrap();
	let manifest = blank.join("project.json");
	let reopen = || {
		EditorWorkspace::open_project_with_notifier(
			ProjectLocation::from_manifest(&manifest).unwrap(),
			|| {},
		)
		.unwrap()
	};
	let mut workspace = reopen();
	let baseline = workspace.backend.snapshot().unwrap();
	assert_eq!(baseline.active_scene_id, baseline.project.startup_scene_id);
	let default_camera = baseline.document.default_game_camera.unwrap();

	run(&mut workspace, Command::CreateEmpty);
	let created = workspace.backend.snapshot().unwrap();
	let key = EntityKey::new(created.selection[0].as_str()).unwrap();
	assert_ne!(key, default_camera);
	run(
		&mut workspace,
		Command::CreateFolder {
			name: "Floor".into(),
			parent: None,
		},
	);
	let folder = workspace
		.backend
		.snapshot()
		.unwrap()
		.document
		.organization
		.folders[0]
		.id
		.clone();
	run(
		&mut workspace,
		Command::MoveRootsToFolder {
			roots: vec![key.clone()],
			folder: Some(folder.clone()),
		},
	);
	let snapshot = workspace.backend.snapshot().unwrap();
	let entity = snapshot
		.document
		.entities
		.iter()
		.find(|entity| entity.key == key)
		.unwrap();
	assert_eq!(entity.parent, None);
	assert_eq!(
		snapshot.document.organization.root_membership.get(&key),
		Some(&folder)
	);
	assert!(workspace.prop_preview.is_none());
	assert!(workspace.animation_preview.is_none());

	workspace.save_all(snapshot.revision).unwrap();
	drop(workspace);
	let reopened = reopen();
	let snapshot = reopened.backend.snapshot().unwrap();
	assert_eq!(
		snapshot.document.default_game_camera.as_ref(),
		Some(&default_camera)
	);
	assert_eq!(
		snapshot.document.organization.root_membership.get(&key),
		Some(&folder)
	);
	let prepared = reopened.prepare_build(snapshot.revision).unwrap();
	let export = sandbox.join("export");
	fs::create_dir(&export).unwrap();
	let staged = compiler::stage_prepared_project(&prepared, export.to_str().unwrap()).unwrap();
	let staged_scene = runtime::scene_io::load_data(&staged.path().join("assets/scenes/main.json"))
		.unwrap()
		.data;
	assert!(staged_scene.organization.is_empty());
	assert!(staged_scene.entities.iter().any(|entity| entity.key == key));
}
