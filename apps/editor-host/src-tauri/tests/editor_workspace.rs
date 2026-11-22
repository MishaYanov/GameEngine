// Compile the CPU workspace service directly; Tauri's WebView module stays private.
#[path = "../src/animation_preview.rs"]
mod animation_preview;
#[path = "../src/camera_preview.rs"]
mod camera_preview;
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
			organization: Default::default(),
			default_game_camera: None,
			bindings: Default::default(),
			entities: vec![EntityData::new(
				EntityKey::new("a").unwrap(),
				"Original",
				"models/triangle.glb",
			)],
			camera: runtime::scene_data::CameraData {
				follow: None,
				orbit: None,
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
fn legacy_scene_opens_without_writing_and_explicit_save_publishes_its_camera_object() {
	let fixture = Fixture::new();
	let path = fixture.0.join("scenes/main.json");
	let mut wire: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
	wire["schema_version"] = 5.into();
	let historical = serde_json::to_vec_pretty(&wire).unwrap();
	fs::write(&path, &historical).unwrap();
	let mut workspace = fixture.open();
	let camera = workspace
		.backend
		.session()
		.authored_document()
		.default_game_camera
		.clone()
		.unwrap();
	assert_eq!(fs::read(&path).unwrap(), historical);
	workspace
		.save(workspace.backend.session().revision())
		.unwrap();
	let published: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
	assert_eq!(
		published["schema_version"],
		runtime::scene_data::CURRENT_SCHEMA_VERSION
	);
	assert!(published.get("camera").is_none());
	assert_eq!(published["default_game_camera"], camera.as_str());
	let reopened = fixture.open();
	assert_eq!(
		reopened
			.backend
			.session()
			.authored_document()
			.default_game_camera
			.as_ref(),
		Some(&camera)
	);
}

#[test]
fn selected_camera_preview_is_scoped_and_can_see_an_unassigned_follow_camera() {
	let fixture = Fixture::new();
	let path = fixture.0.join("scenes/main.json");
	let mut legacy: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
	legacy["schema_version"] = 5.into();
	let migrated = runtime::SceneData::from_json(&legacy.to_string()).unwrap();
	let mut scene: serde_json::Value = serde_json::from_str(&migrated.to_json().unwrap()).unwrap();
	let mut follow_object = scene["entities"][1].clone();
	follow_object["key"] = "inspection-camera".into();
	follow_object["components"][0]["data"]["settings"]["rig"] = serde_json::json!({
		"mode": "follow",
		"settings": {"target": "a", "offset": [1.0, 2.0, 5.0], "look_offset": [0.0, 0.0, 0.0], "smoothing_seconds": 0.2},
	});
	scene["entities"]
		.as_array_mut()
		.unwrap()
		.push(follow_object);
	fs::write(&path, serde_json::to_vec_pretty(&scene).unwrap()).unwrap();
	let mut workspace = fixture.open();
	let fixed = migrated.default_game_camera.unwrap();
	let follow = EntityKey::new("inspection-camera").unwrap();
	let generation = workspace.generation;
	let scene_id = workspace
		.backend
		.session()
		.active_scene_id()
		.as_str()
		.to_string();
	let begin = |workspace: &mut EditorWorkspace, key, generation| {
		workspace.open_selected_camera_preview(
			key,
			workspace.backend.session().revision(),
			generation,
			&scene_id,
		)
	};
	assert!(begin(&mut workspace, follow.clone(), generation).is_err());
	run(
		&mut workspace,
		Command::Select {
			keys: vec![fixed.as_str().into()],
		},
	);
	let default_preview = begin(&mut workspace, fixed.clone(), generation).unwrap();
	assert!(matches!(
		default_preview.mode,
		camera_preview::CameraPreviewMode::Fixed
	));
	assert_eq!(
		workspace.active_camera_preview().unwrap().position,
		[0.0, 0.0, 3.0]
	);
	run(
		&mut workspace,
		Command::Select {
			keys: vec![follow.as_str().into()],
		},
	);
	assert!(workspace.active_camera_preview().is_none());
	assert!(begin(&mut workspace, follow.clone(), generation + 1).is_err());
	let before = workspace.backend.snapshot().unwrap();
	let selected = begin(&mut workspace, follow.clone(), generation).unwrap();
	assert!(matches!(
		selected.mode,
		camera_preview::CameraPreviewMode::Follow
	));
	assert_eq!(selected.camera_key, Some(follow.clone()));
	assert_eq!(selected.sample, 0);
	assert_eq!(
		workspace.active_camera_preview().unwrap().position,
		[1.0, 2.0, 5.0]
	);
	assert!(!workspace.close_camera_preview(&default_preview.token));
	assert!(workspace
		.sample_camera_preview(&selected.token, 1, Default::default())
		.is_err());
	assert!(workspace.close_camera_preview(&selected.token));
	assert!(workspace.active_camera_preview().is_none());
	let after = workspace.backend.snapshot().unwrap();
	assert_eq!(after.document, before.document);
	assert_eq!(after.revision, before.revision);
	assert_eq!(after.document.default_game_camera, Some(fixed));

	run(
		&mut workspace,
		Command::SetDefaultGameCamera { key: follow },
	);
	workspace
		.save_all(workspace.backend.session().revision())
		.unwrap();
	run(&mut workspace, Command::StartPlay);
	let editor_camera = workspace.backend.session().render_camera();
	assert_eq!(editor_camera.position, [1.0, 2.0, 5.0]);
	run(&mut workspace, Command::StopPlay);
	let prepared = workspace
		.prepare_build(workspace.backend.session().revision())
		.unwrap();
	let output = fixture.0.parent().unwrap().join("camera-package");
	fs::create_dir(&output).unwrap();
	let staged = compiler::stage_prepared_project(&prepared, output.to_str().unwrap()).unwrap();
	let manifest =
		runtime::project::ProjectData::load(&staged.path().join("project.json")).unwrap();
	let path = manifest.scene_path(&manifest.startup_scene_id).unwrap();
	let installed = runtime::scene_io::load_data(&staged.path().join("assets").join(path))
		.unwrap()
		.data;
	let game = runtime::simulation_session::SimulationSession::new(
		manifest,
		std::sync::Arc::new(installed),
	)
	.unwrap();
	assert_eq!(
		game.camera().view_projection(1.0).unwrap(),
		editor_camera.view_projection(1.0).unwrap(),
	);
}

#[test]
fn authored_cinder_oath_opens_and_enters_play_through_the_editor_workspace() {
	let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("../../..")
		.join(gameplay_catalog::BUNDLED_PROJECT_DIRECTORY)
		.join("project.json");
	let mut workspace = EditorWorkspace::open_project_with_notifier(
		ProjectLocation::from_manifest(&manifest).unwrap(),
		|| {},
	)
	.unwrap();
	assert_eq!(
		workspace
			.backend
			.session()
			.project()
			.gameplay_module
			.as_deref(),
		Some(gameplay_catalog::CINDER_OATH_ID)
	);
	run(&mut workspace, Command::StartPlay);
	assert_eq!(
		workspace
			.backend
			.session()
			.play_scene_id()
			.map(SceneId::as_str),
		Some("intro")
	);
	assert!(workspace.backend.play_status().unwrap().gameplay.is_some());
}

#[test]
fn scene_warming_rejects_an_old_workspace_with_the_same_revision() {
	use bridge::viewport::SceneWarmingMode::{Models, References};
	let (previous_fixture, _, second) = two_scene_project();
	let (current_fixture, _, _) = two_scene_project();
	let previous = previous_fixture.open();
	let mut current = current_fixture.open();
	let revision = current.backend.session().revision();
	assert_eq!(previous.backend.session().revision(), revision);
	assert_ne!(previous.generation, current.generation);
	let report = current_fixture
		.0
		.parent()
		.unwrap()
		.join("warming-report.json");
	assert!(current
		.capture_warming_report(previous.generation, "test-session".into())
		.unwrap_err()
		.contains("Workspace changed"));
	assert!(!report.exists());
	let snapshot = current
		.capture_warming_report(current.generation, "test-session".into())
		.unwrap();
	assert_eq!(
		editor::scene_warming::publish_warming_report(&snapshot, &report).unwrap(),
		0
	);
	let original = fs::read(&report).unwrap();
	assert_eq!(
		serde_json::from_slice::<bridge::viewport::WarmingSessionReportDto>(&original).unwrap(),
		snapshot
	);
	assert!(editor::scene_warming::publish_warming_report(&snapshot, &report).is_err());
	assert_eq!(fs::read(&report).unwrap(), original);

	assert!(current
		.set_scene_warming_mode(previous.generation, Models)
		.is_err());
	let initial = current
		.set_scene_warming_mode(current.generation, References)
		.unwrap();
	current
		.set_scene_warming_mode(current.generation, Models)
		.unwrap();
	let selected = current
		.set_scene_warming_mode(current.generation, References)
		.unwrap();
	assert!(current
		.warm_scene(
			previous.generation,
			revision,
			selected,
			Some(second.clone())
		)
		.is_err());
	assert!(current
		.warm_scene(current.generation, revision, initial, Some(second.clone()))
		.is_err());
	assert!(!current.scene_warming.is_requested());
	current
		.warm_scene(current.generation, revision, selected, Some(second))
		.unwrap();
	assert!(current
		.warm_scene(previous.generation, revision, selected, None)
		.is_err());
	current
		.warm_scene(current.generation, revision, selected, None)
		.unwrap();
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
			organization,
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
			if let Some(organization) = organization {
				document.organization = organization.clone();
			}
			let mut json = serde_json::to_value(state).unwrap();
			json["document"] = serde_json::to_value(document).unwrap();
			json["project"] = serde_json::to_value(previous.project).unwrap();
			serde_json::from_value(json).unwrap()
		}
	}
}

fn collider_fixture() -> Fixture {
	let fixture = Fixture::new();
	fs::copy(
		PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../game/assets/props/cube.glb"),
		fixture.0.join("models/crate.glb"),
	)
	.unwrap();
	let path = fixture.0.join("scenes/main.json");
	let mut document = runtime::scene_io::load_data(&path).unwrap().data;
	document.entities = vec![EntityData::new(
		EntityKey::new("crate").unwrap(),
		"Collision prop",
		"models/crate.glb",
	)];
	fs::write(path, document.to_json().unwrap()).unwrap();
	fixture.write_project("scenes/main.json");
	fixture
}

fn apply_collider_update(
	workspace: &mut EditorWorkspace,
	client: &mut EditorSnapshotDto,
	command: Command,
) -> EditorCommandUpdateDto {
	let update = incremental(workspace, command);
	let wire = serde_json::to_vec(&update).unwrap();
	let update = serde_json::from_slice(&wire).unwrap();
	*client = apply_incremental(client.clone(), &update);
	assert_eq!(
		serde_json::to_value(&*client).unwrap(),
		serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap(),
	);
	update
}

#[path = "editor_workspace/assets.rs"]
mod assets;
#[path = "editor_workspace/colliders.rs"]
mod colliders;
#[path = "editor_workspace/documents.rs"]
mod documents;
#[path = "editor_workspace/incremental.rs"]
mod incremental;
#[path = "editor_workspace/nested_prop.rs"]
mod nested_prop;
#[path = "editor_workspace/play.rs"]
mod play;
#[path = "editor_workspace/prop_overrides.rs"]
mod prop_overrides;
#[path = "editor_workspace/prop_preview.rs"]
mod prop_preview;
#[path = "editor_workspace/prop_recovery.rs"]
mod prop_recovery;

#[path = "editor_workspace/scene_verification.rs"]
mod scene_verification;

#[path = "editor_workspace/prop_roundtrip.rs"]
mod prop_roundtrip;
