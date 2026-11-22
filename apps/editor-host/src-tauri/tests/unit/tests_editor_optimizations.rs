use super::*;

struct Fixture(PathBuf);
impl Fixture {
	fn new() -> Self {
		static NEXT: AtomicU64 = AtomicU64::new(0);
		let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
			.join("../../../target")
			.join(format!(
				"qa-editor-optimization-{}-{}",
				std::process::id(),
				NEXT.fetch_add(1, Ordering::Relaxed)
			));
		fs::create_dir(&root).unwrap();
		let assets = root.join("assets");
		fs::create_dir(&assets).unwrap();
		Self(assets.canonicalize().unwrap())
	}
	fn document(&self, models: usize) -> SceneData {
		let camera = runtime::Camera::default();
		let mut entities = Vec::new();
		let vertices: Vec<u8> = [0.0_f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]
			.into_iter()
			.flat_map(f32::to_le_bytes)
			.collect();
		fs::write(self.0.join("vertices.bin"), vertices).unwrap();
		for index in 0..models {
			let path = format!("triangle-{index}.gltf");
			fs::write(self.0.join(&path), r#"{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],"nodes":[{"mesh":0}],"buffers":[{"byteLength":36,"uri":"vertices.bin"}],"bufferViews":[{"buffer":0,"byteLength":36}],"accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]}],"meshes":[{"primitives":[{"attributes":{"POSITION":0}}]}]}"#).unwrap();
			entities.push(EntityData::new(
				EntityKey::new(format!("entity-{index}")).unwrap(),
				"Triangle",
				path,
			));
		}
		SceneData {
			schema_version: runtime::scene_data::CURRENT_SCHEMA_VERSION,
			organization: Default::default(),
			default_game_camera: None,
			bindings: Default::default(),
			entities,
			lights: vec![],
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
		}
	}
}
impl Drop for Fixture {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(self.0.parent().unwrap());
	}
}

fn open_workspace(
	fixture: &Fixture,
	scene: &SceneData,
) -> (EditorWorkspace, std::sync::mpsc::Receiver<()>) {
	fs::write(fixture.0.join("main.json"), scene.to_json().unwrap()).unwrap();
	let (notify, ready) = std::sync::mpsc::channel();
	let workspace =
		EditorWorkspace::open_with_notifier(fixture.0.to_str().unwrap(), "main.json", move || {
			let _ = notify.send(());
		})
		.unwrap();
	(workspace, ready)
}

fn command(workspace: &mut EditorWorkspace, command: Command) {
	workspace
		.execute_receipt(EditorCommandRequest {
			expected_revision: workspace.backend.session().revision(),
			command,
		})
		.unwrap();
}

fn orbit_camera_for_preview(scene: &SceneData) -> runtime::scene_data::CameraData {
	let mut camera = scene.camera.clone();
	camera.orbit = Some(runtime::scene_data::OrbitCameraSettings {
		target: scene.entities[0].key.clone(),
		pivot_offset: [0.0; 3],
		initial_yaw: 0.0,
		initial_pitch: 0.0,
		initial_distance: 5.0,
		min_distance: 1.0,
		max_distance: 8.0,
		min_pitch: -1.0,
		max_pitch: 1.0,
		follow_seconds: 0.0,
		recovery_seconds: 0.2,
		teleport_distance: 10.0,
		probe_radius: 0.25,
		obstruction_mask: 1,
	});
	camera
}

#[test]
fn camera_preview_validates_samples_before_publication_and_leaves_the_editor_untouched() {
	use runtime::camera_input::CameraIntent;
	let fixture = Fixture::new();
	let scene = fixture.document(1);
	let (mut workspace, _) = open_workspace(&fixture, &scene);
	command(
		&mut workspace,
		Command::Select {
			keys: vec!["entity-0".into()],
		},
	);
	let session = workspace.backend.session();
	let revision = session.revision();
	let generation = workspace.generation;
	let scene_id = session.active_scene_id().clone();
	let before = serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap();
	let cursor = workspace.backend.change_cursor();
	let camera = orbit_camera_for_preview(&scene);
	let first = workspace
		.open_camera_preview(camera.clone(), revision, generation, scene_id.as_str())
		.unwrap();
	assert_eq!(first.sample, 0);
	assert_eq!(first.effective_distance, 5.0);
	assert!(first.required_probe_radius <= 0.25 && first.blocker.is_none() && !first.target_lost);
	let intent = CameraIntent {
		yaw_delta: 0.4,
		..Default::default()
	};
	let pose = workspace.active_camera_preview().unwrap().position;
	for (token, number, sample) in [
		("wrong", 1, intent),
		(first.token.as_str(), 2, intent),
		(first.token.as_str(), u64::MAX, intent),
		(
			first.token.as_str(),
			1,
			CameraIntent {
				pitch_delta: f64::NAN,
				..intent
			},
		),
	] {
		assert!(workspace
			.sample_camera_preview(token, number, sample)
			.is_err());
		assert_eq!(workspace.active_camera_preview().unwrap().position, pose);
	}
	let reply = workspace
		.sample_camera_preview(&first.token, 1, intent)
		.unwrap();
	assert_eq!(reply.sample, 1);
	assert_ne!(workspace.active_camera_preview().unwrap().position, pose);
	assert!(workspace
		.sample_camera_preview(&first.token, 1, intent)
		.is_err());
	let mut invalid = camera.clone();
	invalid.orbit.as_mut().unwrap().target = EntityKey::new("absent").unwrap();
	assert!(workspace
		.open_camera_preview(invalid, revision, generation, scene_id.as_str())
		.is_err());
	assert_eq!(
		workspace
			.sample_camera_preview(&first.token, 2, CameraIntent::default())
			.unwrap()
			.sample,
		2
	);
	let second = workspace
		.open_camera_preview(camera.clone(), revision, generation, scene_id.as_str())
		.unwrap();
	assert_ne!(first.token, second.token);
	assert_eq!(workspace.active_camera_preview().unwrap().position, pose);
	assert!(!workspace.close_camera_preview(&first.token));
	assert!(workspace.active_camera_preview().is_some());
	assert!(workspace.close_camera_preview(&second.token));
	assert!(workspace.active_camera_preview().is_none());
	assert_eq!(workspace.backend.change_cursor(), cursor);
	assert_eq!(
		serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap(),
		before
	);
	assert!(workspace.backend.session().simulation_stats().is_none());
}

#[test]
fn camera_preview_invalidates_on_workspace_or_scene_changes_and_play() {
	let fixture = Fixture::new();
	let scene = fixture.document(1);
	let (mut workspace, _) = open_workspace(&fixture, &scene);
	let camera = orbit_camera_for_preview(&scene);
	let scene_id = workspace.backend.session().active_scene_id().clone();
	let rev = workspace.backend.session().revision();
	let generation = workspace.generation;
	for (r, g, id) in [
		(rev + 1, generation, scene_id.as_str()),
		(rev, generation + 1, scene_id.as_str()),
		(rev, generation, "another-scene"),
	] {
		assert!(workspace
			.open_camera_preview(camera.clone(), r, g, id)
			.is_err());
	}
	let open = |w: &mut EditorWorkspace| {
		w.open_camera_preview(
			camera.clone(),
			w.backend.session().revision(),
			w.generation,
			scene_id.as_str(),
		)
	};
	let first = open(&mut workspace).unwrap();
	command(
		&mut workspace,
		Command::Rename {
			key: "entity-0".into(),
			name: "Changed".into(),
		},
	);
	assert!(workspace.camera_preview.is_none());
	assert!(workspace
		.sample_camera_preview(&first.token, 1, Default::default())
		.is_err());
	let second = open(&mut workspace).unwrap();
	command(&mut workspace, Command::StartPlay);
	assert!(workspace.camera_preview.is_none());
	assert!(open(&mut workspace).is_err());
	command(&mut workspace, Command::StopPlay);
	assert!(workspace
		.sample_camera_preview(&second.token, 1, Default::default())
		.is_err());
	let third = open(&mut workspace).unwrap();
	workspace
		.reload_with_policy(
			workspace.backend.session().revision(),
			ReloadPolicy::DiscardUnsaved,
		)
		.unwrap();
	assert_ne!(workspace.generation, generation);
	assert!(workspace
		.sample_camera_preview(&third.token, 1, Default::default())
		.is_err());
}

#[test]
fn camera_preview_repaints_after_a_save_retires_the_idle_preview() {
	let fixture = Fixture::new();
	let scene = fixture.document(1);
	let (mut workspace, _) = open_workspace(&fixture, &scene);
	command(
		&mut workspace,
		Command::Rename {
			key: "entity-0".into(),
			name: "Dirty before preview".into(),
		},
	);
	let camera = orbit_camera_for_preview(&scene);
	let revision = workspace.backend.session().revision();
	let scene_id = workspace.backend.session().active_scene_id().clone();
	let opened = workspace
		.open_camera_preview(camera, revision, workspace.generation, scene_id.as_str())
		.unwrap();
	let (query, repaint) = workspace.with_camera_preview_cleanup(|w| w.backend.snapshot());
	query.unwrap();
	assert!(!repaint);
	let (save, repaint) = workspace.with_camera_preview_cleanup(|w| w.save(revision));
	save.unwrap();
	assert!(repaint);
	assert!(workspace.camera_preview.is_none());
	let (closed, repaint) =
		workspace.with_camera_preview_cleanup(|w| w.close_camera_preview(&opened.token));
	assert!(!closed && !repaint);
}

#[test]
fn camera_preview_releases_frozen_document_on_every_close() {
	let fixture = Fixture::new();
	let scene = fixture.document(1);
	let (mut workspace, _) = open_workspace(&fixture, &scene);
	let source = workspace.backend.session().authored_document();
	let baseline = Arc::strong_count(&source);
	let camera = orbit_camera_for_preview(&scene);
	let generation = workspace.generation;
	let revision = workspace.backend.session().revision();
	let scene_id = workspace.backend.session().active_scene_id().clone();
	for _ in 0..64 {
		let opened = workspace
			.open_camera_preview(camera.clone(), revision, generation, scene_id.as_str())
			.unwrap();
		assert_eq!(Arc::strong_count(&source), baseline + 1);
		workspace
			.sample_camera_preview(&opened.token, 1, Default::default())
			.unwrap();
		assert!(workspace.close_camera_preview(&opened.token));
		assert_eq!(Arc::strong_count(&source), baseline);
		assert!(workspace.active_camera_preview().is_none());
	}
}

fn drain_until_idle(
	workspace: &mut EditorWorkspace,
	ready: &std::sync::mpsc::Receiver<()>,
) -> Vec<AssetCompletion> {
	let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
	let mut results = Vec::new();
	loop {
		results.extend(workspace.drain_asset_jobs());
		if workspace.jobs.try_stats().unwrap().pending_jobs == 0
			&& workspace.bounds_queue.is_empty()
		{
			assert!(workspace.pending.is_empty());
			return results;
		}
		ready
			.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
			.unwrap();
	}
}

#[test]
fn user_import_keeps_capacity_during_bounds_backlog_and_reload() {
	for reload in [false, true] {
		let fixture = Fixture::new();
		let scene = fixture.document(12);
		let (mut workspace, ready) = open_workspace(&fixture, &scene);
		assert_eq!(
			workspace.jobs.try_stats().unwrap().reserved_bytes,
			128 * 1024 * 1024
		);
		assert_eq!(workspace.pending.len(), 2);
		assert!(!workspace.bounds_queue.is_empty());
		if reload {
			workspace
				.reload_with_policy(
					workspace.backend.session().revision(),
					ReloadPolicy::RequireClean,
				)
				.unwrap();
			// Old-generation reservations survive reload until drained. They must
			// count toward the background limit even though pending was cleared.
			assert!(workspace.pending.is_empty());
			assert_eq!(
				workspace.jobs.try_stats().unwrap().reserved_bytes,
				128 * 1024 * 1024
			);
		}
		let ticket = workspace
			.submit_import(
				"triangle-0.gltf".into(),
				[0.0; 3],
				workspace.backend.session().revision(),
				128 * 1024 * 1024,
			)
			.unwrap();
		assert_eq!(
			workspace.jobs.try_stats().unwrap().reserved_bytes,
			256 * 1024 * 1024
		);
		workspace.refill_bounds();
		assert_eq!(
			workspace.jobs.try_stats().unwrap().reserved_bytes,
			256 * 1024 * 1024
		);
		let results = drain_until_idle(&mut workspace, &ready);
		assert!(results.iter().all(|result| result.accepted));
		assert!(results
			.iter()
			.any(|result| result.job_id == ticket.job_id && result.accepted));
		assert_eq!(workspace.jobs.try_stats().unwrap().reserved_bytes, 0);
		for reference in &workspace.bounds_references {
			assert!(workspace.backend.assets().find(reference).is_some());
		}
	}
}

#[test]
fn scene_switch_cancels_obsolete_bounds_but_preserves_pending_import() {
	let fixture = Fixture::new();
	let scene = fixture.document(12);
	let (mut workspace, ready) = open_workspace(&fixture, &scene);
	let bounds_ids: Vec<_> = workspace.pending.keys().copied().collect();
	let empty_id = SceneId::new("empty").unwrap();
	let mut empty = scene;
	empty.entities.clear();
	command(
		&mut workspace,
		Command::AddScene {
			id: empty_id.clone(),
			asset_path: "empty.json".into(),
			document: empty,
		},
	);
	let ticket = workspace
		.submit_import(
			"triangle-0.gltf".into(),
			[0.0; 3],
			workspace.backend.session().revision(),
			128 * 1024 * 1024,
		)
		.unwrap();
	command(&mut workspace, Command::SwitchScene { scene_id: empty_id });
	assert!(workspace.bounds_references.is_empty());
	assert!(workspace.bounds_queue.is_empty());
	assert!(matches!(
		workspace.pending.get(&ticket.job_id),
		Some(PendingAsset::Import { .. })
	));
	// Cancellation does not release reservations or forget in-flight jobs.
	assert_eq!(workspace.pending.len(), 3);
	assert_eq!(
		workspace.jobs.try_stats().unwrap().reserved_bytes,
		256 * 1024 * 1024
	);
	let results = drain_until_idle(&mut workspace, &ready);
	assert_eq!(results.len(), 3);
	for id in bounds_ids {
		let result = results.iter().find(|result| result.job_id == id).unwrap();
		assert_eq!(result.error_code, Some("cancelled"));
	}
	// Foreground jobs retain the existing stale-revision rejection; switching
	// scenes must not silently cancel them along with background metadata.
	assert_eq!(
		results
			.iter()
			.find(|result| result.job_id == ticket.job_id)
			.unwrap()
			.error_code,
		Some("conflict")
	);
	assert!(workspace.bounds_seen.is_empty());
	assert_eq!(workspace.jobs.try_stats().unwrap().reserved_bytes, 0);
	for index in 0..12 {
		assert!(workspace
			.backend
			.assets()
			.find(&AssetRef::new(format!("triangle-{index}.gltf"), Some(0)).unwrap())
			.is_none());
	}
}

#[test]
fn undo_requeues_cancelled_bounds_only_after_the_reservation_is_drained() {
	let fixture = Fixture::new();
	let scene = fixture.document(12);
	let (mut workspace, ready) = open_workspace(&fixture, &scene);
	let (cancelled_id, reference) = workspace
		.pending
		.iter()
		.find_map(|(id, purpose)| {
			if let PendingAsset::Bounds(reference) = purpose {
				Some((*id, reference.clone()))
			} else {
				None
			}
		})
		.unwrap();
	let key = scene
		.entities
		.iter()
		.find(|entity| entity.model_path == reference.path)
		.unwrap()
		.key
		.as_str()
		.to_owned();
	command(&mut workspace, Command::Select { keys: vec![key] });
	command(&mut workspace, Command::DeleteSelection);
	assert!(!workspace.bounds_references.contains(&reference));
	assert!(workspace.pending.contains_key(&cancelled_id));
	command(&mut workspace, Command::Undo);
	assert!(workspace.bounds_references.contains(&reference));
	assert!(workspace.bounds_seen.contains(&reference));
	assert_eq!(workspace.pending.len(), 2);
	assert_eq!(
		workspace.jobs.try_stats().unwrap().reserved_bytes,
		128 * 1024 * 1024
	);
	let results = drain_until_idle(&mut workspace, &ready);
	assert_eq!(
		results
			.iter()
			.find(|result| result.job_id == cancelled_id)
			.unwrap()
			.error_code,
		Some("cancelled")
	);
	assert!(results
		.iter()
		.filter(|result| result.job_id != cancelled_id)
		.all(|result| result.accepted));
	assert!(workspace.backend.assets().find(&reference).is_some());
	assert_eq!(workspace.bounds_seen.len(), 12);
	assert_eq!(workspace.jobs.try_stats().unwrap().reserved_bytes, 0);
}

#[test]
fn bounded_metadata_queue_refills_without_repeated_scene_scans() {
	let fixture = Fixture::new();
	let mut scene = fixture.document(12);
	// Many entities can share one model; it still needs only one bounds job.
	for index in 12..32 {
		scene.entities.push(EntityData::new(
			EntityKey::new(format!("entity-{index}")).unwrap(),
			"Shared",
			"triangle-0.gltf",
		));
	}
	fs::write(fixture.0.join("main.json"), scene.to_json().unwrap()).unwrap();
	let (notify, ready) = std::sync::mpsc::channel();
	let mut workspace =
		EditorWorkspace::open_with_notifier(fixture.0.to_str().unwrap(), "main.json", move || {
			let _ = notify.send(());
		})
		.unwrap();
	assert_eq!(workspace.bounds_references.len(), 12);
	assert!(!workspace.bounds_queue.is_empty());
	let scans = workspace.bounds_reference_scans;
	let mut completed = 0;
	let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
	while completed < 12 {
		let results = workspace.drain_asset_jobs();
		assert!(
			results.iter().all(|result| result.accepted),
			"bounds imports must succeed"
		);
		completed += results.len();
		if completed < 12 {
			ready
				.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
				.unwrap();
		}
	}
	assert!(workspace.bounds_queue.is_empty());
	assert_eq!(workspace.bounds_seen.len(), 12);
	assert_eq!(workspace.bounds_reference_scans, scans);
	for _ in 0..4 {
		assert!(workspace.drain_asset_jobs().is_empty());
	}
	assert_eq!(workspace.bounds_reference_scans, scans);
	for reference in &workspace.bounds_references {
		assert!(workspace.backend.assets().find(reference).is_some());
	}
	// Reload invalidates the metadata registry and builds a fresh queue once.
	workspace
		.reload_with_policy(
			workspace.backend.session().revision(),
			ReloadPolicy::RequireClean,
		)
		.unwrap();
	assert_eq!(workspace.bounds_reference_scans, scans + 1);
	assert!(!workspace.bounds_queue.is_empty());
}

#[test]
fn catalog_loader_reuses_startup_and_still_validates_other_documents() {
	let fixture = Fixture::new();
	let startup = fixture.document(0);
	fs::write(
		fixture.0.join("main.json"),
		"already parsed; must not read again",
	)
	.unwrap();
	let mut project = ProjectData::for_scene("Reuse", "main.json", &startup).unwrap();
	let other = SceneId::new("other").unwrap();
	project
		.scenes
		.insert(runtime::scene_catalog::SceneDescriptor::new(other.clone(), "other.json").unwrap())
		.unwrap();
	fs::write(fixture.0.join("other.json"), startup.to_json().unwrap()).unwrap();
	let documents = load_project_documents(&fixture.0, &project, &startup).unwrap();
	assert_eq!(documents, vec![(other, startup.clone())]);
	fs::write(fixture.0.join("other.json"), "invalid inactive scene").unwrap();
	assert!(load_project_documents(&fixture.0, &project, &startup).is_err());
	let mut invalid_startup = startup;
	invalid_startup.camera.far_plane = -1.0;
	assert!(load_project_documents(
		&fixture.0,
		&ProjectData::for_scene("Reuse", "main.json", &fixture.document(0)).unwrap(),
		&invalid_startup
	)
	.is_err());
}

#[test]
fn structural_edits_prune_queued_references_and_refill_new_assets() {
	let fixture = Fixture::new();
	let scene = fixture.document(12);
	fs::write(fixture.0.join("main.json"), scene.to_json().unwrap()).unwrap();
	let (notify, ready) = std::sync::mpsc::channel();
	let mut workspace =
		EditorWorkspace::open_with_notifier(fixture.0.to_str().unwrap(), "main.json", move || {
			let _ = notify.send(());
		})
		.unwrap();
	let removed = workspace.bounds_queue.front().unwrap().clone();
	let key = scene
		.entities
		.iter()
		.find(|entity| entity.model_path == removed.path)
		.unwrap()
		.key
		.as_str()
		.to_owned();
	for command in [
		Command::Select { keys: vec![key] },
		Command::DeleteSelection,
	] {
		workspace
			.execute_receipt(EditorCommandRequest {
				expected_revision: workspace.backend.session().revision(),
				command,
			})
			.unwrap();
	}
	assert!(!workspace.bounds_references.contains(&removed));
	assert!(!workspace.bounds_queue.contains(&removed));
	fs::copy(
		fixture.0.join("triangle-0.gltf"),
		fixture.0.join("added.gltf"),
	)
	.unwrap();
	workspace
		.execute_receipt(EditorCommandRequest {
			expected_revision: workspace.backend.session().revision(),
			command: Command::Spawn {
				entity: bridge::editor::EntitySpecDto {
					asset_id: None,
					name: "Added".into(),
					model_path: "added.gltf".into(),
					mesh_index: Some(0),
					parent: None,
					transform: Default::default(),
				},
			},
		})
		.unwrap();
	let added = AssetRef::new("added.gltf", Some(0)).unwrap();
	assert!(workspace.bounds_references.contains(&added));
	assert_eq!(workspace.bounds_reference_scans, 3);
	let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
	while !workspace.pending.is_empty() || !workspace.bounds_queue.is_empty() {
		assert!(workspace
			.drain_asset_jobs()
			.iter()
			.all(|result| result.accepted));
		if !workspace.pending.is_empty() || !workspace.bounds_queue.is_empty() {
			ready
				.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
				.unwrap();
		}
	}
	assert!(workspace.backend.assets().find(&removed).is_none());
	assert!(workspace.backend.assets().find(&added).is_some());
	assert_eq!(workspace.bounds_reference_scans, 3);
	fs::copy(
		fixture.0.join("added.gltf"),
		fixture.0.join("imported.gltf"),
	)
	.unwrap();
	workspace
		.import_model(
			"imported.gltf",
			[0.0; 3],
			workspace.backend.session().revision(),
		)
		.unwrap();
	assert_eq!(workspace.bounds_reference_scans, 4);
	assert_eq!(workspace.bounds_references.len(), 13);
	assert!(workspace.bounds_references.contains(&added));
	assert!(workspace
		.bounds_references
		.contains(&AssetRef::new("imported.gltf", Some(0)).unwrap()));
}
