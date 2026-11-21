mod support;

use engine_core::{
	EntityKey, InputState, Key,
	command::{EditorCommandDto as Command, EditorCommandRequest},
	editor::EditorSession,
	input::{GameInputBinding, GamepadSample, InputProfile, MovementBindings},
	project::{BehaviorSpace, BehaviorSpec, ProjectData},
	scene_catalog::SceneId,
	simulation_session::{ActionWriter, GameModule, GameModuleFactory, InputFrame, MovementTarget},
	world::RuntimeWorld,
};
use std::{
	collections::HashSet,
	sync::atomic::{AtomicUsize, Ordering},
	sync::{Arc, Mutex},
};
use support::*;

fn profile() -> InputProfile {
	InputProfile {
		movement: MovementBindings::ARROWS,
		actions: vec![
			GameInputBinding {
				id: "move_right".into(),
				key: Key::ArrowRight,
				animation_parameter: Some("IsMoving".into()),
			},
			GameInputBinding {
				id: "interact".into(),
				key: Key::Space,
				animation_parameter: Some("IsMoving".into()),
			},
		],
		..InputProfile::default()
	}
}

fn execute(session: &mut EditorSession, command: Command) -> Result<(), String> {
	session.execute_command(EditorCommandRequest {
		expected_revision: session.revision(),
		command,
	})?;
	Ok(())
}

#[test]
fn project_edit_persists_input_profile_and_remains_undoable() {
	let data = scene(vec![entity("actor", [0.0; 3])]);
	let mut editor = EditorSession::new(data.clone()).unwrap();
	let legacy_project = editor.project().to_json().unwrap();
	assert!(
		ProjectData::from_json(&legacy_project)
			.unwrap()
			.input_profile
			.is_none()
	);
	let mut project = editor.project().clone();
	project.input_profile = Some(profile());
	execute(
		&mut editor,
		Command::SetProject {
			project: project.clone(),
		},
	)
	.unwrap();
	assert!(editor.snapshot().unwrap().project_dirty);
	assert_eq!(editor.authored_document().as_ref(), &data);
	let serialized = editor.project().to_json().unwrap();
	assert_eq!(ProjectData::from_json(&serialized).unwrap(), project);
	let directory = Directory::new("saved-game-input");
	let manifest = directory.0.join("project.json");
	std::fs::write(&manifest, &serialized).unwrap();
	let saved = ProjectData::load(&manifest).unwrap();
	let reopened = EditorSession::for_project(data, saved.clone()).unwrap();
	assert_eq!(reopened.project().input_profile.as_ref(), Some(&profile()));
	assert!(!reopened.snapshot().unwrap().project_dirty);
	execute(&mut editor, Command::Undo).unwrap();
	assert!(editor.project().input_profile.is_none());
	execute(&mut editor, Command::Redo).unwrap();
	assert_eq!(editor.project(), &saved);
}

#[test]
fn input_profile_defaults_legacy_actions_and_validates_new_bindings() {
	let old = r#"{"schema_version":1,"movement":{"negative_x":"a","positive_x":"d","negative_y":"s","positive_y":"w"}}"#;
	assert_eq!(
		InputProfile::from_json(old).unwrap(),
		InputProfile::default()
	);
	let mut profile = profile();
	profile.actions[1].key = profile.actions[0].key;
	assert!(profile.validate().is_ok());
	profile.actions[1].id = profile.actions[0].id.clone();
	assert!(profile.validate().unwrap_err().contains("Duplicate"));
	profile.actions[1].id = "éxample".into();
	assert!(profile.validate().unwrap_err().contains("Action IDs"));
	profile.actions[1].id = "interact".into();
	profile.actions[1].animation_parameter = Some("1bad".into());
	assert!(
		profile
			.validate()
			.unwrap_err()
			.contains("Animation parameters")
	);
	profile.actions[1].animation_parameter = None;
	profile.actions[1].key = Key::Escape;
	assert!(profile.validate().unwrap_err().contains("Escape"));
}

#[derive(Debug, Clone, PartialEq)]
struct RecordedInput {
	movement: [f32; 3],
	move_right: bool,
	interact: bool,
	moving_parameter: bool,
}

#[derive(Clone)]
struct RecordInput {
	actor: EntityKey,
	latest: Arc<Mutex<Option<RecordedInput>>>,
	interact_presses: Arc<AtomicUsize>,
}

impl GameModuleFactory for RecordInput {
	fn create(&self, _: &ProjectData, _: &SceneId) -> Result<Box<dyn GameModule>, String> {
		Ok(Box::new(self.clone()))
	}
}

impl GameModule for RecordInput {
	fn needs_tick(&self, _: &InputFrame) -> bool {
		true
	}

	fn plan(
		&self,
		_: &RuntimeWorld,
		input: &InputFrame,
		_: f32,
		_: &mut ActionWriter<'_>,
	) -> Result<(), String> {
		if input.action_pressed("interact") {
			self.interact_presses.fetch_add(1, Ordering::Relaxed);
		}
		*self.latest.lock().unwrap() = Some(RecordedInput {
			movement: input.movement(&self.actor),
			move_right: input.action_held("move_right"),
			interact: input.action_held("interact"),
			moving_parameter: input.animation_bool("IsMoving"),
		});
		Ok(())
	}
}

#[test]
fn native_codes_and_player_keys_feed_the_same_actions_and_parameters() {
	let data = scene(vec![entity("actor", [0.0; 3])]);
	let mut project = ProjectData::for_scene("Game", "scenes/main.json", &data).unwrap();
	let profile = profile();
	project.input_profile = Some(profile.clone());
	project.add_behavior(BehaviorSpec::ControlledMove {
		entity: key("actor"),
		speed: 1.0,
		space: BehaviorSpace::World,
	});
	let make_session = || {
		let recorder = RecordInput {
			actor: key("actor"),
			latest: Arc::default(),
			interact_presses: Arc::default(),
		};
		let mut session = EditorSession::for_project(data.clone(), project.clone()).unwrap();
		session
			.set_module_factory(Arc::new(recorder.clone()))
			.unwrap();
		execute(&mut session, Command::StartPlay).unwrap();
		(session, recorder)
	};
	let (mut editor, editor_recorder) = make_session();
	let (mut player, player_recorder) = make_session();
	let revision = editor.revision();
	let mut held_codes = HashSet::from(["ArrowRight".into(), "Space".into()]);
	let mut keys = InputState::default();
	keys.press(Key::ArrowRight);
	keys.press(Key::Space);
	for expected in [
		RecordedInput {
			movement: [1.0, 0.0, 0.0],
			move_right: true,
			interact: true,
			moving_parameter: true,
		},
		RecordedInput {
			movement: [0.0; 3],
			move_right: false,
			interact: true,
			moving_parameter: true,
		},
		RecordedInput {
			movement: [0.0; 3],
			move_right: false,
			interact: false,
			moving_parameter: false,
		},
	] {
		editor
			.set_game_keyboard_input_from_codes(&held_codes)
			.unwrap();
		player
			.set_game_keyboard_input(
				&MovementTarget::AllControlled,
				profile.resolve_keyboard(&keys),
			)
			.unwrap();
		editor.tick(1.0 / 30.0).unwrap();
		player.tick(1.0 / 30.0).unwrap();
		assert_eq!(
			editor_recorder.latest.lock().unwrap().as_ref(),
			Some(&expected)
		);
		assert_eq!(
			player_recorder.latest.lock().unwrap().as_ref(),
			Some(&expected)
		);
		if held_codes.remove("ArrowRight") {
			keys.release(Key::ArrowRight);
		} else {
			held_codes.clear();
			keys.reset();
			editor.clear_play_input();
			player.clear_play_input();
		}
	}
	assert_eq!(editor.revision(), revision);
	assert_eq!(editor.authored_document().as_ref(), &data);
	assert!(!editor.snapshot().unwrap().can_undo);

	// Lifecycle reset starts a neutral frame even when the preceding run used held actions.
	execute(&mut editor, Command::StopPlay).unwrap();
	execute(&mut player, Command::StopPlay).unwrap();
	execute(&mut editor, Command::StartPlay).unwrap();
	execute(&mut player, Command::StartPlay).unwrap();
	*editor_recorder.latest.lock().unwrap() = None;
	*player_recorder.latest.lock().unwrap() = None;
	editor.tick(1.0 / 30.0).unwrap();
	player.tick(1.0 / 30.0).unwrap();
	let neutral = RecordedInput {
		movement: [0.0; 3],
		move_right: false,
		interact: false,
		moving_parameter: false,
	};
	assert_eq!(
		editor_recorder.latest.lock().unwrap().as_ref(),
		Some(&neutral)
	);
	assert_eq!(
		player_recorder.latest.lock().unwrap().as_ref(),
		Some(&neutral)
	);
	assert_eq!(editor_recorder.interact_presses.load(Ordering::Relaxed), 1);
	assert_eq!(player_recorder.interact_presses.load(Ordering::Relaxed), 1);

	editor
		.set_game_keyboard_input_from_codes(&HashSet::from(["Space".into()]))
		.unwrap();
	editor
		.set_game_keyboard_input_from_codes(&HashSet::new())
		.unwrap();
	editor.tick(0.0).unwrap();
	assert_eq!(editor_recorder.interact_presses.load(Ordering::Relaxed), 1);
	editor.tick(3.0 / 60.0).unwrap();
	assert_eq!(editor_recorder.interact_presses.load(Ordering::Relaxed), 2);

	let start = [
		editor_recorder.interact_presses.load(Ordering::Relaxed),
		player_recorder.interact_presses.load(Ordering::Relaxed),
	];
	for (keyboard_held, gamepad_held, expected_edges) in [
		(true, false, 1),
		(true, true, 1),
		(false, true, 1),
		(false, false, 1),
		(false, true, 2),
	] {
		let mut keys = InputState::default();
		let mut held_codes = HashSet::new();
		if keyboard_held {
			keys.press(Key::Space);
			held_codes.insert("Space".into());
		}
		let sample = GamepadSample {
			interact: gamepad_held,
			..GamepadSample::default()
		};
		editor
			.set_game_controls_from_codes(&held_codes, sample)
			.unwrap();
		player
			.set_game_keyboard_input(
				&MovementTarget::AllControlled,
				profile.resolve_controls(&keys, sample),
			)
			.unwrap();
		editor.tick(1.0 / 30.0).unwrap();
		player.tick(1.0 / 30.0).unwrap();
		assert_eq!(
			editor_recorder.interact_presses.load(Ordering::Relaxed),
			start[0] + expected_edges
		);
		assert_eq!(
			player_recorder.interact_presses.load(Ordering::Relaxed),
			start[1] + expected_edges
		);
	}
}
