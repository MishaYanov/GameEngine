use super::*;

#[test]
fn game_status_projects_only_on_changes_during_steady_ticks() {
	#[derive(Clone, PartialEq)]
	struct TestProgress(i32);
	impl runtime::game_progress::GameProgress for TestProgress {
		type Action = i32;

		fn validate(&self) -> Result<(), String> {
			Ok(())
		}

		fn retained_bytes(&self) -> usize {
			std::mem::size_of::<Self>()
		}

		fn action_bytes(_: &Self::Action) -> usize {
			std::mem::size_of::<i32>()
		}

		fn apply(&mut self, action: &Self::Action) -> Result<(), String> {
			self.0 += action;
			Ok(())
		}
	}
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
			ProgressState::new(TestProgress(0))
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
				score: progress.get::<TestProgress>().map(|progress| progress.0),
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

	let award = ProgressAction::new::<TestProgress>(2).unwrap();
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
		GameCameraCapture::acquire(&controller, &workspace, true, || Err::<(), _>(
			"OS refused grab".into()
		))
		.is_err()
	);
	let (capture, ()) = GameCameraCapture::acquire(&controller, &workspace, true, || Ok(()))
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
	let (capture, ()) = GameCameraCapture::acquire(&controller, &workspace, true, || Ok(()))
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
fn cinder_captured_primary_click_swings_and_blur_cancels_the_next_press() {
	let fixture = Fixture::new(0);
	let (mut workspace, mut controller, seconds) = cinder_duel(&fixture, None);

	// A raw pointer event has no authority until a current native grab succeeds.
	send(
		&mut controller,
		&mut workspace,
		event("pointer_down", [400.0, 300.0], Some("left")),
	);
	workspace.backend.tick(seconds).unwrap();
	assert_eq!(cinder_progress(&workspace).swings(), 0);
	let (capture, ()) = GameCameraCapture::acquire(&controller, &workspace, true, || Ok(()))
		.unwrap()
		.unwrap();
	capture
		.set_primary_button(&mut controller, &mut workspace, true)
		.unwrap();
	capture
		.set_primary_button(&mut controller, &mut workspace, false)
		.unwrap();
	workspace.backend.tick(seconds).unwrap();
	assert_eq!(cinder_progress(&workspace).swings(), 1);

	for _ in 0..50 {
		workspace.backend.tick(seconds).unwrap();
	}
	capture
		.set_primary_button(&mut controller, &mut workspace, true)
		.unwrap();
	send(
		&mut controller,
		&mut workspace,
		event("blurred", [0.0; 2], None),
	);
	capture
		.set_primary_button(&mut controller, &mut workspace, true)
		.unwrap();
	workspace.backend.tick(seconds).unwrap();
	assert_eq!(cinder_progress(&workspace).swings(), 1);
	assert!(!controller.captured_primary_held);
}

#[test]
fn cinder_defeat_releases_a_held_game_capture_and_prevents_recapture() {
	use gameplay_catalog::cinder_oath::Phase;

	let fixture = Fixture::new(0);
	let (mut workspace, mut controller, seconds) = cinder_duel(&fixture, None);
	let (capture, ()) = GameCameraCapture::acquire(&controller, &workspace, true, || Ok(()))
		.unwrap()
		.unwrap();
	capture
		.set_primary_button(&mut controller, &mut workspace, true)
		.unwrap();

	for _ in 0..1_200 {
		workspace.backend.tick(seconds).unwrap();
		if cinder_progress(&workspace).phase() == Phase::Defeat {
			break;
		}
	}
	assert_eq!(cinder_progress(&workspace).phase(), Phase::Defeat);
	assert!(controller.captured_primary_held);
	assert!(capture.current(&workspace));
	assert!(!capture.accepts_input(&workspace));
	assert!(capture
		.submit(
			&mut workspace,
			CameraIntent {
				yaw_delta: 0.2,
				..Default::default()
			}
		)
		.is_err());

	capture.release(&mut controller, Some(&mut workspace));
	assert!(!controller.captured_primary_held);
	assert!(
		GameCameraCapture::acquire(&controller, &workspace, true, || -> Result<(), String> {
			panic!("defeat attempted another native grab")
		})
			.unwrap()
			.is_none()
	);
}

#[test]
fn an_authored_game_mouse_button_routes_both_edges_without_camera_capture() {
	let fixture = Fixture::new(0);
	let (mut workspace, mut controller, seconds) =
		cinder_duel(&fixture, Some(runtime::MouseButton::Right));

	send(
		&mut controller,
		&mut workspace,
		ViewportInputDto {
			position: None,
			..event("pointer_down", [0.0; 2], Some("right"))
		},
	);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_down", [400.0, 300.0], Some("left")),
	);
	workspace.backend.tick(seconds).unwrap();
	assert_eq!(cinder_progress(&workspace).swings(), 0);

	send(
		&mut controller,
		&mut workspace,
		event("pointer_down", [400.0, 300.0], Some("right")),
	);
	assert!(controller.keys.contains("MouseRight"));
	send(
		&mut controller,
		&mut workspace,
		ViewportInputDto {
			position: None,
			..event("pointer_up", [0.0; 2], Some("right"))
		},
	);
	assert!(!controller.keys.contains("MouseRight"));
	workspace.backend.tick(seconds).unwrap();
	assert_eq!(cinder_progress(&workspace).swings(), 1);

	for _ in 0..50 {
		workspace.backend.tick(seconds).unwrap();
	}
	send(
		&mut controller,
		&mut workspace,
		event("pointer_down", [400.0, 300.0], Some("right")),
	);
	send(
		&mut controller,
		&mut workspace,
		event("blurred", [0.0; 2], None),
	);
	assert!(controller.keys.is_empty());
	workspace.backend.tick(seconds).unwrap();
	assert_eq!(cinder_progress(&workspace).swings(), 1);
}

fn cinder_duel(
	fixture: &Fixture,
	mouse_button: Option<runtime::MouseButton>,
) -> (EditorWorkspace, ViewportInteraction, f64) {
	use gameplay_catalog::cinder_oath::Phase;

	let project =
		compiler::create_authoring_project("cinder-pointer", fixture.directory.to_str().unwrap())
			.unwrap();
	let location =
		crate::project_location::ProjectLocation::from_manifest(&project.join("project.json"))
			.unwrap();
	let mut workspace = EditorWorkspace::open_project_with_notifier(location, || {}).unwrap();
	if let Some(button) = mouse_button {
		let mut project = workspace.backend.session().project().clone();
		project.input_profile.as_mut().unwrap().mouse_actions.push(
			runtime::input::MouseActionBinding {
				id: "attack".into(),
				button,
			},
		);
		command(&mut workspace, Command::SetProject { project });
	}
	let mut controller = controller(&mut workspace);
	controller.settings.view = ViewportView::Game;
	command(&mut workspace, Command::StartPlay);
	focus_keyboard(&mut controller, &mut workspace);
	send(&mut controller, &mut workspace, key("Enter"));

	let seconds = 1.0 / f64::from(workspace.backend.session().project().fixed_hz);
	workspace.backend.tick(seconds).unwrap();
	let (arena, _) = workspace.backend.prepare_pending_play_scene().unwrap();
	workspace.backend.commit_play_scene(arena).unwrap();
	controller.clear_play_input(&mut workspace);
	assert_eq!(cinder_progress(&workspace).phase(), Phase::Duel);
	(workspace, controller, seconds)
}

fn cinder_progress(workspace: &EditorWorkspace) -> gameplay_catalog::cinder_oath::CinderProgress {
	*workspace.backend.session().play_progress().unwrap()
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
