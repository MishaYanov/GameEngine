mod support;

use engine_core::{
	Camera, Engine, SceneData,
	camera_input::{CameraInputOwner, CameraIntent},
	command::{EditorCommandDto as Command, EditorCommandRequest},
	game_camera::OrbitCameraSettings,
	game_session::GameSession,
	project::{BehaviorSpace, BehaviorSpec, ProjectData},
	scene_catalog::{SceneDescriptor, SceneId},
	simulation_session::{
		ActionWriter, ConfiguredBehaviorFactory, GameModule, GameModuleFactory, InputFrame,
		SimulationAction,
	},
	world::RuntimeWorld,
};
use std::sync::Arc;
use support::{close, entity, key, scene};

const STEP: f64 = 1.0 / 60.0;

fn orbit_scene() -> Arc<SceneData> {
	let mut data = scene(vec![
		entity("parent", [2.0, 0.0, 0.0]),
		entity("actor", [1.0, 0.0, 0.0]).with_parent(Some(key("parent"))),
	]);
	data.camera.orbit = Some(OrbitCameraSettings {
		target: key("actor"),
		pivot_offset: [0.0, 1.0, 0.0],
		initial_yaw: 0.0,
		initial_pitch: 0.0,
		initial_distance: 5.0,
		min_distance: 2.0,
		max_distance: 8.0,
		min_pitch: -0.5,
		max_pitch: 0.6,
		follow_seconds: 0.0,
		recovery_seconds: 0.0,
		teleport_distance: 8.0,
		probe_radius: 0.25,
		obstruction_mask: 1,
	});
	Arc::new(data)
}

fn project(data: &SceneData) -> ProjectData {
	ProjectData::for_scene("Game", "scenes/main.json", data).unwrap()
}

fn session(
	data: Arc<SceneData>,
	project: ProjectData,
	factory: Arc<dyn GameModuleFactory>,
) -> GameSession {
	let startup = project.startup_scene_id.clone();
	GameSession::with_factory_for_scene(Arc::new(project), startup, data, factory).unwrap()
}

fn orbit_game() -> GameSession {
	let data = orbit_scene();
	let project = project(&data);
	session(data, project, Arc::new(ConfiguredBehaviorFactory))
}

fn submit(game: &mut GameSession, input: CameraIntent) {
	let owner = game.camera_input_owner().unwrap();
	game.submit_camera_input(&owner.run_id, owner.scene_generation, input)
		.unwrap();
}

fn yaw(yaw_delta: f64) -> CameraIntent {
	CameraIntent {
		yaw_delta,
		..Default::default()
	}
}

fn pose(camera: &Camera) -> ([f32; 3], f32, f32) {
	(camera.position, camera.yaw, camera.pitch)
}

#[test]
fn input_wakes_both_schedulers_and_survives_short_frames() {
	let mut game = orbit_game();
	let initial = pose(game.active().camera());
	assert_eq!(initial, ([3.0, 1.0, 5.0], 0.0, 0.0));
	assert!(!game.needs_tick() && !game.active().needs_tick());
	submit(&mut game, yaw(0.2));
	assert!(game.needs_tick() && game.active().needs_tick());
	assert_eq!(game.tick(STEP * 0.5).unwrap().steps, 0);
	assert_eq!(game.active().pending_camera_input(), yaw(0.2));
	assert_eq!(pose(game.active().camera()), initial);
	let outcome = game.tick(STEP * 0.5).unwrap();
	assert_eq!(outcome.steps, 1);
	assert!(outcome.changes.camera_changed);
	close(game.active().camera().yaw, 0.2);
	assert_eq!(
		game.active().pending_camera_input(),
		CameraIntent::default()
	);
	assert!(!game.needs_tick() && !game.active().needs_tick());
}

#[test]
fn catch_up_applies_rotation_and_log_zoom_once_across_all_admitted_steps() {
	// Spinning the actor leaves its own world-space pivot stationary and admits idle-input steps.
	let create = || {
		let data = orbit_scene();
		let mut project = project(&data);
		project.add_behavior(BehaviorSpec::Spin {
			entity: key("actor"),
			angular_velocity: [0.0, 1.0, 0.0],
		});
		session(data, project, Arc::new(ConfiguredBehaviorFactory))
	};
	let mut batched = create();
	let mut split = create();
	let intent = CameraIntent {
		yaw_delta: 0.2,
		pitch_delta: 0.3,
		log_distance_delta: 1.2_f64.ln(),
	};
	submit(&mut batched, intent);
	submit(&mut split, intent);
	assert_eq!(batched.tick(3.0 * STEP).unwrap().steps, 3);
	assert_eq!(split.tick(STEP).unwrap().steps, 1);
	let remaining = split.tick(2.0 * STEP).unwrap();
	assert_eq!(remaining.steps, 2);
	assert!(!remaining.changes.camera_changed);
	assert_eq!(batched.active().tick_index(), split.active().tick_index());
	let camera = batched.active().camera();
	assert_eq!(pose(camera), pose(split.active().camera()));
	close(camera.yaw, 0.2);
	close(camera.pitch, 0.3);
	let pivot = glam::Vec3::new(3.0, 1.0, 0.0);
	close(glam::Vec3::from_array(camera.position).distance(pivot), 6.0);
	for (pitch, zoom, expected_pitch, expected_distance) in
		[(2.0, 4.0_f64, 0.6, 8.0), (-2.0, 0.25_f64, -0.5, 2.0)]
	{
		submit(
			&mut split,
			CameraIntent {
				yaw_delta: std::f64::consts::PI,
				pitch_delta: pitch,
				log_distance_delta: zoom.ln(),
			},
		);
		split.tick(STEP).unwrap();
		let camera = split.active().camera();
		close(camera.pitch, expected_pitch);
		assert!(camera.yaw.abs() <= std::f32::consts::PI);
		close(
			glam::Vec3::from_array(camera.position).distance(pivot),
			expected_distance,
		);
	}
}

#[test]
fn invalid_samples_reject_atomically_and_fixed_cameras_do_not_claim_input() {
	let mut game = orbit_game();
	let owner = game.camera_input_owner().unwrap();
	submit(&mut game, yaw(0.3));
	for log_distance_delta in [f64::NAN, f64::INFINITY, 4.0_f64.ln() + 0.01] {
		let before = pose(game.active().camera());
		let invalid = CameraIntent {
			log_distance_delta,
			..yaw(0.4)
		};
		assert!(
			game.submit_camera_input(&owner.run_id, owner.scene_generation, invalid)
				.is_err()
		);
		assert_eq!(pose(game.active().camera()), before);
		assert_eq!(game.active().pending_camera_input(), yaw(0.3));
	}
	let data = Arc::new(scene(vec![entity("actor", [0.0; 3])]));
	let mut fixed = session(
		data.clone(),
		project(&data),
		Arc::new(ConfiguredBehaviorFactory),
	);
	let run = fixed.run_id().to_owned();
	assert!(fixed.camera_input_owner().is_none());
	assert!(fixed.submit_camera_input(&run, 0, yaw(0.2)).is_err());
	assert!(!fixed.needs_tick());
}

#[test]
fn shared_clear_and_pause_drop_camera_and_movement_together() {
	let data = orbit_scene();
	let mut project = project(&data);
	project.add_behavior(BehaviorSpec::ControlledMove {
		entity: key("actor"),
		speed: 1.0,
		space: BehaviorSpace::World,
	});
	let mut game = session(data.clone(), project, Arc::new(ConfiguredBehaviorFactory));
	let owner = game.camera_input_owner().unwrap();
	let initial = pose(game.active().camera());
	for pause in [false, true] {
		game.active_mut()
			.set_controlled_input([1.0, 0.0, 0.0])
			.unwrap();
		submit(&mut game, yaw(0.2));
		if pause {
			game.active_mut().set_paused(true);
			assert!(game.camera_input_owner().is_none());
			assert!(
				game.submit_camera_input(&owner.run_id, owner.scene_generation, yaw(0.1))
					.is_err()
			);
			game.active_mut().set_paused(false);
		} else {
			game.active_mut().clear_input();
		}
		assert_eq!(
			game.active().pending_camera_input(),
			CameraIntent::default()
		);
		assert_eq!(game.tick(STEP).unwrap().steps, 0);
		assert_eq!(game.active().document(), data);
		assert_eq!(pose(game.active().camera()), initial);
	}
}

#[test]
fn stale_samples_reject_after_scene_round_trip_and_restart() {
	let data = orbit_scene();
	let mut project = project(&data);
	let first = project.startup_scene_id.clone();
	let second = SceneId::new("second").unwrap();
	project
		.scenes
		.insert(SceneDescriptor::new(second.clone(), "scenes/second.json").unwrap())
		.unwrap();
	let mut game = session(data.clone(), project, Arc::new(ConfiguredBehaviorFactory));
	let old = game.camera_input_owner().unwrap();
	for destination in [second, first.clone()] {
		submit(&mut game, yaw(0.1));
		let next = game.prepare_scene(destination, data.clone()).unwrap();
		game.commit_scene(next).unwrap();
		assert!(!game.owns_camera_input(&old));
		assert_eq!(old.run_id, game.run_id());
		assert_ne!(old.scene_generation, game.scene_generation());
		assert!(
			game.submit_camera_input(&old.run_id, old.scene_generation, yaw(0.4))
				.is_err()
		);
		assert!(!game.needs_tick());
		assert_eq!(game.active().camera().yaw, 0.0);
	}
	let next = game.prepare_new_game(first, data).unwrap();
	game.commit_scene(next).unwrap();
	assert_eq!(game.scene_generation(), old.scene_generation);
	assert_ne!(game.run_id(), old.run_id);
	assert!(
		game.submit_camera_input(&old.run_id, old.scene_generation, yaw(0.4))
			.is_err()
	);
	assert!(!game.needs_tick());
}

struct RejectSecondStep;
impl GameModuleFactory for RejectSecondStep {
	fn create(&self, _: &ProjectData, _: &SceneId) -> Result<Box<dyn GameModule>, String> {
		Ok(Box::new(RejectSecondStep))
	}
}
impl GameModule for RejectSecondStep {
	fn needs_tick(&self, _: &InputFrame) -> bool {
		true
	}
	fn plan(
		&self,
		world: &RuntimeWorld,
		_: &InputFrame,
		_: f32,
		output: &mut ActionWriter<'_>,
	) -> Result<(), String> {
		let actor = key("actor");
		if world.data().entities[world.entity_index(&actor).unwrap()]
			.transform
			.position[0]
			> 1.5
		{
			return Err("planned second-step rejection".into());
		}
		output.push(SimulationAction::Move {
			entity: actor,
			world_delta: [1.0, 0.0, 0.0],
		})
	}
}

#[test]
fn failed_aggregate_rolls_back_an_applied_camera_step_and_clears_live_input() {
	let data = orbit_scene();
	let mut game = session(data.clone(), project(&data), Arc::new(RejectSecondStep));
	let before = pose(game.active().camera());
	submit(
		&mut game,
		CameraIntent {
			log_distance_delta: 0.5,
			..yaw(0.3)
		},
	);
	assert!(
		game.tick(2.0 * STEP)
			.unwrap_err()
			.contains("second-step rejection")
	);
	assert!(game.active().paused());
	assert_eq!(game.active().tick_index(), 0);
	assert_eq!(game.active().document(), data);
	assert_eq!(pose(game.active().camera()), before);
	assert_eq!(
		game.active().pending_camera_input(),
		CameraIntent::default()
	);
	game.active_mut().set_paused(false);
	// The module moves the target one metre on the newly accepted step.
	// Follow that movement while keeping the yaw and zoom from before rejection.
	assert!(game.tick(STEP).unwrap().changes.camera_changed);
	let (mut eye, yaw, pitch) = before;
	eye[0] += 1.0;
	assert_eq!(pose(game.active().camera()), (eye, yaw, pitch));
	assert_eq!(game.active().orbit_state().unwrap().user_distance, 5.0);
}

fn command(owner: &CameraInputOwner, workspace_generation: u64, input: CameraIntent) -> Command {
	Command::SetPlayCameraInput {
		workspace_generation,
		run_id: owner.run_id.clone(),
		scene_generation: owner.scene_generation,
		yaw_delta: input.yaw_delta,
		pitch_delta: input.pitch_delta,
		log_distance_delta: input.log_distance_delta,
	}
}

#[test]
fn consuming_input_at_the_pitch_limit_publishes_status_without_a_new_render_pose() {
	use engine_core::events::BackendEventKind;
	let data = orbit_scene();
	let mut engine = Engine::new((*data).clone(), project(&data)).unwrap();
	engine
		.execute(EditorCommandRequest {
			expected_revision: 0,
			command: Command::StartPlay,
		})
		.unwrap();
	let owner = engine.camera_input_owner().unwrap();
	let input = CameraIntent {
		pitch_delta: 1.0,
		..Default::default()
	};
	engine
		.submit_camera_input(&owner.run_id, owner.scene_generation, input)
		.unwrap();
	assert!(engine.tick(STEP).unwrap().changes.camera_changed);
	let before = pose(&engine.session().render_camera());
	let cursor = engine.change_cursor();
	let scene_revision = engine.session().scene_revision();

	engine
		.submit_camera_input(&owner.run_id, owner.scene_generation, input)
		.unwrap();
	assert!(!engine.tick(STEP * 0.5).unwrap().camera_status_changed);
	assert_eq!(engine.change_cursor(), cursor);
	let outcome = engine.tick(STEP * 0.5).unwrap();
	assert_eq!(outcome.steps, 1);
	assert!(outcome.camera_status_changed);
	assert!(outcome.changes.is_empty());
	assert_eq!(pose(&engine.session().render_camera()), before);
	assert_eq!(engine.session().scene_revision(), scene_revision);
	assert!(
		engine
			.changes_since(cursor)
			.unwrap()
			.events
			.iter()
			.any(|e| matches!(
				e.event,
				BackendEventKind::SimulationAdvanced {
					camera_status_changed: true,
					..
				}
			))
	);
	assert!(!engine.session().needs_tick());
}

#[test]
fn editor_command_is_revision_checked_and_leaves_authored_history_untouched() {
	let data = orbit_scene();
	let mut engine = Engine::new((*data).clone(), project(&data)).unwrap();
	assert!(engine.camera_input_owner().is_none());
	engine
		.execute(EditorCommandRequest {
			expected_revision: 0,
			command: Command::StartPlay,
		})
		.unwrap();
	let before = engine.snapshot().unwrap();
	let owner = engine.camera_input_owner().unwrap();
	assert!(engine.owns_camera_input(&owner));
	let revision = before.revision;
	let request = |expected_revision, command| EditorCommandRequest {
		expected_revision,
		command,
	};
	assert!(
		engine
			.execute(request(revision - 1, command(&owner, 0, yaw(0.3))))
			.is_err()
	);
	assert!(
		engine
			.execute(request(revision, command(&owner, 1_u64 << 53, yaw(0.3))))
			.is_err()
	);
	assert_eq!(engine.session().revision(), revision);
	assert_eq!(
		engine
			.execute(request(
				revision,
				command(&owner, 0, CameraIntent::default())
			))
			.unwrap()
			.revision,
		revision
	);
	let receipt = engine
		.execute(request(revision, command(&owner, 0, yaw(0.1))))
		.unwrap();
	assert_eq!(receipt.revision, revision + 1);
	assert_eq!(receipt.scene_revision, before.scene_revision);
	engine
		.submit_camera_input(&owner.run_id, owner.scene_generation, yaw(0.2))
		.unwrap();
	assert_eq!(engine.session().revision(), receipt.revision);
	assert!(engine.tick(STEP).unwrap().changes.camera_changed);
	let after = engine.snapshot().unwrap();
	assert_eq!(engine.session().authored_document(), data);
	assert_eq!(
		(after.dirty, after.can_undo, after.can_redo),
		(before.dirty, before.can_undo, before.can_redo)
	);
	engine
		.execute(request(after.revision, Command::StopPlay))
		.unwrap();
	assert!(engine.camera_input_owner().is_none());
	assert!(
		engine
			.submit_camera_input(&owner.run_id, owner.scene_generation, yaw(0.7))
			.is_err()
	);
	assert!(!engine.owns_camera_input(&owner));
	assert_eq!(engine.snapshot().unwrap().document, *data);
}
