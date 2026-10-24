// Compile the CPU workspace service directly; Tauri's WebView module stays private.
#[path = "../src/editor.rs"]
mod editor;
use bridge::editor::{EditorCommandDto as Command, EditorCommandRequest};
use editor::EditorWorkspace;
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
}
impl Drop for Fixture {
	fn drop(&mut self) {
		// This fixture exclusively created the project parent and assets child.
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
	let reloaded = w.reload(w.backend.session().revision()).unwrap();
	assert_eq!(reloaded.document.entities[0].name, "Saved");
	assert!(!reloaded.can_undo);
	assert!(!reloaded.dirty);
	let source = w.backend.session().authored_document();
	fs::write(&w.scene_path, "{bad json").unwrap();
	let revision = w.backend.session().revision();
	assert!(w.reload(revision).is_err());
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
	assert!(w.reload(revision).is_err());
	assert_eq!(fs::read(&w.scene_path).unwrap(), original);
	assert!(w.backend.session().has_transaction());
	run(&mut w, Command::CommitTransaction);
	w.scene_path = fixture.0.join("missing/main.json");
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
	project.behaviors = vec![runtime::project::BehaviorSpec::ControlledMove {
		entity: EntityKey::new("a").unwrap(),
		speed: 2.0,
		space: Default::default(),
	}];
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
	w.reload(w.backend.session().revision()).unwrap();
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
	project.behaviors = vec![runtime::project::BehaviorSpec::ControlledMove {
		entity: EntityKey::new("a").unwrap(),
		speed: 3.0,
		space: Default::default(),
	}];
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
	w.reload(w.backend.session().revision()).unwrap();
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
	assert!(w.reload(w.backend.session().revision()).is_err());
	assert_eq!(
		serde_json::to_value(w.backend.snapshot().unwrap()).unwrap(),
		before
	);
	assert_eq!(w.scene_path, path);
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
	w.reload(w.backend.session().revision()).unwrap();
	assert!(w.backend.assets().get(handle).is_none());
	assert!(w.backend.assets().is_empty());
	assert_eq!(w.backend.session().authored_document().entities.len(), 1);
}
