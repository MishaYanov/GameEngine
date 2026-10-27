// Compile the CPU workspace service directly; Tauri's WebView module stays private.
#[path = "../src/editor.rs"]
mod editor;
#[path = "../src/project_location.rs"]
mod project_location;
use bridge::editor::{EditorCommandDto as Command, EditorCommandRequest};
use bridge::editor::{EditorCommandUpdateDto, EditorSnapshotDto};
use editor::{EditorWorkspace, ReloadPolicy};
use project_location::{ProjectLocation, ProjectOpenError};
use runtime::scene_catalog::{SceneDescriptor, SceneId};
use runtime::{scene_data::EntityData, EntityKey};
use std::{
	fs,
	path::PathBuf,
	sync::atomic::{AtomicU64, Ordering},
};

struct Fixture(PathBuf);
impl Fixture {
	fn new() -> Self {
		static NEXT: AtomicU64 = AtomicU64::new(0);
		let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
			.join("../../../target")
			.join(format!(
				"qa-workspace-{}-{}",
				std::process::id(),
				NEXT.fetch_add(1, Ordering::Relaxed)
			));
		fs::create_dir(&root).unwrap();
		let path = root.join("assets");
		fs::create_dir(&path).unwrap();
		fs::create_dir(path.join("scenes")).unwrap();
		fs::create_dir(path.join("prefabs")).unwrap();
		fs::create_dir(path.join("models")).unwrap();
		fs::copy(
			PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../game/assets/props/robot.glb"),
			path.join("models/triangle.glb"),
		)
		.unwrap();
		let camera = runtime::Camera::default();
		let data = runtime::scene_data::SceneData {
			schema_version: runtime::scene_data::CURRENT_SCHEMA_VERSION,
			bindings: Default::default(),
			entities: vec![EntityData::new(
				EntityKey::new("a").unwrap(),
				"Original",
				"models/triangle.glb",
			)],
			camera: runtime::scene_data::CameraData {
				position: camera.position,
				yaw: camera.yaw,
				pitch: camera.pitch,
				vertical_fov: camera.vertical_fov,
				near_plane: camera.near,
				far_plane: camera.far,
			},
			lights: vec![],
		};
		fs::write(path.join("scenes/main.json"), data.to_json().unwrap()).unwrap();
		Self(path)
	}
	fn open(&self) -> EditorWorkspace {
		EditorWorkspace::open(self.0.to_str().unwrap(), "scenes/main.json").unwrap()
	}
	fn write_project(&self, startup_scene: &str) -> PathBuf {
		let scene = runtime::scene_io::load_data(&self.0.join("scenes/main.json"))
			.unwrap()
			.data;
		let project =
			runtime::project::ProjectData::for_scene("Selected project", startup_scene, &scene)
				.unwrap();
		let manifest = self.0.parent().unwrap().join("project.json");
		fs::write(&manifest, project.to_json().unwrap()).unwrap();
		manifest
	}
}
impl Drop for Fixture {
	fn drop(&mut self) {
		// This fixture exclusively created the project parent and asset child.
		let _ = fs::remove_dir_all(self.0.parent().unwrap());
	}
}
fn run(workspace: &mut EditorWorkspace, command: Command) {
	workspace
		.execute(EditorCommandRequest {
			expected_revision: workspace.backend.session().revision(),
			command,
		})
		.unwrap();
}

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

fn two_scene_project() -> (Fixture, runtime::project::ProjectData, SceneId) {
	let fixture = Fixture::new();
	let main = runtime::scene_io::load_data(&fixture.0.join("scenes/main.json"))
		.unwrap()
		.data;
	let mut secondary = main.clone();
	secondary.entities[0].name = "Second original".into();
	fs::write(
		fixture.0.join("scenes/second.json"),
		secondary.to_json().unwrap(),
	)
	.unwrap();
	let mut project =
		runtime::project::ProjectData::for_scene("Two scenes", "scenes/main.json", &main).unwrap();
	let second = SceneId::new("second").unwrap();
	project
		.scenes
		.insert(SceneDescriptor::new(second.clone(), "scenes/second.json").unwrap())
		.unwrap();
	fs::write(
		fixture.0.parent().unwrap().join("project.json"),
		project.to_json().unwrap(),
	)
	.unwrap();
	(fixture, project, second)
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
	run(
		&mut workspace,
		Command::SwitchScene {
			scene_id: second.clone(),
		},
	);
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
		schema_version: 1,
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

fn assert_missing_manifest_reload_preserves_edits(workspace: &mut EditorWorkspace) {
	let original_scene = workspace.backend.session().authored_document();
	let original_project = workspace.backend.session().project().clone();
	run(
		workspace,
		Command::Rename {
			key: "a".into(),
			name: "Unsaved actor".into(),
		},
	);
	let mut edited_project = original_project.clone();
	edited_project.name = "Unsaved project settings".into();
	run(
		workspace,
		Command::SetProject {
			project: edited_project.clone(),
		},
	);
	let before = serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap();
	let revision = workspace.backend.session().revision();
	let cursor = workspace.backend.change_cursor();
	let generation = workspace.generation;
	let paths = (
		workspace.asset_root.clone(),
		workspace.project_path.clone(),
		workspace.scene_path.clone(),
	);
	let disk_scene = fs::read(&workspace.scene_path).unwrap();
	assert!(workspace.backend.session().is_dirty());
	fs::remove_file(&workspace.project_path).unwrap();
	assert!(
		workspace
			.reload_with_policy(revision, ReloadPolicy::DiscardUnsaved)
			.is_err(),
		"A manifest-backed workspace must not reload without its manifest"
	);
	assert_eq!(
		serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap(),
		before
	);
	assert_eq!(workspace.backend.change_cursor(), cursor);
	assert_eq!(workspace.generation, generation);
	assert_eq!(
		(
			&workspace.asset_root,
			&workspace.project_path,
			&workspace.scene_path
		),
		(&paths.0, &paths.1, &paths.2)
	);
	assert_eq!(fs::read(&workspace.scene_path).unwrap(), disk_scene);
	// Exercise the retained history rather than relying only on snapshot flags.
	run(workspace, Command::Undo);
	assert_eq!(workspace.backend.session().project(), &original_project);
	assert_eq!(
		workspace.backend.session().authored_document().entities[0].name,
		"Unsaved actor"
	);
	run(workspace, Command::Undo);
	assert_eq!(
		workspace.backend.session().authored_document(),
		original_scene
	);
	run(workspace, Command::Redo);
	run(workspace, Command::Redo);
	assert_eq!(workspace.backend.session().project(), &edited_project);
	assert_eq!(
		workspace.backend.session().authored_document().entities[0].name,
		"Unsaved actor"
	);
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

fn async_workspace(fixture: &Fixture) -> (EditorWorkspace, std::sync::mpsc::Receiver<()>) {
	fs::write(fixture.0.join("models/hierarchy.gltf"), r#"{"asset":{"version":"2.0"},"scenes":[{"nodes":[0]}],"nodes":[{"name":"Root","translation":[1,0,0],"children":[1]},{"name":"Child","translation":[0,2,0]}]}"#).unwrap();
	let (notify, ready) = std::sync::mpsc::channel();
	let workspace = EditorWorkspace::open_with_notifier(
		fixture.0.to_str().unwrap(),
		"scenes/main.json",
		move || {
			let _ = notify.send(());
		},
	)
	.unwrap();
	(workspace, ready)
}
fn await_completion(
	workspace: &mut EditorWorkspace,
	ready: &std::sync::mpsc::Receiver<()>,
	id: runtime::asset_jobs::AssetJobId,
) -> editor::AssetCompletion {
	let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
	loop {
		if let Some(completion) = workspace
			.drain_asset_jobs()
			.into_iter()
			.find(|c| c.job_id == id)
		{
			return completion;
		}
		let remaining = deadline.saturating_duration_since(std::time::Instant::now());
		assert!(!remaining.is_zero(), "asset adoption did not finish");
		ready
			.recv_timeout(remaining)
			.expect("asset completion notification");
	}
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
fn async_model_import_adopts_one_current_revision_edit_and_is_undoable() {
	let fixture = Fixture::new();
	let (mut w, ready) = async_workspace(&fixture);
	let before = w.backend.session().authored_document();
	let ticket = w
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

fn metadata_import_workspace(
	fixture: &Fixture,
	root_x: f32,
) -> (EditorWorkspace, std::sync::mpsc::Receiver<()>) {
	let scene_path = fixture.0.join("scenes/main.json");
	let mut data = runtime::scene_io::load_data(&scene_path).unwrap().data;
	data.entities[0].renderable = false;
	fs::write(scene_path, data.to_json().unwrap()).unwrap();
	let mut bytes = Vec::new();
	for value in [0.0_f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
		bytes.extend_from_slice(&value.to_le_bytes());
	}
	fs::write(fixture.0.join("models/vertices.bin"), bytes).unwrap();
	let model = serde_json::json!({"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],"nodes":[{"name":"Imported","mesh":1,"translation":[root_x,0.0,0.0]}],
        "buffers":[{"byteLength":36,"uri":"vertices.bin"}],"bufferViews":[{"buffer":0,"byteLength":36}],
        "accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]}],
        "meshes":[{"primitives":[{"attributes":{"POSITION":0}}]},{"primitives":[{"attributes":{"POSITION":0}}]}]});
	fs::write(fixture.0.join("models/selected.gltf"), model.to_string()).unwrap();
	async_workspace(fixture)
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

/// Sequential acceptance of the manual step 46 backend flow. This exercises
/// real workspace/file operations, not the Tauri WebView or its installation slot.
#[test]
fn step46_manual_lifecycle_flow_is_automated() {
	fn snapshot(workspace: &EditorWorkspace) -> serde_json::Value {
		serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap()
	}
	fn rename(workspace: &mut EditorWorkspace, name: &str) {
		run(
			workspace,
			Command::Rename {
				key: "a".into(),
				name: name.into(),
			},
		);
	}
	fn configure(workspace: &mut EditorWorkspace, name: &str) {
		let mut project = workspace.backend.session().project().clone();
		project.name = name.into();
		run(workspace, Command::SetProject { project });
	}
	fn hide_manifest_and_reject_reload(workspace: &mut EditorWorkspace) -> PathBuf {
		let before = snapshot(workspace);
		let cursor = workspace.backend.change_cursor();
		let generation = workspace.generation;
		let paths = (
			workspace.asset_root.clone(),
			workspace.project_path.clone(),
			workspace.scene_path.clone(),
		);
		let scene_bytes = fs::read(&workspace.scene_path).unwrap();
		let held = workspace.project_path.with_extension("json.held");
		assert!(!held.exists());
		fs::rename(&workspace.project_path, &held).unwrap();
		let error = workspace
			.reload_with_policy(
				workspace.backend.session().revision(),
				ReloadPolicy::DiscardUnsaved,
			)
			.unwrap_err();
		assert!(error.contains("Cannot read project:"), "{error}");
		assert_eq!(snapshot(workspace), before);
		assert_eq!(workspace.backend.change_cursor(), cursor);
		assert_eq!(workspace.generation, generation);
		assert_eq!(
			(
				&workspace.asset_root,
				&workspace.project_path,
				&workspace.scene_path
			),
			(&paths.0, &paths.1, &paths.2)
		);
		assert_eq!(fs::read(&workspace.scene_path).unwrap(), scene_bytes);
		assert!(!workspace.project_path.exists());
		held
	}

	let fixture = Fixture::new();
	let manifest = fixture.write_project("scenes/main.json");
	let mut workspace = EditorWorkspace::open_project_with_notifier(
		ProjectLocation::from_manifest(&manifest).unwrap(),
		|| {},
	)
	.unwrap();
	assert!(!workspace.backend.snapshot().unwrap().dirty);
	rename(&mut workspace, "Keep A unsaved");

	// Validate the bad candidate using the real constructor. Workspace slot
	// replacement is private to the Tauri adapter and is not simulated here.
	let invalid_fixture = Fixture::new();
	let invalid_manifest = invalid_fixture.write_project("scenes/missing.json");
	assert!(EditorWorkspace::open_project_with_notifier(
		ProjectLocation::from_manifest(&invalid_manifest).unwrap(),
		|| {},
	)
	.is_err());

	let stale_revision = workspace.backend.session().revision();
	rename(&mut workspace, "A newer edit");
	let before_stale = snapshot(&workspace);
	let error = workspace
		.execute(EditorCommandRequest {
			expected_revision: stale_revision,
			command: Command::Rename {
				key: "a".into(),
				name: "Rejected stale edit".into(),
			},
		})
		.unwrap_err();
	assert!(error.contains("Revision conflict"), "{error}");
	assert_eq!(snapshot(&workspace), before_stale);

	rename(&mut workspace, "Saved scene name");
	configure(&mut workspace, "Saved project name");
	assert!(
		workspace
			.save(workspace.backend.session().revision())
			.unwrap()
			.dirty
	);
	assert_eq!(
		runtime::project::ProjectData::load(&manifest).unwrap().name,
		"Selected project"
	);
	let disk_scene = runtime::scene_io::load_data(&workspace.scene_path)
		.unwrap()
		.data;
	assert_eq!(disk_scene.entities[0].name, "Saved scene name");
	assert!(
		!workspace
			.save_project(workspace.backend.session().revision())
			.unwrap()
			.dirty
	);
	assert_eq!(
		runtime::project::ProjectData::load(&manifest).unwrap().name,
		"Saved project name"
	);

	rename(&mut workspace, "Unsaved scene survives");
	configure(&mut workspace, "Unsaved settings survive");
	let held = hide_manifest_and_reject_reload(&mut workspace);
	run(&mut workspace, Command::Undo);
	assert_eq!(
		workspace.backend.session().project().name,
		"Saved project name"
	);
	assert_eq!(
		workspace.backend.session().authored_document().entities[0].name,
		"Unsaved scene survives"
	);
	run(&mut workspace, Command::Undo);
	assert_eq!(
		workspace.backend.session().authored_document().entities[0].name,
		"Saved scene name"
	);
	assert!(!workspace.backend.session().is_dirty());
	run(&mut workspace, Command::Redo);
	run(&mut workspace, Command::Redo);
	assert_eq!(
		workspace.backend.session().project().name,
		"Unsaved settings survive"
	);
	assert_eq!(
		workspace.backend.session().authored_document().entities[0].name,
		"Unsaved scene survives"
	);
	assert!(workspace.backend.session().is_dirty());
	fs::rename(held, &manifest).unwrap();
	let restored = workspace
		.reload_with_policy(
			workspace.backend.session().revision(),
			ReloadPolicy::DiscardUnsaved,
		)
		.unwrap();
	assert_eq!(restored.document.entities[0].name, "Saved scene name");
	assert_eq!(restored.project.name, "Saved project name");
	assert!(!restored.dirty);
	assert!(!restored.can_undo);
	assert!(!restored.can_redo);

	let standalone_fixture = Fixture::new();
	let mut standalone = standalone_fixture.open();
	assert!(!standalone.project_path.exists());
	rename(&mut standalone, "Temporary standalone edit");
	let reloaded = standalone
		.reload_with_policy(
			standalone.backend.session().revision(),
			ReloadPolicy::DiscardUnsaved,
		)
		.unwrap();
	assert_eq!(reloaded.document.entities[0].name, "Original");
	assert!(!reloaded.dirty);
	assert!(!reloaded.can_undo);
	assert!(!standalone.project_path.exists());
	assert!(
		!standalone
			.save_project(standalone.backend.session().revision())
			.unwrap()
			.dirty
	);
	assert!(standalone.project_path.is_file());
	rename(&mut standalone, "Keep promoted project edit");
	let held = hide_manifest_and_reject_reload(&mut standalone);
	fs::rename(held, &standalone.project_path).unwrap();
	assert_eq!(
		standalone.backend.session().authored_document().entities[0].name,
		"Keep promoted project edit"
	);
	assert!(standalone.backend.snapshot().unwrap().can_undo);
}

fn incremental(workspace: &mut EditorWorkspace, command: Command) -> EditorCommandUpdateDto {
	workspace
		.execute_incremental(EditorCommandRequest {
			expected_revision: workspace.backend.session().revision(),
			command,
		})
		.unwrap()
}

// Reconstruct exactly what the frontend receives and compare every field with
// the authoritative snapshot, including history, dirty flags and selection.
fn apply_incremental(
	previous: EditorSnapshotDto,
	update: &EditorCommandUpdateDto,
) -> EditorSnapshotDto {
	match update {
		EditorCommandUpdateDto::Snapshot { snapshot, .. } => snapshot.clone(),
		EditorCommandUpdateDto::Delta {
			state,
			entities,
			camera,
			lights,
			..
		} => {
			let mut document = previous.document;
			for changed in entities {
				let target = document
					.entities
					.iter_mut()
					.find(|e| e.key == changed.key)
					.unwrap();
				*target = changed.clone();
			}
			if let Some(camera) = camera {
				document.camera = camera.clone();
			}
			if let Some(lights) = lights {
				document.lights = lights.clone();
			}
			let mut json = serde_json::to_value(state).unwrap();
			json["document"] = serde_json::to_value(document).unwrap();
			json["project"] = serde_json::to_value(previous.project).unwrap();
			serde_json::from_value(json).unwrap()
		}
	}
}

#[test]
fn incremental_commands_match_snapshots_through_edit_history_and_play() {
	let fixture = Fixture::new();
	let mut workspace = fixture.open();
	let mut client = workspace.backend.snapshot().unwrap();
	let mut camera = client.document.camera.clone();
	camera.position[0] += 2.0;
	let commands = vec![
		Command::Select {
			keys: vec!["a".into()],
		},
		Command::Rename {
			key: "a".into(),
			name: "Renamed".into(),
		},
		Command::TranslateSelection {
			delta: [1.0, 0.0, 0.0],
			snap: false,
		},
		Command::SetCamera { camera },
		Command::UpsertLight {
			light: runtime::scene_data::LightData {
				key: EntityKey::new("sun").unwrap(),
				name: "Sun".into(),
				kind: runtime::scene_data::LightKind::Directional,
				position: [0.0; 3],
				direction: [0.0, -1.0, 0.0],
				color: [1.0; 3],
				intensity: 1.0,
				range: 10.0,
			},
		},
		Command::RemoveLight { key: "sun".into() },
		Command::BeginTransaction {
			label: "Drag".into(),
		},
		Command::TranslateSelection {
			delta: [2.0, 0.0, 0.0],
			snap: false,
		},
		Command::CancelTransaction,
		Command::Undo,
		Command::Redo,
		Command::DuplicateSelection {
			offset: [1.0, 0.0, 0.0],
		},
		Command::DeleteSelection,
		Command::Undo,
		Command::StartPlay,
		Command::StopPlay,
	];
	for command in commands {
		let update = incremental(&mut workspace, command);
		// The actual JSON contract must survive transport as well as the Rust enum.
		let wire = serde_json::to_vec(&update).unwrap();
		let update = serde_json::from_slice(&wire).unwrap();
		client = apply_incremental(client, &update);
		assert_eq!(
			serde_json::to_value(&client).unwrap(),
			serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap()
		);
	}
}

#[test]
fn incremental_selection_and_rename_payloads_do_not_scale_with_scene_size() {
	let fixture = Fixture::new();
	let mut workspace = fixture.open();
	let mut data = (*workspace.backend.session().authored_document()).clone();
	for index in 0..1000 {
		let mut entity = data.entities[0].clone();
		entity.key = EntityKey::new(format!("extra-{index}")).unwrap();
		data.entities.push(entity);
	}
	let project = workspace.backend.session().project().clone();
	workspace.backend = runtime::Engine::new(data, project).unwrap();
	let full_bytes = serde_json::to_vec(&workspace.backend.snapshot().unwrap())
		.unwrap()
		.len();
	for command in [
		Command::Select {
			keys: vec!["a".into()],
		},
		Command::Rename {
			key: "a".into(),
			name: "Small update".into(),
		},
		Command::Rename {
			key: "a".into(),
			name: "Small update".into(),
		},
	] {
		let update = incremental(&mut workspace, command);
		let EditorCommandUpdateDto::Delta { entities, .. } = &update else {
			panic!("expected compact update")
		};
		assert!(entities.len() <= 1);
		let bytes = serde_json::to_vec(&update).unwrap().len();
		assert!(bytes < 4096, "compact update was {bytes} bytes");
		assert!(bytes * 20 < full_bytes);
	}
}

#[test]
fn incremental_structural_and_project_changes_resync_and_conflicts_preserve_state() {
	let fixture = Fixture::new();
	let mut workspace = fixture.open();
	incremental(
		&mut workspace,
		Command::Select {
			keys: vec!["a".into()],
		},
	);
	let update = incremental(
		&mut workspace,
		Command::DuplicateSelection {
			offset: [1.0, 0.0, 0.0],
		},
	);
	assert!(matches!(update, EditorCommandUpdateDto::Snapshot { .. }));
	let mut project = workspace.backend.session().project().clone();
	project.name = "New name".into();
	assert!(matches!(
		incremental(&mut workspace, Command::SetProject { project }),
		EditorCommandUpdateDto::Snapshot { .. }
	));
	let before = serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap();
	let error = workspace
		.execute_incremental(EditorCommandRequest {
			expected_revision: 0,
			command: Command::DeleteSelection,
		})
		.unwrap_err();
	assert_eq!(error.code, runtime::BackendErrorCode::Conflict);
	assert_eq!(
		before,
		serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap()
	);
}
