use std::collections::{BTreeMap, HashSet};

use engine_core::{
	Engine, EngineBuilder,
	command::{EditorCommandDto, EditorCommandRequest},
	input::{GameInputBinding, InputProfile, Key, MouseActionBinding, MouseButton},
	project::{BehaviorSpace, BehaviorSpec, SceneBehavior},
	scene_data::{CURRENT_SCHEMA_VERSION, ColliderData, TransformData},
};
use serde_json::json;

use super::*;

const FIXTURE_ROWS: [&str; 5] = ["#######", "#P123E#", "#.#.#.#", "#M....#", "#######"];
const JUMP_ROWS: [&str; 5] = ["########", "#PH123E#", "###.####", "#M.....#", "########"];
const FIXED_STEP: f64 = 1.0 / 60.0;

fn engine() -> Engine {
	engine_for(&FIXTURE_ROWS)
}

fn engine_for(rows: &[&str]) -> Engine {
	let mut entities = Vec::new();
	let mut bindings = BTreeMap::new();
	let board = Board::parse(
		&rows.iter().map(ToString::to_string).collect::<Vec<_>>(),
		2.4,
	)
		.unwrap();
	for ((role, name, _), cell) in ROLE_LABELS.into_iter().zip([
		board.player,
		board.monster,
		board.seals[0],
		board.seals[1],
		board.seals[2],
		board.exit,
	]) {
		let key = EntityKey::new(role).unwrap();
		let [x, z] = board.center(cell);
		let mut entity =
			EntityData::new(key.clone(), name, "test.glb").with_transform(TransformData {
				position: [x, 0.5, z],
				..Default::default()
			});
		if role == "gate" {
			entity = entity.with_collider(Some(ColliderData::Box {
				half_extents: [0.5; 3],
				offset: [0.; 3],
				interaction: Default::default(),
			}));
		}
		bindings.insert(role.into(), key);
		entities.push(entity);
	}
	let scene = SceneData {
		schema_version: CURRENT_SCHEMA_VERSION,
		entities,
		bindings,
		camera: Default::default(),
		default_game_camera: None,
		environment: Default::default(),
		lights: Vec::new(),
		organization: Default::default(),
	};
	let scene_id = SceneId::new("maze").unwrap();
	let mut project = ProjectData::for_scene_with_id(
		"The Hollow Maze",
		scene_id.clone(),
		"scenes/maze.json",
		&scene,
	)
		.unwrap();
	project.behaviors.push(SceneBehavior {
		scene_id,
		behavior: BehaviorSpec::ControlledMove {
			entity: EntityKey::new("player").unwrap(),
			speed: 1.0,
			space: BehaviorSpace::World,
		},
	});
	project.gameplay_module = Some(ID.into());
	project.gameplay_settings =
		Some(json!({"schema_version": 1, "scene": "maze", "rows": rows, "tile_size": 2.4}));
	project.input_profile = Some(InputProfile {
		actions: vec![
			GameInputBinding {
				id: "step".into(),
				key: Key::E,
				animation_parameter: None,
			},
			GameInputBinding {
				id: "jump".into(),
				key: Key::Space,
				animation_parameter: None,
			},
			GameInputBinding {
				id: "wait".into(),
				key: Key::Q,
				animation_parameter: None,
			},
		],
		mouse_actions: vec![
			MouseActionBinding {
				id: "step".into(),
				button: MouseButton::Left,
			},
			MouseActionBinding {
				id: "jump".into(),
				button: MouseButton::Right,
			},
		],
		..Default::default()
	});
	let mut engine = EngineBuilder::new(scene, project)
		.module_resolver(Arc::new(crate::resolve))
		.build()
		.unwrap();
	command(&mut engine, EditorCommandDto::StartPlay);
	engine
}

fn command(engine: &mut Engine, command: EditorCommandDto) {
	engine
		.execute(EditorCommandRequest {
			expected_revision: engine.session().revision(),
			command,
		})
		.unwrap();
}

fn keys(engine: &mut Engine, held: &[&str], frames: usize) {
	let held: HashSet<_> = held.iter().copied().map(str::to_owned).collect();
	engine.set_game_keyboard_input_from_codes(&held).unwrap();
	for _ in 0..frames {
		engine.tick(FIXED_STEP).unwrap();
	}
}

fn turn(engine: &mut Engine, code: &str) {
	keys(engine, &[code], 10);
	keys(engine, &[], 1);
}

fn progress(engine: &Engine) -> Run {
	*engine.session().play_progress::<Run>().unwrap()
}

fn entity(engine: &Engine, role: &str) -> EntityData {
	engine
		.session()
		.render_document()
		.entities
		.iter()
		.find(|entity| entity.key.as_str() == role)
		.unwrap()
		.clone()
}

#[test]
fn seals_open_the_gate_and_editor_restart_restores_the_board() {
	let mut engine = engine();
	let initial = progress(&engine);
	let original_gate = entity(&engine, "gate");
	assert!(
		engine
			.play_status()
			.unwrap()
			.gameplay
			.unwrap()
			.objective
			.contains("(0/3)")
	);

	keys(&mut engine, &["KeyD"], 4);
	let in_transit = entity(&engine, "player");
	assert!(in_transit.transform.position[0] > -4.8);
	assert!(in_transit.transform.position[0] < -2.4);
	assert_eq!(in_transit.transform.position[1], 0.5);
	assert_eq!(progress(&engine).seals, 0);

	keys(&mut engine, &["KeyD"], 24);
	assert_eq!(
		progress(&engine).seals,
		0b001,
		"holding a direction spends a single turn"
	);
	assert_eq!(progress(&engine).monster, initial.monster);
	assert!(progress(&engine).chase_due);
	assert!(!entity(&engine, "seal_1").renderable);
	assert!(entity(&engine, "gate").collision_enabled);

	keys(&mut engine, &[], 1);
	turn(&mut engine, "KeyD");
	assert_eq!(progress(&engine).seals, 0b011);
	assert_eq!(progress(&engine).monster, Cell { column: 1, row: 2 });
	turn(&mut engine, "KeyD");
	assert_eq!(progress(&engine).seals, ALL_SEALS);
	assert!(!entity(&engine, "gate").collision_enabled);
	assert!(!entity(&engine, "gate").renderable);
	turn(&mut engine, "KeyD");
	assert_eq!(progress(&engine).phase, Phase::Escaped);
	assert!(engine.play_status().unwrap().gameplay.unwrap().completed);

	command(&mut engine, EditorCommandDto::StopPlay);
	command(&mut engine, EditorCommandDto::StartPlay);
	assert_eq!(progress(&engine), initial);
	assert_eq!(entity(&engine, "gate"), original_gate);
	assert!(entity(&engine, "seal_1").renderable);
}

#[test]
fn walls_cost_no_turn_while_waiting_can_let_the_monster_catch_up() {
	let mut engine = engine();
	let initial = progress(&engine);
	turn(&mut engine, "KeyW");
	assert_eq!(
		progress(&engine),
		Run {
			facing: Direction::North,
			..initial
		}
	);
	turn(&mut engine, "KeyS");
	assert_eq!(progress(&engine).player, Cell { column: 1, row: 2 });
	assert!(progress(&engine).chase_due);
	turn(&mut engine, "KeyQ");
	let caught = progress(&engine);
	assert_eq!(caught.phase, Phase::Caught);
	assert_eq!(caught.monster, caught.player);
	assert!(engine.play_status().unwrap().gameplay.unwrap().completed);
}

#[test]
fn space_jumps_over_a_hurdle_once_and_collects_the_seal_on_landing() {
	let mut engine = engine_for(&JUMP_ROWS);
	let initial = progress(&engine);
	let takeoff = entity(&engine, "player").transform.position;

	turn(&mut engine, "KeyD");
	assert_eq!(
		progress(&engine),
		initial,
		"walking into a hurdle costs no turn"
	);

	keys(&mut engine, &["Space"], 9);
	let apex = entity(&engine, "player").transform.position;
	assert!((apex[0] - (takeoff[0] + 2.4)).abs() < 0.001);
	assert!((apex[1] - (takeoff[1] + JUMP_HEIGHT_METERS)).abs() < 0.001);
	assert_eq!(apex[2], takeoff[2]);
	assert_eq!(progress(&engine).seals, 0);

	keys(&mut engine, &["Space"], 36);
	let landed = progress(&engine);
	assert_eq!(landed.player, Cell { column: 3, row: 1 });
	assert_eq!(landed.seals, 0b001, "holding Space spends a single turn");
	assert_eq!(landed.monster, initial.monster);
	assert!(landed.chase_due);
	assert!(!entity(&engine, "seal_1").renderable);
	assert_eq!(entity(&engine, "player").transform.position[1], takeoff[1]);
}

#[test]
fn jump_uses_the_current_direction_and_can_be_caught_on_landing() {
	let mut engine = engine();
	turn(&mut engine, "KeyQ");
	keys(&mut engine, &["KeyS", "Space"], 9);
	assert_eq!(progress(&engine).facing, Direction::South);
	assert_eq!(progress(&engine).phase, Phase::Searching);
	assert!(entity(&engine, "player").transform.position[1] > 0.5);

	keys(&mut engine, &[], 9);
	let landed = progress(&engine);
	assert_eq!(landed.player, Cell { column: 1, row: 2 });
	assert_eq!(landed.phase, Phase::Caught);
	assert_eq!(landed.player, landed.monster);
	assert_eq!(entity(&engine, "player").transform.position[1], 0.5);
}

#[test]
fn blocked_jump_hops_in_place_and_spends_a_turn() {
	let mut engine = engine_for(&JUMP_ROWS);
	let initial = progress(&engine);
	let takeoff = entity(&engine, "player").transform.position;

	turn(&mut engine, "KeyW");
	keys(&mut engine, &["Space"], 9);
	let apex = entity(&engine, "player").transform.position;
	assert_eq!([apex[0], apex[2]], [takeoff[0], takeoff[2]]);
	assert!(apex[1] > takeoff[1]);

	keys(&mut engine, &[], 9);
	assert_eq!(progress(&engine).player, initial.player);
	assert!(progress(&engine).chase_due);
	assert_eq!(entity(&engine, "player").transform.position, takeoff);

	turn(&mut engine, "KeyQ");
	assert_eq!(progress(&engine).monster, initial.monster);
	assert!(!progress(&engine).chase_due);
}

#[test]
fn jump_edges_still_respect_locked_exits_and_consecutive_barriers() {
	let gated = [
		"#########",
		"#P123.HE#",
		"#########",
		"#MHH....#",
		"#########",
	]
		.map(String::from);
	let board = Board::parse(&gated, 2.4).unwrap();
	let before_hurdle = Cell { column: 5, row: 1 };
	assert_eq!(
		board.jump_destination(before_hurdle, Direction::East, false),
		None
	);
	assert_eq!(
		board.jump_destination(before_hurdle, Direction::East, true),
		Some(board.exit)
	);
	assert_eq!(
		board.jump_destination(board.monster, Direction::East, true),
		None
	);
	assert_eq!(
		board.jump_destination(Cell { column: 0, row: 0 }, Direction::West, true),
		None
	);
}

#[test]
fn validation_requires_unique_markers_and_seal_routes_before_the_gate_opens() {
	let duplicate = ["#######", "#P113E#", "#.#.#.#", "#M....#", "#######"].map(String::from);
	assert!(
		Board::parse(&duplicate, 2.4)
			.err()
			.unwrap()
			.contains("exactly one 1 cell")
	);
	let trapped = ["#######", "#P.1E2#", "#.#3###", "#M..###", "#######"].map(String::from);
	assert!(
		Board::parse(&trapped, 2.4)
			.err()
			.unwrap()
			.contains("paths to all seals")
	);
	let valid = Board::parse(&FIXTURE_ROWS.map(String::from), 2.4).unwrap();
	assert_eq!(valid.neighbor(valid.seals[2], Direction::East, false), None);
	assert_eq!(
		valid.neighbor(valid.seals[2], Direction::East, true),
		Some(valid.exit)
	);
}

#[test]
fn quick_mouse_edges_clear_the_hurdle_and_step_forward() {
	let mut engine = engine_for(&JUMP_ROWS);
	let start = progress(&engine);
	turn(&mut engine, "MouseLeft");
	assert_eq!(progress(&engine), start);

	keys(&mut engine, &["MouseRight"], 0);
	keys(&mut engine, &[], 20);
	let after_jump = progress(&engine);
	assert_eq!(after_jump.player.column, start.player.column + 2);
	assert_eq!(after_jump.seals, 0b001);

	keys(&mut engine, &["MouseLeft"], 0);
	keys(&mut engine, &[], 10);
	let after_step = progress(&engine);
	assert_eq!(after_step.player.column, after_jump.player.column + 1);
	assert_eq!(after_step.seals, 0b011);
}
