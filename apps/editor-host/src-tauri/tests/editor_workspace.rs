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
		workspace.backend.session().project().gameplay_module.as_deref(),
		Some(gameplay_catalog::CINDER_OATH_ID)
	);
	run(&mut workspace, Command::StartPlay);
	assert_eq!(
		workspace.backend.session().play_scene_id().map(SceneId::as_str),
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
	assert_eq!(editor::scene_warming::publish_warming_report(&snapshot, &report).unwrap(), 0);
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
#[path = "editor_workspace/play.rs"]
mod play;

fn campaign_workspace() -> (
	Fixture,
	PathBuf,
	EditorWorkspace,
	courtyard_gameplay::settings::CampaignDefinition,
) {
	let fixture = Fixture::new();
	let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../game");
	let manifest = fixture.0.parent().unwrap().join("project.json");
	fs::copy(source.join("project.courtyard-chapters.json"), &manifest).unwrap();
	let project = runtime::project::ProjectData::load(&manifest).unwrap();
	for scene in project.scenes.iter() {
		fs::copy(
			source.join("assets").join(scene.asset_path()),
			fixture.0.join(scene.asset_path()),
		)
		.unwrap();
	}
	fs::create_dir(fixture.0.join("props")).unwrap();
	for model in ["capsule.glb", "cube.glb", "sphere.glb"] {
		fs::copy(
			source.join("assets/props").join(model),
			fixture.0.join("props").join(model),
		)
		.unwrap();
	}
	let campaign = courtyard_gameplay::settings::InteractionSettings::from_project(&project)
		.unwrap()
		.unwrap()
		.campaign
		.unwrap();
	let workspace = EditorWorkspace::open_project_with_notifier(
		ProjectLocation::from_manifest(&manifest).unwrap(),
		|| {},
	)
	.unwrap();
	(fixture, manifest, workspace, campaign)
}

#[test]
fn campaign_authoring_rechecks_owner_and_validates_unsaved_inactive_scene_roles() {
	let (_fixture, manifest, mut workspace, mut route) = campaign_workspace();
	let original_project = workspace.backend.session().project().clone();
	route.chapters[1].label = "Garden Walk".into();
	let revision = workspace.backend.session().revision();
	let generation = workspace.campaign_generation(revision).unwrap();
	let reviewed = workspace
		.capture_campaign(generation, revision, route.clone())
		.unwrap()
		.review()
		.unwrap();
	let generation_error = workspace
		.capture_campaign(generation + 1, revision, route.clone())
		.unwrap_err();
	assert_eq!(generation_error.code, "conflict");

	let garden = SceneId::new("garden").unwrap();
	let courtyard = SceneId::new("courtyard").unwrap();
	run(
		&mut workspace,
		Command::SwitchScene {
			scene_id: garden.clone(),
		},
	);
	run(
		&mut workspace,
		Command::SetCollider {
			key: "garden-exit-trigger".into(),
			collider: None,
		},
	);
	run(
		&mut workspace,
		Command::SwitchScene {
			scene_id: courtyard,
		},
	);
	let current_revision = workspace.backend.session().revision();
	assert!(workspace
		.backend
		.session()
		.scene_states()
		.iter()
		.any(|scene| scene.id == garden && scene.dirty));
	let stale_error = workspace.commit_campaign(reviewed).unwrap_err();
	assert_eq!(stale_error.code, "conflict");
	let error = workspace
		.capture_campaign(generation, current_revision, route)
		.unwrap()
		.review()
		.err()
		.expect("An unsaved inactive Garden must fail review");
	assert_eq!(error.scene_id.as_ref(), Some(&garden));
	assert!(error.message.contains("Exit trigger"), "{}", error.message);
	let session = workspace.backend.session();
	assert_eq!(session.revision(), current_revision);
	assert_eq!(session.project(), &original_project);
	assert_eq!(
		runtime::project::ProjectData::load(&manifest).unwrap(),
		original_project
	);
}

#[test]
fn campaign_authoring_applies_one_undoable_route_and_roundtrips_through_staging() {
	let (fixture, manifest, mut workspace, mut route) = campaign_workspace();
	let mut legacy = workspace.backend.session().project().clone();
	let payload = legacy
		.gameplay_settings
		.as_mut()
		.unwrap()
		.as_object_mut()
		.unwrap();
	payload.remove("campaign");
	payload.insert("schema_version".into(), 1.into());
	payload.get_mut("scenes").unwrap()["courtyard"]["destination_scene"] = "finish".into();
	run(
		&mut workspace,
		Command::SetProject {
			project: legacy.clone(),
		},
	);
	let revision = workspace.backend.session().revision();
	let generation = workspace.campaign_generation(revision).unwrap();
	route.chapters[1].label = "Garden Walk".into();
	route.chapters.swap(0, 1);
	let reviewed = workspace
		.capture_campaign(generation, revision, route.clone())
		.unwrap()
		.review()
		.unwrap();
	let accepted = workspace.commit_campaign(reviewed).unwrap();
	assert_eq!(accepted.revision, revision + 1);
	assert!(accepted.project_dirty);
	assert_eq!(
		accepted.project.startup_scene_id,
		route.chapters[0].course_scene
	);
	let before = legacy.gameplay_settings.unwrap();
	let after = accepted.project.gameplay_settings.as_ref().unwrap();
	for key in ["project_save_id", "checkpoints", "route_gate"] {
		assert_eq!(before[key], after[key], "{key}");
	}
	assert_eq!(after["scenes"]["garden"]["destination_scene"], "courtyard");
	assert_eq!(after["scenes"]["courtyard"]["destination_scene"], "finish");

	run(&mut workspace, Command::Undo);
	assert_eq!(
		workspace
			.backend
			.session()
			.project()
			.gameplay_settings
			.as_ref(),
		Some(&before)
	);
	run(&mut workspace, Command::Redo);
	let revision = workspace.backend.session().revision();
	workspace.save_all(revision).unwrap();
	let mut reopened = EditorWorkspace::open_project_with_notifier(
		ProjectLocation::from_manifest(&manifest).unwrap(),
		|| {},
	)
	.unwrap();
	let disk = runtime::project::ProjectData::load(&manifest).unwrap();
	assert_eq!(reopened.backend.session().project(), &disk);
	assert_eq!(&disk, &accepted.project);
	let parent = fixture.0.parent().unwrap().join("stage");
	fs::create_dir(&parent).unwrap();
	let prepared = compiler::prepare_project(&disk, &fixture.0).unwrap();
	let staged = compiler::stage_prepared_project(&prepared, parent.to_str().unwrap()).unwrap();
	assert_eq!(
		runtime::project::ProjectData::load(staged.path().join("project.json")).unwrap(),
		disk
	);
	run(&mut reopened, Command::StartPlay);
	assert_eq!(
		reopened.backend.play_status().unwrap().scene_id,
		route.chapters[0].course_scene
	);
}

#[test]
fn campaign_repair_is_one_guarded_scene_edit_and_stages_after_history_roundtrip() {
	use courtyard_gameplay::authoring_repairs::CampaignRepairRole;
	use runtime::scene_data::ColliderData;

	let (fixture, manifest, mut workspace, route) = campaign_workspace();
	let garden = SceneId::new("garden").unwrap();
	let key = "garden-exit-trigger";
	let collider = |workspace: &EditorWorkspace| {
		workspace
			.backend
			.session()
			.scene_document(&garden)
			.unwrap()
			.entities
			.iter()
			.find(|entity| entity.key.as_str() == key)
			.unwrap()
			.collider
			.clone()
			.unwrap()
	};
	let diagnose = |workspace: &EditorWorkspace| {
		let revision = workspace.backend.session().revision();
		workspace
			.capture_campaign_check(workspace.generation, revision, route.clone())
			.unwrap()
			.diagnose()
	};
	run(
		&mut workspace,
		Command::SwitchScene {
			scene_id: garden.clone(),
		},
	);
	let original = collider(&workspace);
	let mut broken = original.clone();
	match &mut broken {
		ColliderData::Box { interaction, .. } | ColliderData::Sphere { interaction, .. } => {
			interaction.sensor = false;
		}
	}
	run(
		&mut workspace,
		Command::SetCollider {
			key: key.into(),
			collider: Some(broken.clone()),
		},
	);
	let before = workspace.backend.snapshot().unwrap();
	let report = diagnose(&workspace);
	assert_eq!(report.issues.len(), 1);
	let target = report.issues[0].repair.as_ref().unwrap();
	assert_eq!(target.role, CampaignRepairRole::ExitTrigger);
	assert_eq!(target.entity_key.as_str(), key);
	let preview = workspace
		.preview_campaign_repair(workspace.generation, before.revision, target)
		.unwrap();
	assert_eq!(preview.before, broken);
	assert_eq!(preview.after, original);
	assert_eq!(
		serde_json::to_value(&before).unwrap(),
		serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap()
	);

	let generation = workspace.generation;
	assert!(workspace
		.apply_campaign_repair(generation, before.revision, target, &original)
		.is_err());
	run(
		&mut workspace,
		Command::Rename {
			key: key.into(),
			name: "Garden exit".into(),
		},
	);
	let current = workspace.backend.session().revision();
	assert!(workspace
		.apply_campaign_repair(generation, before.revision, target, &preview.before)
		.is_err());
	assert_eq!(workspace.backend.session().revision(), current);
	assert_eq!(collider(&workspace), broken);
	let fresh = workspace
		.preview_campaign_repair(generation, current, target)
		.unwrap();
	let repaired = workspace
		.apply_campaign_repair(generation, current, target, &fresh.before)
		.unwrap();
	assert_eq!(repaired.revision, current + 1);
	assert_eq!(repaired.project, before.project);
	assert!(diagnose(&workspace).issues.is_empty());

	run(&mut workspace, Command::Undo);
	assert_eq!(collider(&workspace), broken);
	assert!(diagnose(&workspace).issues[0].repair.is_some());
	run(&mut workspace, Command::Redo);
	assert_eq!(collider(&workspace), original);
	assert!(diagnose(&workspace).issues.is_empty());
	workspace
		.save_all(workspace.backend.session().revision())
		.unwrap();

	let reopened = EditorWorkspace::open_project_with_notifier(
		ProjectLocation::from_manifest(&manifest).unwrap(),
		|| {},
	)
	.unwrap();
	assert_eq!(collider(&reopened), original);
	let project = runtime::project::ProjectData::load(&manifest).unwrap();
	let prepared = compiler::prepare_project(&project, &fixture.0).unwrap();
	let parent = fixture.0.parent().unwrap().join("repair-stage");
	fs::create_dir(&parent).unwrap();
	let staged = compiler::stage_prepared_project(&prepared, parent.to_str().unwrap()).unwrap();
	let result =
		runtime::scene_io::load_data(&staged.path().join("assets/scenes/chapter-garden.json"))
			.unwrap()
			.data;
	assert_eq!(
		result
			.entities
			.iter()
			.find(|entity| entity.key.as_str() == key)
			.unwrap()
			.collider,
		Some(original)
	);
}

#[test]
fn campaign_repair_batch_updates_two_colliders_in_one_reviewed_history_entry() {
	use courtyard_gameplay::authoring_repairs::{CampaignRepairExpectation, CampaignRepairRole};
	use runtime::scene_data::ColliderData;

	let (fixture, manifest, mut workspace, route) = campaign_workspace();
	let garden = SceneId::new("garden").unwrap();
	run(
		&mut workspace,
		Command::SwitchScene {
			scene_id: garden.clone(),
		},
	);
	let keys = ["garden-key-trigger", "garden-exit-trigger"];
	let colliders = |workspace: &EditorWorkspace| {
		let document = workspace.backend.session().scene_document(&garden).unwrap();
		keys.map(|key| {
			document
				.entities
				.iter()
				.find(|entity| entity.key.as_str() == key)
				.unwrap()
				.collider
				.clone()
				.unwrap()
		})
	};
	let with_settings = |mut collider: ColliderData, sensor, mask: Option<u32>| {
		match &mut collider {
			ColliderData::Box { interaction, .. } | ColliderData::Sphere { interaction, .. } => {
				interaction.sensor = sensor;
				if let Some(mask) = mask {
					interaction.mask = mask;
				}
			}
		}
		collider
	};
	let diagnose = |workspace: &EditorWorkspace| {
		let revision = workspace.backend.session().revision();
		workspace
			.capture_campaign_check(workspace.generation, revision, route.clone())
			.unwrap()
			.diagnose()
	};
	let original = colliders(&workspace);
	let broken = original
		.clone()
		.map(|collider| with_settings(collider, false, None));
	for (key, collider) in keys.iter().zip(broken.iter()) {
		run(
			&mut workspace,
			Command::SetCollider {
				key: (*key).into(),
				collider: Some(collider.clone()),
			},
		);
	}
	run(
		&mut workspace,
		Command::SetCollider {
			key: keys[0].into(),
			collider: Some(with_settings(broken[0].clone(), false, Some(0))),
		},
	);
	let filtered = diagnose(&workspace);
	assert!(filtered.issues[0].repair_batch.is_empty() && filtered.issues[0].repair.is_none());
	run(&mut workspace, Command::Undo);

	let before = workspace.backend.snapshot().unwrap();
	let report = diagnose(&workspace);
	let targets = &report.issues[0].repair_batch;
	assert_eq!(
		targets.iter().map(|target| target.role).collect::<Vec<_>>(),
		[
			CampaignRepairRole::KeyTrigger,
			CampaignRepairRole::ExitTrigger
		]
	);
	let generation = workspace.generation;
	let repeat = [targets[0].clone(), targets[0].clone()];
	assert!(workspace
		.preview_campaign_repair_batch(generation, before.revision, &garden, &repeat)
		.is_err());
	let reviewed = workspace
		.preview_campaign_repair_batch(generation, before.revision, &garden, targets)
		.unwrap();
	assert_eq!(
		reviewed
			.repairs
			.iter()
			.map(|row| row.before.clone())
			.collect::<Vec<_>>(),
		broken
	);
	assert_eq!(
		reviewed
			.repairs
			.iter()
			.map(|row| row.after.clone())
			.collect::<Vec<_>>(),
		original
	);
	assert_eq!(
		serde_json::to_value(&before).unwrap(),
		serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap()
	);
	let mut expected = reviewed
		.repairs
		.into_iter()
		.map(|row| CampaignRepairExpectation {
			target: row.target,
			before: row.before,
		})
		.collect::<Vec<_>>();
	expected[1].before = original[1].clone();
	assert!(workspace
		.apply_campaign_repair_batch(generation, before.revision, &garden, &expected)
		.is_err());
	assert_eq!(colliders(&workspace), broken);
	assert_eq!(workspace.backend.session().revision(), before.revision);
	expected[1].before = broken[1].clone();

	run(
		&mut workspace,
		Command::Rename {
			key: keys[0].into(),
			name: "Garden key".into(),
		},
	);
	let revision = workspace.backend.session().revision();
	assert!(workspace
		.apply_campaign_repair_batch(generation, before.revision, &garden, &expected)
		.is_err());
	assert_eq!(colliders(&workspace), broken);
	let fresh = workspace
		.preview_campaign_repair_batch(generation, revision, &garden, targets)
		.unwrap();
	assert_eq!(fresh.revision, revision);
	let after = workspace
		.apply_campaign_repair_batch(generation, revision, &garden, &expected)
		.unwrap();
	assert_eq!(after.revision, revision + 1);
	assert_eq!(after.project, before.project);
	assert_eq!(colliders(&workspace), original);
	assert!(diagnose(&workspace).issues.is_empty());
	run(&mut workspace, Command::Undo);
	assert_eq!(colliders(&workspace), broken);
	assert_eq!(diagnose(&workspace).issues[0].repair_batch.len(), 2);
	run(&mut workspace, Command::Redo);
	assert_eq!(colliders(&workspace), original);
	workspace
		.save_all(workspace.backend.session().revision())
		.unwrap();
	let reopened = EditorWorkspace::open_project_with_notifier(
		ProjectLocation::from_manifest(&manifest).unwrap(),
		|| {},
	)
		.unwrap();
	assert_eq!(colliders(&reopened), original);
	let project = runtime::project::ProjectData::load(&manifest).unwrap();
	let staged = compiler::stage_prepared_project(
		&compiler::prepare_project(&project, &fixture.0).unwrap(),
		fixture.0.parent().unwrap().to_str().unwrap(),
	)
		.unwrap();
	assert!(staged
		.path()
		.join("assets/scenes/chapter-garden.json")
		.is_file());
}

#[test]
fn campaign_repairs_in_two_scene_visits_keep_separate_undo_and_stage() {
	use runtime::scene_data::ColliderData;

	let (fixture, manifest, mut workspace, route) = campaign_workspace();
	let scenes = [
		(SceneId::new("courtyard").unwrap(), "courtyard-exit-trigger"),
		(SceneId::new("garden").unwrap(), "garden-exit-trigger"),
	];
	let diagnose = |workspace: &EditorWorkspace| {
		workspace
			.capture_campaign_check(
				workspace.generation,
				workspace.backend.session().revision(),
				route.clone(),
			)
			.unwrap()
			.diagnose()
	};
	for (scene, key) in &scenes {
		if workspace.backend.snapshot().unwrap().active_scene_id != *scene {
			run(
				&mut workspace,
				Command::SwitchScene {
					scene_id: scene.clone(),
				},
			);
		}
		let mut collider = workspace
			.backend
			.session()
			.scene_document(scene)
			.unwrap()
			.entities
			.iter()
			.find(|entity| entity.key.as_str() == *key)
			.unwrap()
			.collider
			.clone()
			.unwrap();
		match &mut collider {
			ColliderData::Box { interaction, .. } | ColliderData::Sphere { interaction, .. } => {
				interaction.sensor = false;
			}
		}
		run(
			&mut workspace,
			Command::SetCollider {
				key: (*key).into(),
				collider: Some(collider),
			},
		);
	}

	let generation = workspace.generation;
	assert_eq!(diagnose(&workspace).issues.len(), 2);
	for (scene, _) in &scenes {
		run(
			&mut workspace,
			Command::SwitchScene {
				scene_id: scene.clone(),
			},
		);
		let revision = workspace.backend.session().revision();
		let report = diagnose(&workspace);
		let target = report
			.issues
			.iter()
			.find(|issue| issue.scene_id.as_ref() == Some(scene))
			.and_then(|issue| issue.repair.as_ref())
			.unwrap();
		let preview = workspace
			.preview_campaign_repair(generation, revision, target)
			.unwrap();
		assert_eq!(
			workspace
				.apply_campaign_repair(generation, revision, target, &preview.before)
				.unwrap()
				.revision,
			revision + 1
		);
	}
	assert!(diagnose(&workspace).issues.is_empty());
	run(&mut workspace, Command::Undo);
	let report = diagnose(&workspace);
	assert_eq!(report.issues.len(), 1);
	assert_eq!(report.issues[0].scene_id.as_ref(), Some(&scenes[1].0));
	run(&mut workspace, Command::Redo);
	assert!(diagnose(&workspace).issues.is_empty());
	workspace
		.save_all(workspace.backend.session().revision())
		.unwrap();

	let project = runtime::project::ProjectData::load(&manifest).unwrap();
	let staged = compiler::stage_prepared_project(
		&compiler::prepare_project(&project, &fixture.0).unwrap(),
		fixture.0.parent().unwrap().to_str().unwrap(),
	)
		.unwrap();
	for (scene_id, key) in &scenes {
		let scene = project
			.scenes
			.iter()
			.find(|scene| scene.id() == scene_id)
			.unwrap();
		let document =
			runtime::scene_io::load_data(&staged.path().join("assets").join(scene.asset_path()))
				.unwrap()
				.data;
		let collider = document
			.entities
			.iter()
			.find(|entity| entity.key.as_str() == *key)
			.and_then(|entity| entity.collider.as_ref())
			.unwrap();
		match collider {
			ColliderData::Box { interaction, .. } | ColliderData::Sphere { interaction, .. } => {
				assert!(interaction.sensor);
			}
		}
	}
}

#[test]
fn campaign_diagnostics_read_unsaved_scenes_in_draft_order_without_an_edit() {
	use runtime::scene_data::ColliderData;
	let (fixture, manifest, mut workspace, route) = campaign_workspace();
	let garden = SceneId::new("garden").unwrap();
	let courtyard = SceneId::new("courtyard").unwrap();
	let edit_trigger =
		|workspace: &mut EditorWorkspace, scene: &SceneId, key: &str, enabled: bool| {
			let doc = workspace.backend.session().scene_document(scene).unwrap();
			let mut collider = doc
				.entities
				.iter()
				.find(|entity| entity.key.as_str() == key)
				.unwrap()
				.collider
				.clone()
				.unwrap();
			match &mut collider {
				ColliderData::Box { interaction, .. }
				| ColliderData::Sphere { interaction, .. } => {
					interaction.sensor = enabled;
				}
			}
			run(
				workspace,
				Command::SetCollider {
					key: key.into(),
					collider: Some(collider),
				},
			);
		};
	let select = |workspace: &mut EditorWorkspace, id: &SceneId| {
		run(
			workspace,
			Command::SwitchScene {
				scene_id: id.clone(),
			},
		);
	};
	let run_check = |workspace: &EditorWorkspace| {
		let revision = workspace.backend.session().revision();
		let generation = workspace.campaign_generation(revision).unwrap();
		workspace
			.capture_campaign_check(generation, revision, route.clone())
			.unwrap()
			.diagnose()
	};

	edit_trigger(&mut workspace, &courtyard, "courtyard-key-trigger", false);
	select(&mut workspace, &garden);
	edit_trigger(&mut workspace, &garden, "garden-exit-trigger", false);
	select(&mut workspace, &courtyard);
	let before = workspace.backend.snapshot().unwrap();
	let report = workspace
		.accept_campaign_diagnostics(run_check(&workspace))
		.unwrap();
	assert_eq!(report.revision, before.revision);
	assert_eq!(report.issues.len(), 2);
	assert_eq!(report.issues[0].scene_id.as_ref(), Some(&courtyard));
	assert!(report.issues[0].message.contains("Key trigger"));
	assert!(report.issues[0].repair.is_some());
	assert_eq!(report.issues[1].scene_id.as_ref(), Some(&garden));
	assert!(report.issues[1].repair.is_none());
	assert_eq!(
		report.issues[1].message,
		"Exit trigger needs an enabled sensor collider"
	);
	assert_eq!(
		serde_json::to_value(&before).unwrap(),
		serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap()
	);

	let reopened = EditorWorkspace::open_project_with_notifier(
		ProjectLocation::from_manifest(&manifest).unwrap(),
		|| (),
	)
	.unwrap();
	assert_eq!(
		reopened
			.accept_campaign_diagnostics(run_check(&workspace))
			.unwrap_err()
			.code,
		"conflict"
	);

	select(&mut workspace, &garden);
	edit_trigger(&mut workspace, &garden, "garden-exit-trigger", true);
	assert_eq!(
		workspace
			.accept_campaign_diagnostics(report)
			.unwrap_err()
			.code,
		"conflict"
	);
	let only_courtyard = run_check(&workspace);
	assert_eq!(only_courtyard.issues.len(), 1);
	assert_eq!(only_courtyard.issues[0].scene_id.as_ref(), Some(&courtyard));
	select(&mut workspace, &courtyard);
	edit_trigger(&mut workspace, &courtyard, "courtyard-key-trigger", true);
	assert!(workspace
		.accept_campaign_diagnostics(run_check(&workspace))
		.unwrap()
		.issues
		.is_empty());

	let revision = workspace.backend.session().revision();
	let generation = workspace.campaign_generation(revision).unwrap();
	let mut incomplete = route.clone();
	incomplete.chapters.pop();
	let project_issue = workspace
		.capture_campaign_check(generation, revision, incomplete)
		.unwrap()
		.diagnose();
	assert_eq!(project_issue.issues.len(), 1);
	assert!(project_issue.issues[0].scene_id.is_none());

	let mut repeated = route;
	repeated.chapters[1].course_scene = courtyard.clone();
	let scene_issue = workspace
		.capture_campaign_check(generation, revision, repeated)
		.unwrap()
		.diagnose();
	assert_eq!(scene_issue.issues.len(), 1);
	assert_eq!(scene_issue.issues[0].scene_id.as_ref(), Some(&courtyard));
	assert!(scene_issue.issues[0]
		.message
		.contains("Choose this Course only once"));

	workspace.save_all(revision).unwrap();
	let project = runtime::project::ProjectData::load(&manifest).unwrap();
	let destination = fixture.0.parent().unwrap().join("repaired-stage");
	fs::create_dir(&destination).unwrap();
	let stage = compiler::prepare_project(&project, &fixture.0).unwrap();
	assert!(compiler::stage_prepared_project(&stage, destination.to_str().unwrap()).is_ok());
}
