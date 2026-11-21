mod support;

use engine_core::{
	Camera, SceneData,
	camera_input::CameraIntent,
	components::{ComponentId, NativeBehaviorComponent, NativeBehaviorSpec},
	editor::PrefabData,
	game_camera::{MAX_GAME_ASPECT, OrbitCameraSettings},
	game_session::GameSession,
	project::{BehaviorSpace, BehaviorSpec, ProjectData},
	scene_catalog::{SceneDescriptor, SceneId},
	scene_data::{ColliderData, ColliderInteraction, EntityData},
	simulation_session::{ConfiguredBehaviorFactory, SimulationSession},
};
use std::{collections::BTreeMap, sync::Arc};
use support::{close, entity, key, scene};

const STEP: f64 = 1.0 / 60.0;
const HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn box_at(name: &str, position: [f32; 3], half_extents: [f32; 3]) -> EntityData {
	entity(name, position).with_collider(Some(ColliderData::Box {
		half_extents,
		offset: [0.0; 3],
		interaction: Default::default(),
	}))
}

fn authored(entities: Vec<EntityData>) -> SceneData {
	let mut data = scene(entities);
	data.camera.orbit = Some(OrbitCameraSettings {
		target: key("actor"),
		pivot_offset: [0.0; 3],
		initial_yaw: 0.0,
		initial_pitch: 0.0,
		initial_distance: 5.0,
		min_distance: 4.0,
		max_distance: 8.0,
		min_pitch: -1.0,
		max_pitch: 1.0,
		follow_seconds: 0.0,
		recovery_seconds: 0.2,
		teleport_distance: 10.0,
		probe_radius: 0.25,
		obstruction_mask: 1,
	});
	data
}

fn room(wall_z: f32) -> SceneData {
	authored(vec![
		entity("actor", [0.0; 3]),
		box_at("wall", [0.0, 0.0, wall_z], [2.0, 2.0, 0.25]),
	])
}

fn project(data: &SceneData) -> ProjectData {
	ProjectData::for_scene("Orbit obstruction", "scenes/main.json", data).unwrap()
}

fn game(data: SceneData, project: ProjectData) -> Result<GameSession, String> {
	let startup = project.startup_scene_id.clone();
	GameSession::with_factory_for_scene(
		Arc::new(project),
		startup,
		Arc::new(data),
		Arc::new(ConfiguredBehaviorFactory),
	)
}

fn simple_game(data: SceneData) -> GameSession {
	let project = project(&data);
	game(data, project).unwrap()
}

fn movement_game(data: SceneData, target: &str) -> GameSession {
	let mut project = project(&data);
	project.add_behavior(BehaviorSpec::ControlledMove {
		entity: key(target),
		speed: 60.0,
		space: BehaviorSpace::World,
	});
	game(data, project).unwrap()
}

fn submit(run: &mut GameSession, input: CameraIntent) {
	let owner = run.camera_input_owner().unwrap();
	run.submit_camera_input(&owner.run_id, owner.scene_generation, input)
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

fn assert_safe_distance(actual: f32, unguarded_limit: f32) {
	// The collision query subtracts a small conservative clearance and rounding guard.
	assert!(
		(unguarded_limit - 0.01..=unguarded_limit).contains(&actual),
		"distance {actual} is not safely near the geometric limit {unguarded_limit}"
	);
}

#[test]
fn admission_clamps_below_user_minimum_without_mutating_the_authored_camera() {
	let data = room(3.0);
	let mut run = simple_game(data.clone());
	let state = run.active().orbit_state().unwrap();
	assert_eq!(state.tick, 0);
	assert_eq!(state.user_distance, 5.0);
	assert_safe_distance(state.effective_distance, 2.5);
	assert_eq!(state.effective_distance, state.last_allowed_distance);
	assert!(state.effective_distance < data.camera.orbit.as_ref().unwrap().min_distance);
	assert_eq!(state.obstruction.as_ref().unwrap().blocker, key("wall"));
	close(run.active().camera().position[2], state.effective_distance);
	assert!(state.pivot_settled && state.arm_settled);
	assert!(!run.needs_tick() && !run.active().needs_tick());
	for _ in 0..1_000 {
		assert_eq!(run.tick(STEP).unwrap().steps, 0);
	}
	assert_eq!(run.active().tick_index(), 0);
	assert_eq!(run.active().document().as_ref(), &data);
}

#[test]
fn the_published_near_plane_clears_a_wall_that_only_grazes_its_corner() {
	let corners = |camera: &Camera| {
		let inverse = camera
			.view_projection(MAX_GAME_ASPECT as f32)
			.unwrap()
			.inverse();
		[[-1.0, -1.0], [-1.0, 1.0], [1.0, -1.0], [1.0, 1.0]]
			.map(|[x, y]| inverse.project_point3(glam::Vec3::new(x, y, 0.0)))
	};
	let actor = entity("actor", [0.0; 3]);
	let clear = simple_game(authored(vec![actor.clone()]));
	let corner = corners(clear.active().camera())[3];
	let half = 0.01;
	// In clear space this near-plane corner lands inside the thin wall. Its
	// nearest face in X is still strictly to the right of the center ray.
	assert!(corner.x - half > 0.0);
	let run = simple_game(authored(vec![
		actor,
		box_at("edge", corner.to_array(), [half; 3]),
	]));
	let state = run.active().orbit_state().unwrap();
	assert_eq!(state.obstruction.as_ref().unwrap().blocker, key("edge"));
	assert!(state.effective_distance < state.user_distance);
	// Every corner of the accepted plane lies on the pivot side of the wall.
	// The plane is convex, so the whole rectangle clears that face.
	assert!(
		corners(run.active().camera())
			.iter()
			.all(|p| p.z < corner.z - half)
	);
}

#[test]
fn arm_uses_the_same_steps_collision_resolved_target_position() {
	let data = authored(vec![
		box_at("actor", [0.0; 3], [0.3; 3]),
		box_at("wall", [0.0, 0.0, 3.0], [2.0, 2.0, 0.25]),
	]);
	let mut run = movement_game(data, "actor");
	run.active_mut()
		.set_input(&key("actor"), [0.0, 0.0, 1.0])
		.unwrap();
	for expected_tick in 1..=3 {
		let outcome = run.tick(STEP).unwrap();
		assert_eq!((outcome.steps, outcome.tick), (1, expected_tick));
		let position = world_position(run.active(), "actor");
		let state = run.active().orbit_state().unwrap();
		assert_eq!(state.filtered_pivot, position);
		assert_eq!(state.tick, outcome.tick);
		assert_eq!(state.obstruction.as_ref().unwrap().blocker, key("wall"));
		assert_safe_distance(state.effective_distance, 2.5 - position[2]);
		assert_safe_distance(run.active().camera().position[2], 2.5);
	}
	// The requested third meter was blocked; the camera follows accepted motion.
	let actual = world_position(run.active(), "actor")[2];
	assert!((2.40..2.451).contains(&actual), "target reached {actual}");
	assert!(run.active().orbit_state().unwrap().effective_distance < 0.06);
}

#[test]
fn moving_wall_clamps_immediately_then_recovers_until_the_camera_sleeps() {
	let mut run = movement_game(room(3.0), "wall");
	run.active_mut()
		.set_input(&key("wall"), [0.0, 0.0, -1.0])
		.unwrap();
	let inward = run.tick(STEP).unwrap();
	assert_eq!(inward.steps, 1);
	close(world_position(run.active(), "wall")[2], 2.0);
	assert_safe_distance(run.active().orbit_state().unwrap().effective_distance, 1.5);
	assert!(inward.changes.camera_changed);
	run.active_mut().clear_input();
	assert!(!run.needs_tick());

	run.active_mut()
		.set_input(&key("wall"), [0.0, 0.0, 1.0])
		.unwrap();
	let outward = run.tick(5.0 * STEP).unwrap();
	assert_eq!(outward.steps, 5);
	close(world_position(run.active(), "wall")[2], 7.0);
	run.active_mut().clear_input();
	let state = run.active().orbit_state().unwrap();
	assert_eq!(state.last_allowed_distance, 5.0);
	assert!(state.obstruction.is_none());
	assert!(state.effective_distance < 5.0 && !state.arm_settled);
	assert!(run.needs_tick() && run.active().needs_tick());
	let mut recovery_steps = 0;
	while run.needs_tick() && recovery_steps < 600 {
		let previous = run.active().orbit_state().unwrap().effective_distance;
		let outcome = run.tick(STEP).unwrap();
		assert_eq!(outcome.steps, 1);
		assert!(run.active().orbit_state().unwrap().effective_distance >= previous);
		recovery_steps += 1;
	}
	assert!(recovery_steps > 0 && recovery_steps < 600);
	let state = run.active().orbit_state().unwrap();
	assert_eq!(state.effective_distance, 5.0);
	assert!(state.pivot_settled && state.arm_settled);
	assert!(!run.needs_tick() && !run.active().needs_tick());
}

fn with_interaction(mut entity: EntityData, interaction: ColliderInteraction) -> EntityData {
	let Some(ColliderData::Box {
		interaction: value, ..
	}) = &mut entity.collider
	else {
		panic!("box fixture required")
	};
	*value = interaction;
	entity
}

#[test]
fn target_subtree_and_nonblockers_are_ignored_but_hidden_one_way_layer_wall_blocks() {
	let mut disabled = box_at("disabled", [0.0, 0.0, 0.6], [0.05; 3]);
	disabled.collision_enabled = false;
	let sensor = with_interaction(
		box_at("sensor", [0.0, 0.0, 0.5], [0.05; 3]),
		ColliderInteraction {
			sensor: true,
			..Default::default()
		},
	);
	let other_layer = with_interaction(
		box_at("other_layer", [0.0, 0.0, 1.5], [0.05; 3]),
		ColliderInteraction {
			layers: 2,
			..Default::default()
		},
	);
	let mut wall = with_interaction(
		box_at("wall", [0.0, 0.0, 3.5], [2.0, 2.0, 0.25]),
		ColliderInteraction {
			layers: 1,
			mask: 0,
			..Default::default()
		},
	);
	wall.renderable = false;
	let data = authored(vec![
		box_at("actor", [0.0; 3], [0.3; 3]),
		box_at("child", [0.0, 0.0, 1.0], [0.3; 3]).with_parent(Some(key("actor"))),
		box_at("grandchild", [0.0, 0.0, 1.0], [0.3; 3]).with_parent(Some(key("child"))),
		sensor,
		disabled,
		other_layer,
		wall,
	]);
	let mut run = simple_game(data.clone());
	let state = run.active().orbit_state().unwrap();
	assert_eq!(state.obstruction.as_ref().unwrap().blocker, key("wall"));
	assert_safe_distance(state.effective_distance, 3.0);
	let stats_before = run.active().collision_stats();
	submit(
		&mut run,
		CameraIntent {
			yaw_delta: 0.01,
			..Default::default()
		},
	);
	run.tick(STEP).unwrap();
	assert_eq!(
		run.active()
			.orbit_state()
			.unwrap()
			.obstruction
			.as_ref()
			.unwrap()
			.blocker,
		key("wall")
	);
	assert_eq!(run.active().document().as_ref(), &data);
	let stats_after = run.active().collision_stats();
	assert_eq!(stats_after.rebuilds, stats_before.rebuilds);
	assert_eq!(
		stats_after.updated_colliders,
		stats_before.updated_colliders
	);
}

#[test]
fn penetrating_admission_and_prepared_transitions_fail_without_touching_the_active_run() {
	let invalid = room(0.0);
	let invalid_project = project(&invalid);
	let error = game(invalid.clone(), invalid_project).err().unwrap();
	assert!(error.contains("penetrat"), "{error}");
	let data = room(3.0);
	let mut project = project(&data);
	let second = SceneId::new("second").unwrap();
	project
		.scenes
		.insert(SceneDescriptor::new(second.clone(), "scenes/second.json").unwrap())
		.unwrap();
	let mut run = game(data.clone(), project).unwrap();
	submit(
		&mut run,
		CameraIntent {
			yaw_delta: 0.1,
			..Default::default()
		},
	);
	run.tick(STEP).unwrap();
	submit(
		&mut run,
		CameraIntent {
			yaw_delta: 0.2,
			..Default::default()
		},
	);
	let state = run.active().orbit_state().unwrap().clone();
	let pose = camera_pose(run.active().camera());
	let owner = run.camera_input_owner().unwrap();
	let progress = run.progress_revision();
	let document = run.active().document();
	let startup = run.active().scene_id().clone();
	let transition_error = run
		.prepare_scene(second.clone(), Arc::new(invalid.clone()))
		.err()
		.unwrap();
	let restart_error = run
		.prepare_new_game(startup.clone(), Arc::new(invalid))
		.err()
		.unwrap();
	assert!(transition_error.contains("penetrat"), "{transition_error}");
	assert!(restart_error.contains("penetrat"), "{restart_error}");
	assert_eq!(run.active().orbit_state().unwrap(), &state);
	assert_eq!(camera_pose(run.active().camera()), pose);
	assert_eq!(run.camera_input_owner(), Some(owner.clone()));
	assert_eq!(run.active().document(), document);
	assert_eq!(run.progress_revision(), progress);
	assert_eq!(run.active().pending_camera_input().yaw_delta, 0.2);
	assert!(!run.active().paused());

	let transition = run
		.prepare_scene(second.clone(), Arc::new(room(2.0)))
		.unwrap();
	run.commit_scene(transition).unwrap();
	assert_eq!(run.active().scene_id(), &second);
	assert_eq!(run.run_id(), owner.run_id);
	assert_eq!(run.active().orbit_state().unwrap().tick, 0);
	assert_safe_distance(run.active().orbit_state().unwrap().effective_distance, 1.5);
	assert_eq!(run.active().pending_camera_input(), CameraIntent::default());
	let restart = run
		.prepare_new_game(startup, Arc::new(data.clone()))
		.unwrap();
	run.commit_scene(restart).unwrap();
	assert_ne!(run.run_id(), owner.run_id);
	assert_eq!(run.active().orbit_state().unwrap().tick, 0);
	assert_safe_distance(run.active().orbit_state().unwrap().effective_distance, 2.5);
	assert_eq!(run.active().document().as_ref(), &data);
}

#[test]
fn second_step_pivot_penetration_rolls_back_the_world_rig_and_collision_state_then_pauses() {
	// Prove the first candidate is valid, so the aggregate failure exercises rollback
	// after a successfully resolved world and rig rather than failing at admission.
	let mut first_step = movement_game(room(2.0), "wall");
	first_step
		.active_mut()
		.set_input(&key("wall"), [0.0, 0.0, -1.0])
		.unwrap();
	submit(
		&mut first_step,
		CameraIntent {
			yaw_delta: 0.1,
			..Default::default()
		},
	);
	assert_eq!(first_step.tick(STEP).unwrap().steps, 1);
	close(world_position(first_step.active(), "wall")[2], 1.0);
	assert!(
		first_step
			.active()
			.orbit_state()
			.unwrap()
			.effective_distance
			< 0.51
	);
	assert_ne!(first_step.active().orbit_state().unwrap().yaw, 0.0);

	let mut run = movement_game(room(2.0), "wall");
	let before_state = run.active().orbit_state().unwrap().clone();
	let before_pose = camera_pose(run.active().camera());
	let before_document = run.active().document();
	let before_native = run.active().native_replay_state();
	run.active_mut()
		.set_input(&key("wall"), [0.0, 0.0, -1.0])
		.unwrap();
	submit(
		&mut run,
		CameraIntent {
			yaw_delta: 0.1,
			..Default::default()
		},
	);
	let error = run.tick(2.0 * STEP).unwrap_err();
	assert!(error.contains("penetrat"), "{error}");
	assert!(run.active().paused());
	assert_eq!(run.active().tick_index(), 0);
	assert_eq!(run.active().orbit_state().unwrap(), &before_state);
	assert_eq!(camera_pose(run.active().camera()), before_pose);
	assert_eq!(run.active().document(), before_document);
	assert_eq!(run.active().native_replay_state(), before_native);
	assert_eq!(run.active().pending_camera_input(), CameraIntent::default());
	run.active_mut().set_paused(false);
	assert!(!run.needs_tick());
	// Fresh input must use the original wall/collision cache, not either failed candidate.
	run.active_mut()
		.set_input(&key("wall"), [0.0, 0.0, 1.0])
		.unwrap();
	let accepted = run.tick(STEP).unwrap();
	assert_eq!((accepted.tick, accepted.steps), (1, 1));
	close(world_position(run.active(), "wall")[2], 3.0);
	assert_safe_distance(
		run.active().orbit_state().unwrap().last_allowed_distance,
		2.5,
	);
}

#[test]
fn runtime_prefab_blocker_publishes_at_the_same_tick_and_penetrating_spawn_is_atomic() {
	let mut run = simple_game(authored(vec![entity("actor", [0.0; 3])]));
	let prefab = PrefabData {
		schema_version: 3,
		entities: vec![box_at("blocker", [0.0; 3], [2.0, 2.0, 0.25])],
		gameplay: None,
		reference_slots: BTreeMap::new(),
	};
	submit(
		&mut run,
		CameraIntent {
			yaw_delta: 0.1,
			..Default::default()
		},
	);
	let prepared = run
		.active()
		.prepare_runtime_spawn(&prefab, HASH, [0.0, 0.0, 2.0], None)
		.unwrap();
	let blocker = prepared.entities()[0].key.clone();
	let change = run.active_mut().commit_runtime_spawn(prepared).unwrap();
	assert!(change.camera_changed && change.topology_changed && change.full_resync);
	assert!(change.changed_entities.contains(&blocker));
	assert_eq!(run.active().tick_index(), 0);
	let state = run.active().orbit_state().unwrap();
	assert_eq!(state.tick, 0);
	assert_eq!(state.obstruction.as_ref().unwrap().blocker, blocker);
	assert_safe_distance(state.effective_distance, 1.5);
	close(run.active().camera().position[2], state.effective_distance);
	assert_eq!(run.active().pending_camera_input().yaw_delta, 0.1);
	assert_eq!(run.active().camera().yaw, 0.0);
	let before_state = state.clone();
	let before_document = run.active().document();
	let before_native = run.active().native_replay_state();
	let before_pose = camera_pose(run.active().camera());
	let invalid = run
		.active()
		.prepare_runtime_spawn(&prefab, HASH, [0.0; 3], None)
		.unwrap();
	let invalid_key = invalid.entities()[0].key.clone();
	let error = run.active_mut().commit_runtime_spawn(invalid).unwrap_err();
	assert!(error.contains("penetrat"), "{error}");
	assert_eq!(run.active().orbit_state().unwrap(), &before_state);
	assert_eq!(run.active().document(), before_document);
	assert_eq!(run.active().native_replay_state(), before_native);
	assert_eq!(camera_pose(run.active().camera()), before_pose);
	assert_eq!(run.active().pending_camera_input().yaw_delta, 0.1);
	assert!(run.active().runtime_handle(&invalid_key).is_none());
	assert!(!run.active().paused());
}

#[test]
fn an_expiring_wall_releases_the_zoomed_arm_on_the_despawn_step() {
	let mut emitter = entity("emitter", [0.0, 0.0, 4.0]);
	emitter.native_behaviors.push(NativeBehaviorComponent {
		id: ComponentId::new("emit-wall").unwrap(),
		enabled: true,
		behavior: NativeBehaviorSpec::TimedSpawner {
			prefab_path: "prefabs/wall.json".into(),
			interval_ticks: 1,
			max_alive: 1,
			lifetime_ticks: 3,
			spawn_offset: [0.0; 3],
			bindings: BTreeMap::new(),
		},
	});
	let authored = authored(vec![entity("actor", [0.0; 3]), emitter]);
	let mut run = movement_game(authored.clone(), "actor");
	assert_eq!(run.tick(STEP).unwrap().tick, 1);
	let request = run.active().pending_runtime_spawns().pop().unwrap();
	let wall = PrefabData {
		schema_version: 3,
		entities: vec![box_at("wall", [0.0; 3], [2.0, 2.0, 0.25])],
		gameplay: None,
		reference_slots: BTreeMap::new(),
	};
	let spawn = run
		.active()
		.prepare_runtime_spawn(&wall, HASH, request.position, Some(request.id))
		.unwrap();
	let blocker = spawn.entities()[0].key.clone();
	run.active_mut().commit_runtime_spawn(spawn).unwrap();
	assert_safe_distance(run.active().orbit_state().unwrap().effective_distance, 3.5);

	run.active_mut()
		.set_controlled_input([0.0, 0.0, 1.0])
		.unwrap();
	assert_eq!(run.tick(STEP).unwrap().tick, 2);
	run.active_mut().clear_input();
	assert_safe_distance(run.active().orbit_state().unwrap().effective_distance, 2.5);
	submit(
		&mut run,
		CameraIntent {
			yaw_delta: 0.1,
			log_distance_delta: (8.0_f64 / 5.0).ln(),
			..Default::default()
		},
	);
	assert_eq!(run.tick(STEP).unwrap().tick, 3);
	let blocked = run.active().orbit_state().unwrap().clone();
	assert_eq!(blocked.obstruction.as_ref().unwrap().blocker, blocker);
	assert_eq!((blocked.yaw, blocked.user_distance), (0.1, 8.0));
	assert!(blocked.effective_distance <= blocked.last_allowed_distance);
	assert_safe_distance(blocked.last_allowed_distance, 2.5 / blocked.yaw.cos());

	run.active_mut().set_paused(true);
	assert_eq!(run.tick(10.0).unwrap().steps, 0);
	assert_eq!(run.active().orbit_state().unwrap(), &blocked);
	run.active_mut().set_paused(false);
	let expired = run.tick(STEP).unwrap();
	assert_eq!((expired.steps, expired.tick), (1, 4));
	assert!(expired.changes.full_resync && expired.changes.topology_changed);
	assert!(expired.changes.camera_changed);
	assert!(run.active().runtime_handle(&blocker).is_none());
	let clear = run.active().orbit_state().unwrap();
	assert_eq!((clear.tick, clear.last_allowed_distance), (4, 8.0));
	assert!(clear.obstruction.is_none());
	let alpha = 1.0 - (-(STEP as f32) / 0.2).exp();
	close(
		clear.effective_distance,
		blocked.effective_distance + (8.0 - blocked.effective_distance) * alpha,
	);
	assert!(!clear.arm_settled);

	let restart = run
		.prepare_new_game(run.active().scene_id().clone(), Arc::new(authored))
		.unwrap();
	run.commit_scene(restart).unwrap();
	let reset = run.active().orbit_state().unwrap();
	assert_eq!(
		(
			reset.tick,
			reset.yaw,
			reset.user_distance,
			reset.effective_distance
		),
		(0, 0.0, 5.0, 5.0)
	);
	assert!(reset.obstruction.is_none());
}

#[test]
fn camera_and_sensor_sampling_share_the_aggregate_budget_and_rollback_both_steps() {
	// Overlapping X intervals make the sensor broad phase visit 999 * 998 / 2
	// pairs per step. Just 550 solids reach the camera's endpoint pass. Each
	// step fits separately, but together they exhaust the 1M allowance during
	// the second camera query, well below the 8192 visits allowed per cast.
	let mut entities = vec![entity("actor", [0.0; 3])];
	for i in 0..998 {
		let y = if i < 550 { 0.0 } else { 50.0 };
		entities.push(box_at(&format!("obstacle-{i}"), [0.0, y, 4.0], [0.25; 3]));
	}
	entities.push(with_interaction(
		box_at("sensor", [0.0, 100.0, 4.0], [0.25; 3]),
		ColliderInteraction {
			sensor: true,
			..Default::default()
		},
	));
	let data = authored(entities);
	let mut fixed_data = data.clone();
	fixed_data.camera.orbit = None;
	let mut fixed = movement_game(fixed_data, "actor");
	fixed
		.active_mut()
		.set_controlled_input([0.0, 0.0, 1.0])
		.unwrap();
	assert_eq!(fixed.tick(2.0 * STEP).unwrap().steps, 2);

	let mut run = movement_game(data, "actor");
	let before = run.active().orbit_state().unwrap().clone();
	let pose = camera_pose(run.active().camera());
	let document = run.active().document();
	let turn = CameraIntent {
		yaw_delta: 0.01,
		..Default::default()
	};
	run.active_mut()
		.set_controlled_input([0.0, 0.0, 1.0])
		.unwrap();
	submit(&mut run, turn);
	let error = run.tick(2.0 * STEP).unwrap_err();
	assert!(
		error.contains("Camera obstruction exhausted the shared collision work budget"),
		"{error}"
	);
	assert!(run.active().paused());
	assert_eq!(run.active().tick_index(), 0);
	assert_eq!(run.active().orbit_state().unwrap(), &before);
	assert_eq!(camera_pose(run.active().camera()), pose);
	assert_eq!(run.active().document(), document);
	assert_eq!(run.active().pending_camera_input(), CameraIntent::default());

	run.active_mut().set_paused(false);
	assert_eq!(run.tick(STEP).unwrap().steps, 1); // Initial sensor sample is still due.
	assert_eq!(world_position(run.active(), "actor"), [0.0; 3]);
	assert_eq!(run.active().orbit_state().unwrap().yaw, 0.0);
	run.active_mut()
		.set_controlled_input([0.0, 0.0, 1.0])
		.unwrap();
	submit(&mut run, turn);
	for z in [1.0, 2.0] {
		assert_eq!(run.tick(STEP).unwrap().steps, 1);
		assert_eq!(world_position(run.active(), "actor")[2], z);
	}
}
