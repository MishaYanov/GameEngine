mod support;

use engine_core::{
	Camera, Key, SceneData,
	camera_input::CameraIntent,
	character_controller::{CharacterControllerComponent, CharacterControllerSettings},
	components::ComponentId,
	game_camera::{CameraTargetSnapshot, OrbitCameraSettings, OrbitStepInput, resolve_orbit},
	game_session::GameSession,
	input::{GameInputBinding, InputProfile},
	project::{BehaviorSpace, BehaviorSpec, ProjectData},
	scene_catalog::{SceneDescriptor, SceneId},
	scene_data::{ColliderData, EntityData, TransformData},
	simulation_session::{
		ActionWriter, ConfiguredBehaviorFactory, GameModule, GameModuleFactory, InputFrame,
		SimulationAction, SimulationSession,
	},
	world::RuntimeWorld,
};
use std::sync::Arc;
use support::{close, entity, key, scene};

const STEP: f64 = 1.0 / 60.0;

fn settings(target: &str) -> OrbitCameraSettings {
	OrbitCameraSettings {
		target: key(target),
		pivot_offset: [0.0, 0.7, 0.0],
		initial_yaw: 0.0,
		initial_pitch: 0.0,
		initial_distance: 5.0,
		min_distance: 1.0,
		max_distance: 10.0,
		min_pitch: -1.0,
		max_pitch: 1.0,
		follow_seconds: 0.0,
		recovery_seconds: 0.0,
		teleport_distance: 10.0,
		probe_radius: 0.25,
		obstruction_mask: 1,
	}
}

fn authored(entities: Vec<EntityData>, target: &str) -> SceneData {
	let mut data = scene(entities);
	data.camera.orbit = Some(settings(target));
	data
}

fn project(data: &SceneData) -> ProjectData {
	ProjectData::for_scene("Orbit runtime", "scenes/main.json", data).unwrap()
}

fn game(
	data: Arc<SceneData>,
	project: ProjectData,
	factory: Arc<dyn GameModuleFactory>,
) -> GameSession {
	let startup = project.startup_scene_id.clone();
	GameSession::with_factory_for_scene(Arc::new(project), startup, data, factory).unwrap()
}

fn simple_game(data: SceneData) -> GameSession {
	let project = project(&data);
	game(Arc::new(data), project, Arc::new(ConfiguredBehaviorFactory))
}

fn movement_game(mut data: SceneData, speed: f32, response: f32, teleport: f32) -> GameSession {
	let orbit = data.camera.orbit.as_mut().unwrap();
	orbit.follow_seconds = response;
	orbit.teleport_distance = teleport;
	let mut project = project(&data);
	project.add_behavior(BehaviorSpec::ControlledMove {
		entity: key("actor"),
		speed,
		space: BehaviorSpace::World,
	});
	game(Arc::new(data), project, Arc::new(ConfiguredBehaviorFactory))
}

fn submit(game: &mut GameSession, input: CameraIntent) {
	let owner = game.camera_input_owner().unwrap();
	game.submit_camera_input(&owner.run_id, owner.scene_generation, input)
		.unwrap();
}

fn camera_pose(camera: &Camera) -> ([f32; 3], f32, f32) {
	(camera.position, camera.yaw, camera.pitch)
}

fn world_position(session: &SimulationSession, target: &str) -> [f32; 3] {
	let index = session.world().entity_index(&key(target)).unwrap();
	session.world().world_matrices()[index]
		.w_axis
		.truncate()
		.to_array()
}

fn assert_vec_close(actual: [f32; 3], expected: [f32; 3]) {
	for (a, b) in actual.into_iter().zip(expected) {
		close(a, b);
	}
}

#[test]
fn parented_target_follows_resolved_world_matrix_without_rotating_its_world_offset() {
	let parent = entity("parent", [2.0, 0.0, 0.0]).with_transform(TransformData {
		position: [2.0, 0.0, 0.0],
		rotation: [0.0, std::f32::consts::FRAC_PI_2, 0.0],
		scale: [2.0; 3],
	});
	let actor = entity("actor", [1.0, 0.0, 0.0]).with_parent(Some(key("parent")));
	let data = authored(vec![parent, actor], "actor");
	let mut project = project(&data);
	project.add_behavior(BehaviorSpec::Spin {
		entity: key("parent"),
		angular_velocity: [0.0, 1.0, 0.0],
	});
	let mut run = game(
		Arc::new(data.clone()),
		project,
		Arc::new(ConfiguredBehaviorFactory),
	);
	assert_vec_close(
		run.active().orbit_state().unwrap().filtered_pivot,
		[2.0, 0.7, -2.0],
	);
	assert_vec_close(run.active().camera().position, [2.0, 0.7, 3.0]);
	let outcome = run.tick(STEP).unwrap();
	assert_eq!(outcome.steps, 1);
	assert!(outcome.changes.camera_changed);
	let position = world_position(run.active(), "actor");
	let pivot = [position[0], position[1] + 0.7, position[2]];
	assert_vec_close(run.active().orbit_state().unwrap().filtered_pivot, pivot);
	assert_vec_close(
		run.active().camera().position,
		[pivot[0], pivot[1], pivot[2] + 5.0],
	);
	assert_eq!(run.active().camera().yaw, 0.0);
	assert_eq!(run.active().camera().pitch, 0.0);
	assert_eq!(run.active().orbit_state().unwrap().tick, outcome.tick);
	assert_eq!(run.active().document().camera, data.camera);
}

fn box_at(name: &str, position: [f32; 3], half: [f32; 3]) -> EntityData {
	entity(name, position).with_collider(Some(ColliderData::Box {
		half_extents: half,
		offset: [0.0; 3],
		interaction: Default::default(),
	}))
}

#[test]
fn camera_follows_actual_wall_resolved_character_motion_and_the_same_jump_step() {
	let mut player = box_at("player", [0.0, 0.901, 0.0], [0.3, 0.9, 0.3]);
	player.character_controller = Some(CharacterControllerComponent {
		id: ComponentId::new("controller").unwrap(),
		enabled: true,
		settings: CharacterControllerSettings::default(),
	});
	let data = authored(
		vec![
			player,
			box_at("floor", [0.0, -0.5, 0.0], [20.0, 0.5, 20.0]),
			box_at("wall", [2.0, 1.5, 0.0], [0.25, 1.5, 2.0]),
		],
		"player",
	);
	let mut project = project(&data);
	project.input_profile = Some(InputProfile {
		actions: vec![GameInputBinding {
			id: "jump".into(),
			key: Key::Space,
			animation_parameter: None,
		}],
		..Default::default()
	});
	let mut run = game(
		Arc::new(data.clone()),
		project,
		Arc::new(ConfiguredBehaviorFactory),
	);
	run.active_mut()
		.set_character_input(&key("player"), [1.0, 0.0], false)
		.unwrap();
	for _ in 0..120 {
		run.tick(STEP).unwrap();
	}
	let blocked = world_position(run.active(), "player");
	assert!(
		(1.40..1.451).contains(&blocked[0]),
		"expected actual wall stop, got {blocked:?}"
	);
	assert!(run.active().character_motion()[0].planar_speed < 0.001);
	assert_vec_close(
		run.active().orbit_state().unwrap().filtered_pivot,
		[blocked[0], blocked[1] + 0.7, blocked[2]],
	);
	assert_vec_close(
		run.active().camera().position,
		[blocked[0], blocked[1] + 0.7, blocked[2] + 5.0],
	);
	run.active_mut()
		.set_character_input(&key("player"), [0.0; 2], true)
		.unwrap();
	let outcome = run.tick(STEP).unwrap();
	let jumped = world_position(run.active(), "player");
	assert!(jumped[1] > blocked[1]);
	let motion = &run.active().character_motion()[0];
	assert_eq!(motion.jumped_at_tick, Some(outcome.tick));
	let state = run.active().orbit_state().unwrap();
	assert_eq!(state.tick, motion.tick);
	assert_vec_close(
		state.filtered_pivot,
		[jumped[0], jumped[1] + 0.7, jumped[2]],
	);
	assert!(outcome.changes.camera_changed);
	assert_eq!(run.active().document().camera, data.camera);
}

#[test]
fn follow_smoothing_alone_wakes_both_schedulers_then_settles_and_pause_preserves_state() {
	let data = authored(vec![entity("actor", [0.0; 3])], "actor");
	let mut run = movement_game(data, 12.0, 0.2, 10.0);
	assert!(!run.needs_tick());
	run.active_mut()
		.set_controlled_input([1.0, 0.0, 0.0])
		.unwrap();
	run.tick(STEP).unwrap();
	run.active_mut().clear_input();
	let target = world_position(run.active(), "actor");
	let first = run.active().orbit_state().unwrap().filtered_pivot[0];
	assert!(first > 0.0 && first < target[0]);
	assert!(run.needs_tick() && run.active().needs_tick());
	let before_pause = run.active().orbit_state().unwrap().clone();
	let pose_before_pause = camera_pose(run.active().camera());
	run.active_mut().set_paused(true);
	assert_eq!(run.tick(3.0).unwrap().steps, 0);
	assert_eq!(run.active().orbit_state().unwrap(), &before_pause);
	assert_eq!(camera_pose(run.active().camera()), pose_before_pause);
	run.active_mut().set_paused(false);
	let mut settling_steps = 0;
	while run.needs_tick() && settling_steps < 600 {
		assert_eq!(run.tick(STEP).unwrap().steps, 1);
		settling_steps += 1;
	}
	assert!(settling_steps > 1 && settling_steps < 600);
	assert!(!run.needs_tick() && !run.active().needs_tick());
	let state = run.active().orbit_state().unwrap();
	assert!(state.pivot_settled && state.arm_settled);
	assert_eq!(
		state.filtered_pivot,
		[target[0], target[1] + 0.7, target[2]]
	);
	assert_eq!(state.tick, run.active().tick_index());
	let settled = state.clone();
	assert_eq!(run.tick(0.2).unwrap().steps, 0);
	assert_eq!(run.active().orbit_state().unwrap(), &settled);
}

#[test]
fn target_jump_past_teleport_threshold_snaps_even_with_slow_follow() {
	let data = authored(vec![entity("actor", [0.0; 3])], "actor");
	let mut run = movement_game(data, 120.0, 5.0, 1.0);
	run.active_mut()
		.set_controlled_input([1.0, 0.0, 0.0])
		.unwrap();
	let outcome = run.tick(STEP).unwrap();
	run.active_mut().clear_input();
	let position = world_position(run.active(), "actor");
	assert!(position[0] >= 2.0);
	let state = run.active().orbit_state().unwrap();
	assert_eq!(state.filtered_pivot, [position[0], 0.7, 0.0]);
	assert_eq!(state.previous_target_pivot, state.filtered_pivot);
	assert!(state.pivot_settled && state.arm_settled);
	assert_eq!(state.tick, outcome.tick);
	assert_vec_close(run.active().camera().position, [position[0], 0.7, 5.0]);
	assert!(!run.needs_tick());
}

struct RejectAfterMove;
impl GameModuleFactory for RejectAfterMove {
	fn create(&self, _: &ProjectData, _: &SceneId) -> Result<Box<dyn GameModule>, String> {
		Ok(Box::new(Self))
	}
}
impl GameModule for RejectAfterMove {
	fn needs_tick(&self, _: &InputFrame) -> bool {
		true
	}
	fn plan(
		&self,
		world: &RuntimeWorld,
		_: &InputFrame,
		_: f32,
		out: &mut ActionWriter<'_>,
	) -> Result<(), String> {
		let index = world.entity_index(&key("actor")).unwrap();
		if world.data().entities[index].transform.position[0] > 0.5 {
			return Err("reject second orbit step".into());
		}
		out.push(SimulationAction::Move {
			entity: key("actor"),
			world_delta: [1.0, 0.0, 0.0],
		})
	}
}

#[test]
fn second_step_failure_restores_the_full_rig_not_only_its_visible_pose() {
	let mut data = authored(vec![entity("actor", [0.0; 3])], "actor");
	data.camera.orbit.as_mut().unwrap().follow_seconds = 0.3;
	let mut run = game(
		Arc::new(data.clone()),
		project(&data),
		Arc::new(RejectAfterMove),
	);
	let state_before = run.active().orbit_state().unwrap().clone();
	let pose_before = camera_pose(run.active().camera());
	submit(
		&mut run,
		CameraIntent {
			yaw_delta: 0.4,
			pitch_delta: -0.2,
			log_distance_delta: 0.4,
		},
	);
	assert!(
		run.tick(2.0 * STEP)
			.unwrap_err()
			.contains("second orbit step")
	);
	assert!(run.active().paused());
	assert_eq!(run.active().tick_index(), 0);
	assert_eq!(run.active().document().as_ref(), &data);
	assert_eq!(run.active().orbit_state().unwrap(), &state_before);
	assert_eq!(camera_pose(run.active().camera()), pose_before);
	assert_eq!(run.active().pending_camera_input(), CameraIntent::default());
	run.active_mut().set_paused(false);
	run.tick(STEP).unwrap();
	let state = run.active().orbit_state().unwrap();
	assert_eq!(state.yaw, 0.0);
	assert_eq!(state.pitch, 0.0);
	assert_eq!(state.user_distance, 5.0);
	assert!(state.filtered_pivot[0] > 0.0 && state.filtered_pivot[0] < 1.0);
}

#[test]
fn public_resolver_freezes_on_loss_or_reused_key_without_rebinding() {
	let data = authored(vec![entity("actor", [0.0; 3])], "actor");
	let run = simple_game(data.clone());
	let previous = run.active().orbit_state().unwrap().clone();
	let template = run.active().camera();
	let settings = data.camera.orbit.as_ref().unwrap();
	let mut replacement = previous.target.clone();
	replacement.generation += 1;
	let mut foreign = previous.target.clone();
	foreign.world_id = format!("other-{}", foreign.world_id);
	// Authored targets cannot be despawned through SimulationAction, and there is no runtime
	// retarget command. Exercise the public resolver's lifetime contract without such a bypass.
	for handle in [None, Some(replacement), Some(foreign)] {
		let target = handle.map(|handle| CameraTargetSnapshot {
			handle,
			world_position: [100.0; 3],
		});
		let rejected_target = resolve_orbit(
			settings,
			&previous,
			template,
			OrbitStepInput {
				target: target.as_ref(),
				intent: CameraIntent {
					yaw_delta: 0.5,
					..Default::default()
				},
				step_seconds: STEP as f32,
				tick: 1,
				snap: false,
			},
			|_, _, _| panic!("a lost target must not query obstruction"),
		)
			.unwrap();
		assert!(rejected_target.state.target_lost && rejected_target.clear_input);
		assert!(rejected_target.state.pivot_settled && rejected_target.state.arm_settled);
		assert_eq!(rejected_target.state.target, previous.target);
		assert_eq!(rejected_target.state.tick, 1);
		assert_eq!(camera_pose(&rejected_target.pose), camera_pose(template));
		assert_eq!(
			rejected_target.state.filtered_pivot,
			previous.filtered_pivot
		);
		let original = CameraTargetSnapshot {
			handle: previous.target.clone(),
			world_position: [50.0; 3],
		};
		let still_lost = resolve_orbit(
			settings,
			&rejected_target.state,
			&rejected_target.pose,
			OrbitStepInput {
				target: Some(&original),
				intent: CameraIntent::default(),
				step_seconds: STEP as f32,
				tick: 2,
				snap: false,
			},
			|_, _, _| panic!("target loss is sticky until a fresh binding"),
		)
			.unwrap();
		assert!(still_lost.state.target_lost && still_lost.clear_input);
		assert_eq!(camera_pose(&still_lost.pose), camera_pose(template));
	}
}

#[test]
fn rejected_destination_camera_preserves_active_run_and_pending_input() {
	let data = Arc::new(authored(vec![entity("actor", [0.0; 3])], "actor"));
	let second = SceneId::new("second").unwrap();
	let mut project = project(&data);
	project
		.scenes
		.insert(SceneDescriptor::new(second.clone(), "scenes/second.json").unwrap())
		.unwrap();
	let mut run = game(data.clone(), project, Arc::new(ConfiguredBehaviorFactory));
	submit(
		&mut run,
		CameraIntent {
			yaw_delta: 0.3,
			..Default::default()
		},
	);
	run.tick(STEP).unwrap();
	let before = run.active().orbit_state().unwrap().clone();
	let before_pose = camera_pose(run.active().camera());
	let owner = run.camera_input_owner().unwrap();
	let scene_before = run.active().scene_id().clone();
	let progress_before = run.progress_revision();
	submit(
		&mut run,
		CameraIntent {
			yaw_delta: 0.2,
			..Default::default()
		},
	);
	let mut invalid = (*data).clone();
	// Finite source coordinates can still make a one-unit view direction unrepresentable.
	invalid.entities[0].transform.position[2] = 1.0e12;
	assert!(
		run.prepare_scene(second.clone(), Arc::new(invalid))
			.is_err()
	);
	assert_eq!(run.camera_input_owner(), Some(owner));
	assert_eq!(run.active().scene_id(), &scene_before);
	assert_eq!(run.progress_revision(), progress_before);
	assert_eq!(run.active().orbit_state().unwrap(), &before);
	assert_eq!(camera_pose(run.active().camera()), before_pose);
	assert_eq!(run.active().pending_camera_input().yaw_delta, 0.2);
	assert!(!run.active().paused());
	run.tick(STEP).unwrap();
	close(run.active().camera().yaw, 0.5);
	let prepared = run.prepare_scene(second.clone(), data.clone()).unwrap();
	run.commit_scene(prepared).unwrap();
	assert_eq!(run.active().scene_id(), &second);
	assert_eq!(run.active().orbit_state().unwrap().yaw, 0.0);
	assert_eq!(run.active().orbit_state().unwrap().tick, 0);
	assert_eq!(run.active().pending_camera_input(), CameraIntent::default());
}

#[test]
fn restart_rebinds_and_restores_authored_rig_instead_of_saving_live_orbit() {
	let data = Arc::new(authored(vec![entity("actor", [3.0, 0.0, -2.0])], "actor"));
	let mut run = game(
		data.clone(),
		project(&data),
		Arc::new(ConfiguredBehaviorFactory),
	);
	let initial_pose = camera_pose(run.active().camera());
	let old = run.camera_input_owner().unwrap();
	let target_before = run.active().orbit_state().unwrap().target.clone();
	submit(
		&mut run,
		CameraIntent {
			yaw_delta: 0.7,
			pitch_delta: -0.3,
			log_distance_delta: 0.5,
		},
	);
	run.tick(STEP).unwrap();
	assert_ne!(camera_pose(run.active().camera()), initial_pose);
	submit(
		&mut run,
		CameraIntent {
			yaw_delta: 0.2,
			..Default::default()
		},
	);
	run.active_mut().set_paused(true);
	let prepared = run
		.prepare_new_game(run.active().scene_id().clone(), data.clone())
		.unwrap();
	run.commit_scene(prepared).unwrap();
	assert_ne!(run.run_id(), old.run_id);
	assert!(!run.active().paused());
	let state = run.active().orbit_state().unwrap();
	assert_ne!(state.target.world_id, target_before.world_id);
	assert_eq!(state.target.key, target_before.key);
	assert_eq!(
		(state.yaw, state.pitch, state.user_distance, state.tick),
		(0.0, 0.0, 5.0, 0)
	);
	assert!(!state.target_lost && state.pivot_settled && state.arm_settled);
	assert_eq!(camera_pose(run.active().camera()), initial_pose);
	assert_eq!(run.active().pending_camera_input(), CameraIntent::default());
	assert_eq!(run.active().document(), data);
	assert!(
		run.submit_camera_input(
			&old.run_id,
			old.scene_generation,
			CameraIntent {
				yaw_delta: 0.1,
				..Default::default()
			}
		)
			.is_err()
	);
}
