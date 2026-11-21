use super::*;

#[test]
fn game_status_publishes_progress_only_completion_restart_and_new_play_identity() {
	use std::sync::Arc;
	let fixture = Fixture::new(1);
	let mut workspace = fixture.open();
	let sample = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../game");
	let project = runtime::project::ProjectData::load(sample.join("project.json")).unwrap();
	let mut documents = Vec::new();
	for entry in project.scenes.iter() {
		let mut document =
			runtime::scene_io::load_data(&sample.join("assets").join(entry.asset_path()))
				.unwrap()
				.data;
		// Coincident markers exercise the entire objective route without GPU/input timing.
		if matches!(entry.id().as_str(), "courtyard" | "finish") {
			for entity in &mut document.entities {
				entity.transform.position = [0.0; 3];
			}
		}
		documents.push((entry.id().clone(), document));
	}
	let startup = documents
		.iter()
		.find(|(id, _)| id == &project.startup_scene_id)
		.unwrap()
		.1
		.clone();
	documents.retain(|(id, _)| id != &project.startup_scene_id);
	workspace.backend = runtime::EngineBuilder::new(startup, project)
		.with_scenes(documents)
		.module_resolver(Arc::new(courtyard_gameplay::resolve))
		.build()
		.unwrap();
	let mut controller = controller(&mut workspace);
	assert!(controller.state().game_status.is_none());
	command(&mut workspace, Command::StartPlay);
	controller.sync(&mut workspace).unwrap();
	let first = controller.state().game_status.unwrap();
	assert_eq!(first.gameplay.as_ref().unwrap().score, Some(0));
	let author_revision = workspace.backend.session().revision();
	let before = controller.revision();
	let outcome = workspace.backend.tick(1.0 / 60.0);
	controller.sync_game_status_after_tick(&workspace, &outcome);
	let outcome = outcome.unwrap();
	// Coincident authored sensors collect the key, publish its visual feedback,
	// and request the unlocked exit in the same accepted step.
	assert!(outcome.progress_changed && !outcome.changes.is_empty());
	assert_eq!(outcome.requested_scene.unwrap().as_str(), "finish");
	assert!(controller.state().revision > before);
	assert_eq!(
		controller
			.state()
			.game_status
			.unwrap()
			.gameplay
			.unwrap()
			.score,
		Some(1)
	);
	assert!(!controller.take_document_changed());
	assert_eq!(
		controller
			.state()
			.game_status
			.unwrap()
			.pending_scene_id
			.unwrap()
			.as_str(),
		"finish"
	);
	let (prepared, _) = workspace.backend.prepare_pending_play_scene().unwrap();
	workspace.backend.commit_play_scene(prepared).unwrap();
	controller.sync_game_status(&workspace);
	let transitioned = controller.state().game_status.unwrap();
	assert_eq!(transitioned.scene_id.as_str(), "finish");
	assert!(transitioned.pending_scene_id.is_none());
	let outcome = workspace.backend.tick(1.0 / 60.0);
	controller.sync_game_status_after_tick(&workspace, &outcome);
	let outcome = outcome.unwrap();
	assert!(outcome.progress_changed && outcome.changes.is_empty());
	let completed = controller.state();
	assert!(completed.game_status.unwrap().gameplay.unwrap().completed);
	assert!(!workspace.backend.session().needs_tick());
	assert_eq!(workspace.backend.session().revision(), author_revision);
	assert!(!controller.take_document_changed());
	controller.sync(&mut workspace).unwrap();
	assert_eq!(
		controller.revision(),
		completed.revision,
		"idle status is deduplicated"
	);
	let (prepared, _) = workspace
		.backend
		.prepare_restart_play(&first.run_id, author_revision)
		.unwrap();
	workspace.backend.commit_play_scene(prepared).unwrap();
	controller.sync_game_status(&workspace);
	let restarted = controller.state().game_status.unwrap();
	assert_eq!(restarted.play_session_id, first.play_session_id);
	assert_ne!(restarted.run_id, first.run_id);
	assert_eq!(restarted.scene_id.as_str(), "courtyard");
	assert_eq!(restarted.gameplay.unwrap().score, Some(0));
	assert!(controller.state().revision > completed.revision);
	command(&mut workspace, Command::SetPaused { paused: true });
	controller.sync(&mut workspace).unwrap();
	assert!(controller.state().game_status.unwrap().paused);
	command(&mut workspace, Command::StopPlay);
	command(&mut workspace, Command::StartPlay);
	// Simulates Stop/Start both arriving between worker wakes.
	controller.sync(&mut workspace).unwrap();
	assert_ne!(
		controller.state().game_status.unwrap().play_session_id,
		first.play_session_id
	);
	command(&mut workspace, Command::StopPlay);
	controller.sync(&mut workspace).unwrap();
	assert!(controller.state().game_status.is_none());
}

#[test]
fn game_status_projects_only_on_changes_during_steady_ticks() {
	use courtyard_gameplay::{CampaignProgress, ProgressAction as CampaignAction};
	use runtime::{
		game_progress::{ProgressAction, ProgressState, ProgressView},
		project::ProjectData,
		scene_catalog::SceneId,
		simulation_session::{
			ActionWriter, GameModule, GameModuleFactory, GameplayContext, GameplayStatus,
			InputFrame,
		},
		world::RuntimeWorld,
	};
	use std::sync::Arc;

	#[derive(Clone)]
	struct CountedStatus(Arc<AtomicU64>);
	impl GameModuleFactory for CountedStatus {
		fn create_progress(&self) -> Result<ProgressState, String> {
			ProgressState::new(CampaignProgress::default())
		}
		fn create(&self, _: &ProjectData, _: &SceneId) -> Result<Box<dyn GameModule>, String> {
			Ok(Box::new(self.clone()))
		}
	}
	impl GameModule for CountedStatus {
		fn needs_tick(&self, _: &InputFrame) -> bool {
			true
		}
		fn plan(
			&self,
			_: &RuntimeWorld,
			_: &InputFrame,
			_: f32,
			_: &mut ActionWriter<'_>,
		) -> Result<(), String> {
			Ok(())
		}
		fn plan_with_progress(
			&self,
			context: GameplayContext<'_>,
			_: &mut ActionWriter<'_>,
		) -> Result<(), String> {
			if context.progress.revision() > 0 {
				return Err("Test simulation rejection".into());
			}
			Ok(())
		}
		fn status(&self, progress: ProgressView<'_>) -> Option<GameplayStatus> {
			self.0.fetch_add(1, Ordering::Relaxed);
			Some(GameplayStatus {
				objective: "Test objective".into(),
				detail: None,
				score: progress
					.get::<CampaignProgress>()
					.map(|progress| progress.score),
				completed: false,
			})
		}
	}

	let fixture = Fixture::new(1);
	let mut workspace = fixture.open();
	let session = workspace.backend.session();
	let calls = Arc::new(AtomicU64::new(0));
	let elapsed = 1.0 / f64::from(session.project().fixed_hz);
	workspace.backend = runtime::EngineBuilder::new(
		(*session.authored_document()).clone(),
		session.project().clone(),
	)
	.module_factory(Arc::new(CountedStatus(Arc::clone(&calls))))
	.build()
	.unwrap();
	let mut controller = controller(&mut workspace);
	command(&mut workspace, Command::StartPlay);
	for _ in 0..120 {
		controller.sync(&mut workspace).unwrap();
		let outcome = workspace.backend.tick(elapsed);
		controller.sync_game_status_after_tick(&workspace, &outcome);
		assert_eq!(outcome.unwrap().steps, 1);
		controller.sync(&mut workspace).unwrap();
	}
	assert_eq!(calls.load(Ordering::Relaxed), 1);

	let award =
		ProgressAction::new::<CampaignProgress>(CampaignAction::AwardScore { amount: 2 }).unwrap();
	workspace.backend.apply_play_progress(&[award]).unwrap();
	controller.sync(&mut workspace).unwrap();
	let status = controller.state().game_status.unwrap();
	assert_eq!(status.gameplay.unwrap().score, Some(2));
	assert!(!status.paused);
	assert_eq!(calls.load(Ordering::Relaxed), 2);

	let author_revision = workspace.backend.session().revision();
	let rejected = workspace.backend.tick(elapsed);
	assert!(rejected.is_err());
	controller.sync_game_status_after_tick(&workspace, &rejected);
	assert!(controller.state().game_status.unwrap().paused);
	assert_eq!(workspace.backend.session().revision(), author_revision);
	controller.sync(&mut workspace).unwrap();
	assert_eq!(calls.load(Ordering::Relaxed), 3);
}

#[test]
fn play_mode_locks_authored_transforms_placement_and_delete() {
	let fixture = Fixture::new(1);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	focus_keyboard(&mut controller, &mut workspace);
	select(&mut workspace, &["entity-0"]);
	command(&mut workspace, Command::StartPlay);
	controller.sync(&mut workspace).unwrap();
	let authored = workspace.backend.session().authored_document();
	assert!(controller.gizmo(&workspace, EXTENT, 1.0).unwrap().is_none());
	assert!(controller
		.action(
			ViewportActionDto::BeginPlacement {
				path: "models/triangle.glb".into()
			},
			&mut workspace,
			EXTENT
		)
		.is_err());
	send(&mut controller, &mut workspace, key("Delete"));
	assert_eq!(*workspace.backend.session().authored_document(), *authored);
	assert!(!workspace.backend.session().has_transaction());
	assert!(!workspace.backend.session().is_dirty());
}

#[test]
fn game_camera_capture_requires_focused_orbit_and_expires_across_pause_or_replacement() {
	let fixture = Fixture::new(1);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	let forbidden_grab =
		|| -> Result<(), String> { panic!("ineligible viewport attempted a grab") };
	assert!(
		GameCameraCapture::acquire(&controller, &workspace, true, forbidden_grab)
			.unwrap()
			.is_none()
	);
	focus_keyboard(&mut controller, &mut workspace);
	controller.settings.view = ViewportView::Game;
	command(&mut workspace, Command::StartPlay);
	controller.sync(&mut workspace).unwrap();
	assert!(
		GameCameraCapture::acquire(&controller, &workspace, true, forbidden_grab)
			.unwrap()
			.is_none()
	);
	command(&mut workspace, Command::StopPlay);

	let mut document = (*workspace.backend.session().authored_document()).clone();
	document.camera.orbit = Some(
		serde_json::from_value(serde_json::json!({
			"target": "entity-0", "pivot_offset": [0, 0, 0],
			"initial_yaw": 0, "initial_pitch": 0, "initial_distance": 6,
			"min_pitch": -1, "max_pitch": 1, "min_distance": 1, "max_distance": 12,
			"follow_seconds": 0, "recovery_seconds": 0, "teleport_distance": 10,
			"probe_radius": 1, "obstruction_mask": 1
		}))
		.unwrap(),
	);
	workspace.backend.replace_document(document).unwrap();
	command(&mut workspace, Command::StartPlay);
	controller.sync(&mut workspace).unwrap();
	assert!(
		GameCameraCapture::acquire(&controller, &workspace, false, forbidden_grab)
			.unwrap()
			.is_none()
	);
	controller.settings.view = ViewportView::Scene;
	assert!(
		GameCameraCapture::acquire(&controller, &workspace, true, forbidden_grab)
			.unwrap()
			.is_none()
	);
	controller.settings.view = ViewportView::Game;
	assert!(
		GameCameraCapture::acquire(&controller, &workspace, true, || Err(
			"OS refused grab".into()
		))
		.is_err()
	);
	let capture = GameCameraCapture::acquire(&controller, &workspace, true, || Ok(()))
		.unwrap()
		.unwrap();
	let intent = CameraIntent {
		yaw_delta: 0.2,
		..Default::default()
	};
	assert!(capture.current(&workspace));
	capture.submit(&mut workspace, intent).unwrap();
	assert!(workspace.backend.session().needs_tick());
	command(&mut workspace, Command::SetPaused { paused: true });
	assert!(
		GameCameraCapture::acquire(&controller, &workspace, true, forbidden_grab)
			.unwrap()
			.is_none()
	);
	command(&mut workspace, Command::SetPaused { paused: false });
	assert!(!capture.current(&workspace));
	assert!(capture.submit(&mut workspace, intent).is_err());
	let capture = GameCameraCapture::acquire(&controller, &workspace, true, || Ok(()))
		.unwrap()
		.unwrap();
	let mut replacement = fixture.open();
	assert!(!capture.current(&replacement));
	assert!(capture.submit(&mut replacement, intent).is_err());
	assert!(
		GameCameraCapture::acquire(&controller, &replacement, true, forbidden_grab)
			.unwrap()
			.is_none()
	);
	let request = |generation| EditorCommandRequest {
		expected_revision: workspace.backend.session().revision(),
		command: Command::SetPlayCameraInput {
			workspace_generation: generation,
			run_id: capture.owner.run_id.clone(),
			scene_generation: capture.owner.scene_generation,
			yaw_delta: 0.2,
			pitch_delta: 0.0,
			log_distance_delta: 0.0,
		},
	};
	let wrong = request(replacement.generation);
	let correct = request(workspace.generation);
	assert!(workspace
		.execute_receipt(wrong)
		.unwrap_err()
		.message
		.contains("previous workspace"));
	workspace.execute_receipt(correct).unwrap();
}

#[test]
fn focused_game_routes_held_mapped_movement_until_input_reset_or_blur() {
	use runtime::{
		input::{InputProfile, MovementBindings},
		project::BehaviorSpec,
	};

	let fixture = Fixture::new(1);
	let mut workspace = fixture.open();
	let mut project = workspace.backend.session().project().clone();
	let player = EntityKey::new("entity-0").unwrap();
	project.add_behavior(BehaviorSpec::ControlledMove {
		entity: player.clone(),
		speed: 2.0,
		space: Default::default(),
	});
	project.input_profile = Some(InputProfile {
		movement: MovementBindings::ARROWS,
		..Default::default()
	});
	command(&mut workspace, Command::SetProject { project });
	let mut controller = controller(&mut workspace);
	focus_keyboard(&mut controller, &mut workspace);
	controller
		.action(
			ViewportActionDto::SetView {
				view: ViewportView::Game,
			},
			&mut workspace,
			EXTENT,
		)
		.unwrap();
	assert!(!controller.game_keyboard_active());
	command(&mut workspace, Command::StartPlay);
	controller.sync(&mut workspace).unwrap();
	assert!(controller.game_keyboard_active());
	assert_eq!(
		workspace.backend.session().play_controlled_targets(),
		Some(&[player][..])
	);
	let position = |workspace: &EditorWorkspace| {
		workspace.backend.session().render_document().entities[0]
			.transform
			.position
	};
	let tick = |workspace: &mut EditorWorkspace| {
		workspace.backend.tick(0.1).unwrap();
		position(workspace)
	};
	send(&mut controller, &mut workspace, key("KeyD"));
	assert_eq!(tick(&mut workspace), [0.0; 3]);
	send(&mut controller, &mut workspace, key("ArrowRight"));
	assert!(Vec3::from_array(tick(&mut workspace)).abs_diff_eq(Vec3::X * 0.2, 1.0e-5));
	send(
		&mut controller,
		&mut workspace,
		event("pointer_up", [300.0, 300.0], Some("left")),
	);
	let before_release = tick(&mut workspace);
	assert!(Vec3::from_array(before_release).abs_diff_eq(Vec3::X * 0.4, 1.0e-5));
	let key_up = ViewportInputDto {
		kind: "key_up".into(),
		..key("ArrowRight")
	};
	send(&mut controller, &mut workspace, key_up);
	assert_eq!(tick(&mut workspace), before_release);
	send(&mut controller, &mut workspace, key("ArrowRight"));
	controller.clear_play_input(&mut workspace);
	assert!(controller.keys.is_empty());
	assert!(controller.game_keyboard_active());
	send(
		&mut controller,
		&mut workspace,
		ViewportInputDto {
			repeat: true,
			..key("ArrowRight")
		},
	);
	assert_eq!(tick(&mut workspace), before_release);
	send(&mut controller, &mut workspace, key("ArrowRight"));
	let before_blur = tick(&mut workspace);
	assert!(Vec3::from_array(before_blur).abs_diff_eq(Vec3::X * 0.6, 1.0e-5));
	controller
		.input(
			&event("blurred", [0.0; 2], None),
			&mut workspace,
			[0; 2],
			1.0,
		)
		.unwrap();
	assert!(controller.keys.is_empty());
	assert!(!controller.state().focused);
	assert_eq!(tick(&mut workspace), before_blur);
	send(&mut controller, &mut workspace, key("ArrowRight"));
	assert_eq!(tick(&mut workspace), before_blur);
	command(&mut workspace, Command::StopPlay);
	controller.sync(&mut workspace).unwrap();
	assert!(!controller.game_keyboard_active());
	assert_eq!(position(&workspace), [0.0; 3]);
}
