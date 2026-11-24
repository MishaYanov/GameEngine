use super::*;
use engine_core::{
	InputState,
	components::{
		InputActionsRef, PlayerControllerComponent, PlayerControllerSettings, PlayerMovementSpace,
	},
	runtime_lifecycle::RuntimeEntityHandle,
	simulation_session::LocalInputScope,
};
use std::sync::Mutex;

fn actor(name: &str, x: f32, local: bool) -> EntityData {
	let mut actor = player();
	actor.name = name.into();
	actor.key = key(name);
	actor.transform.position[0] = x;
	actor.player_controller = Some(Box::new(PlayerControllerComponent {
		id: ComponentId::new("player").unwrap(),
		enabled: true,
		settings: PlayerControllerSettings {
			input: InputActionsRef::ProjectDefault,
			local,
			space: PlayerMovementSpace::World,
			control_graph: None,
		},
	}));
	actor
}

fn setup(others: Vec<EntityData>) -> Arc<engine_core::SceneData> {
	let mut data = room(others);
	data.entities[0] = actor("player", 0.0, true);
	Arc::new(data)
}

fn possession_project(data: &engine_core::SceneData) -> ProjectData {
	let mut without_local = data.clone();
	without_local.entities[0].player_controller = None;
	project(&without_local)
}

fn send(
	run: &mut SimulationSession,
	owner: &LocalInputScope,
	sequence: u64,
	keys: &[Key],
) -> Result<bool, String> {
	let mut input = InputState::default();
	for key in keys {
		input.press(*key);
	}
	let controls = run
		.project()
		.input_profile
		.as_ref()
		.unwrap()
		.resolve_keyboard(&input);
	run.submit_local_controls(owner, sequence, controls)
}

fn x(run: &SimulationSession, entity: &str) -> f32 {
	run.document()
		.entities
		.iter()
		.find(|actor| actor.key == key(entity))
		.unwrap()
		.transform
		.position[0]
}

#[test]
fn handoff_commits_at_a_fixed_step_and_requires_neutral_before_routing_again() {
	let authored = setup(vec![actor("second", 5.0, false)]);
	let pristine = authored.clone();
	let mut run =
		SimulationSession::new(possession_project(&authored), Arc::clone(&authored)).unwrap();
	tick(&mut run);
	let from = run.local_input_scope().unwrap();
	let target = run.runtime_handle(&key("second")).unwrap();

	assert!(send(&mut run, &from, 12, &[Key::D]).unwrap());
	assert!(!send(&mut run, &from, 11, &[Key::A]).unwrap());
	assert!(!send(&mut run, &from, 12, &[Key::Space]).unwrap());
	tick(&mut run);
	assert!(x(&run, "player") > 0.0);
	run.request_local_possession(0, Some(target.clone()))
		.unwrap();
	assert_eq!(run.tick(0.0).unwrap().steps, 0);
	assert_eq!(run.local_player(), Some(&from.target));
	let outcome = run.tick(1.0 / 60.0).unwrap();
	assert!(outcome.possession_changed);
	assert_eq!(run.local_player(), Some(&target));
	assert_eq!(run.local_possession_generation(), 1);
	assert!(!send(&mut run, &from, 13, &[Key::Space]).unwrap());
	assert!(!send(&mut run, &from, 14, &[]).unwrap());

	let old_speed = run
		.character_motion()
		.into_iter()
		.find(|motion| motion.entity.key == from.target.key)
		.unwrap()
		.planar_speed;
	let to_position = x(&run, "second");
	let to = run.local_input_scope().unwrap();
	assert!(send(&mut run, &to, 0, &[Key::D, Key::Space]).unwrap());
	assert!(
		run.tick(1.0 / 60.0)
			.unwrap()
			.accepted_action_presses
			.is_empty()
	);
	let slowed = run
		.character_motion()
		.into_iter()
		.find(|motion| motion.entity.key == from.target.key)
		.unwrap()
		.planar_speed;
	assert!(
		slowed < old_speed,
		"released input should decelerate the previous motor"
	);
	assert_eq!(x(&run, "second"), to_position);
	for _ in 0..20 {
		tick(&mut run);
	}
	let from_position = x(&run, "player");
	assert!(send(&mut run, &to, 1, &[]).unwrap());
	assert!(send(&mut run, &to, 2, &[Key::D]).unwrap());
	for _ in 0..3 {
		tick(&mut run);
	}
	assert_eq!(x(&run, "player"), from_position);
	assert!(x(&run, "second") > to_position);
	assert_eq!(&*authored, &*pristine);
	assert_eq!(run.document().local_player().unwrap().key, key("player"));
	assert!(run.request_local_possession(0, None).is_err());
}

#[test]
fn invalid_targets_and_scoped_samples_cannot_change_ownership() {
	let mut incompatible = actor("different", 5.0, false);
	incompatible
		.player_controller
		.as_mut()
		.unwrap()
		.settings
		.input = InputActionsRef::Asset {
		asset_id: engine_core::asset_catalog::AssetId::generate(),
	};
	let authored = setup(vec![incompatible]);
	let mut run =
		SimulationSession::new(possession_project(&authored), Arc::clone(&authored)).unwrap();
	let other = SimulationSession::new(possession_project(&authored), authored).unwrap();
	let initial = run.local_input_scope().unwrap();
	let floor = run.runtime_handle(&key("floor")).unwrap();
	let different = run.runtime_handle(&key("different")).unwrap();
	assert!(
		run.request_local_possession(0, Some(floor))
			.unwrap_err()
			.contains("enabled")
	);
	assert!(
		run.request_local_possession(0, Some(different))
			.unwrap_err()
			.contains("prepared actions")
	);
	assert!(
		run.request_local_possession(0, other.local_player().cloned())
			.unwrap_err()
			.contains("another world")
	);
	assert!(!send(&mut run, &other.local_input_scope().unwrap(), 1, &[Key::D]).unwrap());
	run.clear_input();
	assert!(!send(&mut run, &initial, 1, &[Key::D]).unwrap());
	let fresh = run.local_input_scope().unwrap();
	assert!(send(&mut run, &fresh, 1, &[]).unwrap());
	assert_eq!(run.local_player(), Some(&initial.target));
	assert_eq!(run.local_possession_generation(), 0);
}

#[derive(Clone)]
enum Intent {
	PossessAndDrive {
		possess: RuntimeEntityHandle,
		drive: RuntimeEntityHandle,
	},
	Drive(RuntimeEntityHandle),
	PossessAndRemove(RuntimeEntityHandle),
	Remove(RuntimeEntityHandle),
}

#[derive(Clone, Default)]
struct Handoff(Arc<Mutex<Option<Intent>>>);

impl GameModuleFactory for Handoff {
	fn create(&self, _: &ProjectData, _: &SceneId) -> Result<Box<dyn GameModule>, String> {
		Ok(Box::new(self.clone()))
	}
}

impl GameModule for Handoff {
	fn needs_tick(&self, _: &InputFrame) -> bool {
		true
	}

	fn plan(
		&self,
		_: &RuntimeWorld,
		input: &InputFrame,
		_: f32,
		actions: &mut ActionWriter<'_>,
	) -> Result<(), String> {
		if let Some(intent) = self.0.lock().unwrap().clone() {
			match intent {
				Intent::PossessAndDrive { possess, drive } => {
					if input.local_player() != Some(&possess) {
						actions.drive_character(possess.clone(), [1.0, 0.0], false)?;
						actions.set_local_possession(Some(possess))?;
					} else if input.local_player() != Some(&drive) {
						actions.drive_character(drive, [1.0, 0.0], false)?;
					}
				}
				Intent::Drive(target) => actions.drive_character(target, [1.0, 0.0], false)?,
				Intent::PossessAndRemove(target) => {
					actions.set_local_possession(Some(target.clone()))?;
					actions.push(SimulationAction::Despawn { handle: target })?;
				}
				Intent::Remove(handle) => actions.push(SimulationAction::Despawn { handle })?,
			}
		}
		Ok(())
	}
}

fn spawn_actor(run: &mut SimulationSession, name: &str, x: f32) -> RuntimeEntityHandle {
	let prefab = engine_core::editor::PrefabData {
		schema_version: 3,
		composition: Default::default(),
		entities: vec![actor(name, x, false)],
		gameplay: None,
		reference_slots: Default::default(),
	};
	let spawn = run
		.prepare_runtime_spawn(&prefab, &"a".repeat(64), [0.0; 3], None)
		.unwrap();
	let key = spawn.entities()[0].key.clone();
	run.commit_runtime_spawn(spawn).unwrap();
	run.runtime_handle(&key).unwrap()
}

#[test]
fn ai_uses_the_motor_after_release_and_failed_contending_input_rolls_back() {
	let authored = setup(vec![actor("second", 5.0, false)]);
	let actions = Handoff::default();
	let mut run =
		SimulationSession::with_factory(possession_project(&authored), authored, &actions).unwrap();
	let initial = run.local_player().unwrap().clone();
	let second = run.runtime_handle(&key("second")).unwrap();
	*actions.0.lock().unwrap() = Some(Intent::PossessAndDrive {
		possess: second.clone(),
		drive: initial.clone(),
	});
	tick(&mut run);
	assert_eq!(run.local_player(), Some(&second));
	for _ in 0..3 {
		tick(&mut run);
	}
	assert_eq!(
		x(&run, "second"),
		5.0,
		"the AI's previous motor input was released"
	);
	assert!(
		x(&run, "player") > 0.0,
		"AI drove the released actor through its Character motor"
	);

	let before = run.document();
	*actions.0.lock().unwrap() = Some(Intent::Drive(second.clone()));
	let error = run.tick(1.0 / 60.0).unwrap_err();
	assert!(error.contains("already owns"), "{error}");
	assert_eq!(&*run.document(), &*before);
	assert_eq!(run.local_player(), Some(&second));
	assert!(run.paused());
}

#[test]
fn removing_current_or_requested_target_neutralizes_local_input_atomically() {
	let authored = setup(vec![]);
	let actions = Handoff::default();
	let mut run =
		SimulationSession::with_factory(possession_project(&authored), authored, &actions).unwrap();
	let initial = run.local_player().unwrap().clone();
	let first = spawn_actor(&mut run, "first", 5.0);
	*actions.0.lock().unwrap() = Some(Intent::PossessAndRemove(first.clone()));
	tick(&mut run);
	assert_eq!(run.local_player(), Some(&initial));
	assert!(run.runtime_handle(&first.key).is_none());
	assert_eq!(run.local_possession_generation(), 0);

	let second = spawn_actor(&mut run, "second", 5.0);
	*actions.0.lock().unwrap() = None;
	run.request_local_possession(0, Some(second.clone()))
		.unwrap();
	tick(&mut run);
	let old_owner = run.local_input_scope().unwrap();
	assert_eq!(old_owner.target, second);
	assert!(send(&mut run, &old_owner, 1, &[Key::D]).unwrap());
	*actions.0.lock().unwrap() = Some(Intent::Remove(second));
	assert!(run.tick(1.0 / 60.0).unwrap().possession_changed);
	assert!(run.local_player().is_none());
	assert_eq!(run.local_possession_generation(), 2);
	assert!(!send(&mut run, &old_owner, 2, &[Key::D]).unwrap());
	assert!(!run.set_controlled_input([1.0, 0.0, 0.0]).unwrap());
	let position = x(&run, "player");
	*actions.0.lock().unwrap() = None;
	tick(&mut run);
	assert_eq!(
		x(&run, "player"),
		position,
		"removal must not route input back to the authored owner"
	);
	run.request_local_possession(2, Some(initial.clone()))
		.unwrap();
	tick(&mut run);
	assert_eq!(run.local_player(), Some(&initial));
}

#[test]
fn handoff_discards_the_old_motors_buffered_jump() {
	let mut authored = setup(vec![actor("second", 5.0, false)]);
	Arc::make_mut(&mut authored).entities[0].transform.position[1] = 3.0;
	let mut run = SimulationSession::new(possession_project(&authored), authored).unwrap();
	tick(&mut run);
	let owner = run.local_input_scope().unwrap();
	assert!(send(&mut run, &owner, 1, &[Key::Space]).unwrap());
	tick(&mut run);
	assert!(
		run.native_replay_state()["characters"]["states"]["player"]["jump_buffer_until"]
			.is_number()
	);
	run.request_local_possession(0, run.runtime_handle(&key("second")))
		.unwrap();
	tick(&mut run);
	assert!(
		run.native_replay_state()["characters"]["states"]["player"]["jump_buffer_until"].is_null()
	);
}

#[test]
fn physical_queue_overflow_retires_the_scope_and_mutes_held_input() {
	use engine_core::input::{ConditionalInputLayout, InputActionMap, InputLayoutCondition};
	let mut authored = setup(vec![]);
	Arc::make_mut(&mut authored).entities[0]
		.player_controller
		.as_mut()
		.unwrap()
		.settings
		.input = InputActionsRef::Asset {
		asset_id: engine_core::asset_catalog::AssetId::generate(),
	};
	let mut run = SimulationSession::new(possession_project(&authored), authored).unwrap();
	let mut map = InputActionMap::starter();
	map.schema_version = 2;
	map.layouts.push(ConditionalInputLayout {
		id: "room".into(),
		label: "Room".into(),
		when: InputLayoutCondition::SceneIs {
			scene_id: run.scene_id().clone(),
		},
		overrides: vec![],
	});
	let resolver: engine_core::simulation_session::InputActionsResolver =
		Arc::new(move |_| Ok(Some(map.clone())));
	run.set_input_actions_resolver(&resolver).unwrap();
	let owner = run.local_input_scope().unwrap();
	let held = std::collections::HashSet::from(["KeyD".into()]);
	let release = std::collections::HashSet::new();
	for sequence in 0..128 {
		let keys = if sequence % 2 == 0 { &held } else { &release };
		assert!(
			run.submit_local_action_codes(&owner, sequence, keys, Default::default())
				.unwrap()
		);
	}
	let error = run
		.submit_local_action_codes(&owner, 128, &held, Default::default())
		.unwrap_err();
	assert!(error.contains("local input cleared"));
	assert!(
		!run.submit_local_action_codes(&owner, 129, &release, Default::default())
			.unwrap()
	);
	let fresh = run.local_input_scope().unwrap();
	assert!(
		run.submit_local_action_codes(&fresh, 1, &held, Default::default())
			.unwrap()
	);
	tick(&mut run);
	assert_eq!(x(&run, "player"), 0.0);
	for (sequence, keys) in [(2, &release), (3, &held)] {
		assert!(
			run.submit_local_action_codes(&fresh, sequence, keys, Default::default())
				.unwrap()
		);
	}
	tick(&mut run);
	assert!(x(&run, "player") > 0.0);
}
