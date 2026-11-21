mod support;
use engine_core::{
	Key,
	character_controller::{CharacterControllerComponent, CharacterControllerSettings},
	components::{ComponentData, ComponentId, ComponentRecord},
	input::{GameInputBinding, InputProfile},
	project::{BehaviorSpace, BehaviorSpec, ProjectData},
	scene_catalog::SceneId,
	scene_data::{ColliderData, EntityData},
	simulation_session::{
		ActionWriter, GameModule, GameModuleFactory, InputFrame, SimulationAction,
		SimulationSession,
	},
	world::RuntimeWorld,
};
use std::sync::{
	Arc,
	atomic::{AtomicBool, Ordering},
};
use support::*;
fn box_at(name: &str, position: [f32; 3], half: [f32; 3]) -> EntityData {
	entity(name, position).with_collider(Some(ColliderData::Box {
		half_extents: half,
		offset: [0.; 3],
		interaction: Default::default(),
	}))
}
fn player() -> EntityData {
	let mut e = box_at("player", [0., 0.901, 0.], [0.3, 0.9, 0.3]);
	e.character_controller = Some(CharacterControllerComponent {
		id: ComponentId::new("character").unwrap(),
		enabled: true,
		settings: CharacterControllerSettings::default(),
	});
	e
}
fn room(extra: Vec<EntityData>) -> engine_core::SceneData {
	let mut entities = vec![player(), box_at("floor", [0., -0.5, 0.], [20., 0.5, 20.])];
	entities.extend(extra);
	scene(entities)
}
fn project(data: &engine_core::SceneData) -> ProjectData {
	let mut p = ProjectData::for_scene("Character", "scenes/main.json", data).unwrap();
	p.input_profile = Some(InputProfile {
		actions: vec![GameInputBinding {
			id: "jump".into(),
			key: Key::Space,
			animation_parameter: None,
		}],
		..Default::default()
	});
	p
}
fn session(data: engine_core::SceneData) -> SimulationSession {
	SimulationSession::new(project(&data), Arc::new(data)).unwrap()
}
fn position(s: &SimulationSession) -> [f32; 3] {
	s.world().data().entities[s.world().entity_index(&key("player")).unwrap()]
		.transform
		.position
}
fn tick(s: &mut SimulationSession) {
	s.tick(1. / 60.).unwrap();
}
#[test]
fn component_roundtrip_and_disabled_parented_authoring_preserve_settings() {
	let mut data = room(vec![]);
	let record = data.entities[0]
		.component_records()
		.into_iter()
		.find(|r| matches!(r.data, ComponentData::CharacterController { .. }))
		.unwrap();
	assert_eq!(
		serde_json::from_str::<ComponentRecord>(&serde_json::to_string(&record).unwrap()).unwrap(),
		record
	);
	let json = data.to_json().unwrap();
	assert_eq!(engine_core::SceneData::from_json(&json).unwrap(), data);
	assert!(!json.contains("velocity"));
	data.entities[0].parent = Some(key("floor"));
	assert!(
		SimulationSession::new(project(&data), Arc::new(data.clone()))
			.err()
			.unwrap()
			.contains("detach")
	);
	data.entities[0]
		.character_controller
		.as_mut()
		.unwrap()
		.enabled = false;
	let disabled = session(data);
	assert!(disabled.character_motion().is_empty());
}
#[test]
fn floor_rest_is_stable_and_walk_uses_xz() {
	let mut s = session(room(vec![]));
	for _ in 0..600 {
		tick(&mut s);
	}
	let resting = position(&s);
	assert!((resting[1] - 0.9001).abs() < 0.0002);
	assert!(s.character_motion()[0].grounded);
	s.set_character_input(&key("player"), [0., -1.], false)
		.unwrap();
	for _ in 0..60 {
		tick(&mut s);
	}
	assert!(position(&s)[2] < -3.5);
	assert!((position(&s)[1] - resting[1]).abs() < 0.0002);
}
#[test]
fn short_press_survives_zero_steps_and_hold_never_rejumps() {
	let mut s = session(room(vec![]));
	tick(&mut s);
	s.set_character_input(&key("player"), [0.; 2], true)
		.unwrap();
	s.set_character_input(&key("player"), [0.; 2], false)
		.unwrap();
	assert_eq!(s.tick(0.001).unwrap().steps, 0);
	s.tick(1. / 60. - 0.001).unwrap();
	assert_eq!(s.character_motion()[0].jumped_at_tick, Some(2));
	assert!(position(&s)[1] > 1.);
	s.set_character_input(&key("player"), [0.; 2], true)
		.unwrap();
	for _ in 0..120 {
		tick(&mut s);
	}
	assert_eq!(s.character_motion()[0].jumped_at_tick, Some(2));
	assert!(s.character_motion()[0].landed_at_tick.is_some());
	assert!(s.character_motion()[0].grounded);
}
#[test]
fn press_is_not_replayed_across_catchup_and_pause_discards_it() {
	let data = room(vec![]);
	let mut a = session(data.clone());
	let mut b = session(data);
	tick(&mut a);
	tick(&mut b);
	for s in [&mut a, &mut b] {
		s.set_character_input(&key("player"), [1., 0.], true)
			.unwrap();
	}
	a.tick(4. / 60.).unwrap();
	for _ in 0..4 {
		tick(&mut b);
	}
	assert_eq!(a.world().data(), b.world().data());
	assert_eq!(
		serde_json::to_value(a.character_motion()[0].clone()).unwrap()["jumped_at_tick"],
		serde_json::json!(2)
	);
	let mut paused = session(room(vec![]));
	tick(&mut paused);
	paused
		.set_character_input(&key("player"), [0.; 2], true)
		.unwrap();
	paused.set_paused(true);
	paused.set_paused(false);
	tick(&mut paused);
	assert_eq!(paused.character_motion()[0].jumped_at_tick, None);
}
#[test]
fn small_box_step_succeeds_but_tall_step_and_low_ceiling_block() {
	for (height, ceiling, should_pass) in
		[(0.2, false, true), (0.5, false, false), (0.2, true, false)]
	{
		let mut extra = vec![box_at(
			"step",
			[1.2, height / 2., 0.],
			[0.4, height / 2., 1.],
		)];
		if ceiling {
			extra.push(box_at("ceiling", [1.2, 2.05, 0.], [0.6, 0.05, 1.]));
		}
		let mut s = session(room(extra));
		s.set_character_input(&key("player"), [1., 0.], false)
			.unwrap();
		let mut highest = 0.0_f32;
		for _ in 0..60 {
			tick(&mut s);
			highest = highest.max(position(&s)[1]);
		}
		if should_pass {
			assert!(position(&s)[0] > 2., "{:?}", position(&s));
			assert!(highest > 1.09);
		} else {
			assert!(position(&s)[0] < 0.501, "{:?}", position(&s));
			assert!(highest < 0.91);
		}
	}
}
#[test]
fn sensor_floor_never_grounds_and_fall_speed_is_bounded() {
	let mut data = room(vec![]);
	if let Some(ColliderData::Box { interaction, .. }) = &mut data.entities[1].collider {
		interaction.sensor = true;
	}
	let mut s = session(data);
	for _ in 0..180 {
		tick(&mut s);
	}
	let state = &s.character_motion()[0];
	assert!(!state.grounded);
	assert_eq!(state.vertical_speed, -30.);
	assert!(position(&s)[1] < -40.);
}
#[derive(Clone)]
struct Fail(Arc<AtomicBool>);
impl GameModuleFactory for Fail {
	fn create(&self, _: &ProjectData, _: &SceneId) -> Result<Box<dyn GameModule>, String> {
		Ok(Box::new(self.clone()))
	}
}
impl GameModule for Fail {
	fn needs_tick(&self, _: &InputFrame) -> bool {
		true
	}
	fn plan(
		&self,
		_: &RuntimeWorld,
		_: &InputFrame,
		_: f32,
		out: &mut ActionWriter<'_>,
	) -> Result<(), String> {
		if self.0.load(Ordering::Relaxed) {
			out.push(SimulationAction::SetVisible {
				entity: key("missing"),
				visible: false,
			})?;
		}
		Ok(())
	}
}
#[test]
fn late_failure_rolls_back_world_jump_and_motion_snapshot() {
	let data = room(vec![]);
	let flag = Fail(Arc::new(AtomicBool::new(false)));
	let mut s = SimulationSession::with_factory(project(&data), Arc::new(data), &flag).unwrap();
	tick(&mut s);
	let before = s.world().data().clone();
	let motion = serde_json::to_value(s.character_motion()).unwrap();
	s.set_character_input(&key("player"), [1., 0.], true)
		.unwrap();
	flag.0.store(true, Ordering::Relaxed);
	assert!(s.tick(1. / 60.).is_err());
	assert_eq!(&before, s.world().data());
	assert_eq!(motion, serde_json::to_value(s.character_motion()).unwrap());
	assert!(s.paused());
}
#[test]
fn second_movement_authority_and_invalid_body_are_rejected() {
	let data = room(vec![]);
	let mut p = project(&data);
	p.add_behavior(BehaviorSpec::ControlledMove {
		entity: key("player"),
		speed: 1.,
		space: BehaviorSpace::World,
	});
	assert!(
		SimulationSession::new(p, Arc::new(data.clone()))
			.err()
			.unwrap()
			.contains("owns")
	);
	let mut parented = data;
	parented.entities[0].transform.scale = [2.; 3];
	assert!(SimulationSession::new(project(&parented), Arc::new(parented)).is_err());
}

#[test]
fn coyote_jump_and_buffered_landing_use_simulation_ticks() {
	let mut data = room(vec![]);
	data.entities[1] = box_at("floor", [-2., -0.5, 0.], [2.5, 0.5, 2.]);
	let mut s = session(data);
	s.set_character_input(&key("player"), [1., 0.], false)
		.unwrap();
	for _ in 0..60 {
		tick(&mut s);
		if !s.character_motion()[0].grounded {
			break;
		}
	}
	assert!(!s.character_motion()[0].grounded);
	s.set_character_input(&key("player"), [1., 0.], true)
		.unwrap();
	tick(&mut s);
	assert!(s.character_motion()[0].vertical_speed > 0.);
	assert_eq!(s.character_motion()[0].jumped_at_tick, Some(s.tick_index()));

	let mut data = room(vec![]);
	data.entities[0].transform.position[1] = 3.;
	let mut s = session(data);
	for _ in 0..120 {
		tick(&mut s);
		if position(&s)[1] < 1.2 {
			break;
		}
	}
	assert!(!s.character_motion()[0].grounded);
	s.set_character_input(&key("player"), [0.; 2], true)
		.unwrap();
	s.set_character_input(&key("player"), [0.; 2], false)
		.unwrap();
	for _ in 0..8 {
		tick(&mut s);
		if s.character_motion()[0].jumped_at_tick.is_some() {
			break;
		}
	}
	let motion = &s.character_motion()[0];
	assert_eq!(
		motion.jumped_at_tick,
		motion.landed_at_tick.map(|tick| tick + 1)
	);
	assert!(motion.jumped_at_tick.is_some());
}

#[test]
fn linked_controller_needs_unpack_and_detach_and_disabled_record_is_preserved() {
	use engine_core::{
		asset_catalog::AssetId,
		command::{EditorCommandDto as C, EditorCommandRequest},
		editor::{EditorSession, PrefabData},
		linked_prefab::LinkedPrefabSource,
	};
	let data = scene(vec![box_at("floor", [0., -0.5, 0.], [10., 0.5, 10.])]);
	let mut editor = EditorSession::for_project(data.clone(), project(&data)).unwrap();
	fn run(editor: &mut EditorSession, command: C) -> Result<(), String> {
		editor
			.execute_command(EditorCommandRequest {
				expected_revision: editor.revision(),
				command,
			})
			.map(|_| ())
	}
	let source = LinkedPrefabSource {
		source_id: AssetId::generate(),
		path: "prefabs/player.json".into(),
		fingerprint: "a".repeat(64),
		prefab: PrefabData {
			schema_version: 3,
			entities: vec![player()],
			gameplay: None,
			reference_slots: Default::default(),
		},
	};
	run(
		&mut editor,
		C::InstantiateLinkedPrefab {
			source,
			position: [0.; 3],
			bindings: Default::default(),
		},
	)
	.unwrap();
	let document = editor.authored_document();
	let anchor = document
		.entities
		.iter()
		.find(|e| e.linked_prefab.is_some())
		.unwrap()
		.key
		.clone();
	let body = document
		.entities
		.iter()
		.find(|e| e.character_controller.is_some())
		.unwrap()
		.key
		.clone();
	let json = document.to_json().unwrap();
	assert_eq!(
		&engine_core::SceneData::from_json(&json).unwrap(),
		document.as_ref()
	);
	assert!(
		run(&mut editor, C::StartPlay)
			.unwrap_err()
			.contains("detach")
	);
	run(
		&mut editor,
		C::UnpackLinkedPrefab {
			root_key: anchor.as_str().into(),
		},
	)
	.unwrap();
	assert!(
		run(&mut editor, C::StartPlay)
			.unwrap_err()
			.contains("detach")
	);
	run(
		&mut editor,
		C::Reparent {
			key: body.as_str().into(),
			parent: None,
			keep_world: true,
		},
	)
	.unwrap();
	run(&mut editor, C::StartPlay).unwrap();
	editor.tick(1. / 60.).unwrap();
	assert_eq!(
		editor
			.runtime_inspection(0, 128)
			.unwrap()
			.character_motion
			.len(),
		1
	);
	run(&mut editor, C::StopPlay).unwrap();
	assert_eq!(
		editor
			.authored_document()
			.entities
			.iter()
			.find(|e| e.key == body)
			.unwrap()
			.transform
			.position,
		[0., 0.901, 0.]
	);
}

#[test]
fn native_code_and_player_event_samples_capture_short_taps_identically() {
	use engine_core::{
		InputState,
		command::{EditorCommandDto as C, EditorCommandRequest},
		editor::EditorSession,
		simulation_session::MovementTarget,
	};
	let data = room(vec![]);
	let p = project(&data);
	let mut a = EditorSession::for_project(data.clone(), p.clone()).unwrap();
	let mut b = EditorSession::for_project(data, p.clone()).unwrap();
	for e in [&mut a, &mut b] {
		e.execute_command(EditorCommandRequest {
			expected_revision: e.revision(),
			command: C::StartPlay,
		})
		.unwrap();
		e.tick(1. / 60.).unwrap();
	}
	a.set_game_keyboard_input_from_codes(&["Space".into(), "KeyW".into()].into_iter().collect())
		.unwrap();
	a.set_game_keyboard_input_from_codes(&["KeyW".into()].into_iter().collect())
		.unwrap();
	let mut input = InputState::default();
	input.press(Key::W);
	input.press(Key::Space);
	b.set_game_keyboard_input(
		&MovementTarget::AllControlled,
		p.input_profile.as_ref().unwrap().resolve_keyboard(&input),
	)
	.unwrap();
	input.release(Key::Space);
	b.set_game_keyboard_input(
		&MovementTarget::AllControlled,
		p.input_profile.as_ref().unwrap().resolve_keyboard(&input),
	)
	.unwrap();
	a.tick(4. / 60.).unwrap();
	for _ in 0..4 {
		b.tick(1. / 60.).unwrap();
	}
	assert_eq!(
		a.render_world().unwrap().data(),
		b.render_world().unwrap().data()
	);
	assert_eq!(
		a.runtime_inspection(0, 128).unwrap().character_motion[0].jumped_at_tick,
		Some(2)
	);
	assert!(
		a.render_world().unwrap().data().entities[0]
			.transform
			.position[2]
			< 0.
	);
}

#[test]
fn keyboard_targets_do_not_cross_controller_and_legacy_axes_or_owners() {
	use engine_core::{
		InputState,
		command::{EditorCommandDto as C, EditorCommandRequest},
		editor::EditorSession,
		simulation_session::MovementTarget,
	};
	let mut other = player();
	other.key = key("other");
	other.transform.position[0] = 4.;
	let data = room(vec![other, entity("legacy", [8., 3., 0.])]);
	let mut p = project(&data);
	p.add_behavior(BehaviorSpec::ControlledMove {
		entity: key("legacy"),
		speed: 1.,
		space: BehaviorSpace::World,
	});
	let mut e = EditorSession::for_project(data, p.clone()).unwrap();
	e.execute_command(EditorCommandRequest {
		expected_revision: e.revision(),
		command: C::StartPlay,
	})
	.unwrap();
	e.tick(1. / 60.).unwrap();
	let mut input = InputState::default();
	input.press(Key::W);
	input.press(Key::Space);
	e.set_game_keyboard_input(
		&MovementTarget::Entity(key("player")),
		p.input_profile.as_ref().unwrap().resolve_keyboard(&input),
	)
	.unwrap();
	e.tick(1. / 60.).unwrap();
	let world = e.render_world().unwrap();
	assert!(world.data().entities[0].transform.position[2] < 0.);
	assert_eq!(world.data().entities[2].transform.position[2], 0.);
	assert_eq!(world.data().entities[3].transform.position[1], 3.);
	e.set_game_keyboard_input(
		&MovementTarget::AllControlled,
		p.input_profile.as_ref().unwrap().resolve_keyboard(&input),
	)
	.unwrap();
	e.tick(1. / 60.).unwrap();
	let world = e.render_world().unwrap();
	assert!(world.data().entities[2].transform.position[2] < 0.);
	assert!(world.data().entities[3].transform.position[1] > 3.);
}

#[test]
fn focus_neutralization_discards_held_edges_without_suppressing_a_fresh_action() {
	use engine_core::{
		InputState,
		command::{EditorCommandDto as C, EditorCommandRequest},
		editor::EditorSession,
		simulation_session::MovementTarget,
	};
	for held_at_loss in [false, true] {
		let data = room(vec![]);
		let p = project(&data);
		let mut e = EditorSession::for_project(data, p.clone()).unwrap();
		e.execute_command(EditorCommandRequest {
			expected_revision: e.revision(),
			command: C::StartPlay,
		})
		.unwrap();
		e.tick(1. / 60.).unwrap();
		let mut input = InputState::default();
		if held_at_loss {
			input.press(Key::Space);
		}
		e.set_game_keyboard_input(
			&MovementTarget::AllControlled,
			p.input_profile.as_ref().unwrap().resolve_keyboard(&input),
		)
		.unwrap();
		input.suppress_held();
		e.clear_play_input();
		if held_at_loss {
			input.press(Key::Space); // focus restoration/repeat cannot recreate the held edge
			e.set_game_keyboard_input(
				&MovementTarget::AllControlled,
				p.input_profile.as_ref().unwrap().resolve_keyboard(&input),
			)
			.unwrap();
			e.tick(1. / 60.).unwrap();
			assert_eq!(
				e.runtime_inspection(0, 128).unwrap().character_motion[0].jumped_at_tick,
				None
			);
			input.release(Key::Space); // an observed release (including synthetic OS release) re-arms
		}
		input.press(Key::Space);
		e.set_game_keyboard_input(
			&MovementTarget::AllControlled,
			p.input_profile.as_ref().unwrap().resolve_keyboard(&input),
		)
		.unwrap();
		e.tick(1. / 60.).unwrap();
		assert!(
			e.runtime_inspection(0, 128).unwrap().character_motion[0]
				.jumped_at_tick
				.is_some()
		);
	}
}

#[test]
fn short_jump_survives_spawn_admission_drain_without_being_replayed() {
	use engine_core::{
		components::{NativeBehaviorComponent, NativeBehaviorSpec},
		editor::PrefabData,
	};
	let mut spawner = entity("spawner", [8., 0., 0.]);
	spawner.native_behaviors.push(NativeBehaviorComponent {
		id: ComponentId::new("spawn").unwrap(),
		enabled: true,
		behavior: NativeBehaviorSpec::TimedSpawner {
			prefab_path: "prefabs/item.json".into(),
			interval_ticks: 1,
			max_alive: 1,
			lifetime_ticks: 1000,
			spawn_offset: [0.; 3],
			bindings: Default::default(),
		},
	});
	let mut data = room(vec![spawner]);
	data.entities[0]
		.character_controller
		.as_mut()
		.unwrap()
		.settings
		.jump_buffer_ticks = 120;
	let mut whole = session(data.clone());
	let mut split = session(data);
	let item = PrefabData {
		schema_version: 3,
		entities: vec![entity("item", [0.; 3])],
		gameplay: None,
		reference_slots: Default::default(),
	};
	fn frame(s: &mut SimulationSession, item: &PrefabData, mut elapsed: f64) -> u32 {
		let mut steps = 0;
		for _ in 0..8 {
			let report = s.tick(elapsed).unwrap();
			steps += report.steps;
			for request in s.pending_runtime_spawns() {
				let candidate = s
					.prepare_runtime_spawn(
						item,
						&"a".repeat(64),
						request.position,
						Some(request.id),
					)
					.unwrap();
				s.commit_runtime_spawn(candidate).unwrap();
			}
			if report.pending_steps == 0 {
				return steps;
			}
			elapsed = 0.;
		}
		panic!("unbounded drain");
	}
	for s in [&mut whole, &mut split] {
		s.set_character_input(&key("player"), [0.; 2], true)
			.unwrap();
		s.set_character_input(&key("player"), [0.; 2], false)
			.unwrap();
	}
	assert_eq!(frame(&mut whole, &item, 4. / 60.), 4);
	for _ in 0..4 {
		assert_eq!(frame(&mut split, &item, 1. / 60.), 1);
	}
	assert_eq!(whole.world().data(), split.world().data());
	for _ in 0..120 {
		frame(&mut whole, &item, 1. / 60.);
		frame(&mut split, &item, 1. / 60.);
	}
	assert_eq!(whole.world().data(), split.world().data());
	assert_eq!(whole.character_motion()[0].jumped_at_tick, Some(1));
	assert!(whole.character_motion()[0].grounded);
}

#[derive(Clone)]
enum BoundaryAction {
	Collider(engine_core::EntityKey, bool),
	Remove(engine_core::runtime_lifecycle::RuntimeEntityHandle),
}
#[derive(Clone, Default)]
struct BoundaryScript(Arc<std::sync::Mutex<Option<BoundaryAction>>>);
impl GameModuleFactory for BoundaryScript {
	fn create(&self, _: &ProjectData, _: &SceneId) -> Result<Box<dyn GameModule>, String> {
		Ok(Box::new(self.clone()))
	}
}
impl GameModule for BoundaryScript {
	fn needs_tick(&self, _: &InputFrame) -> bool {
		true
	}
	fn plan(
		&self,
		_: &RuntimeWorld,
		_: &InputFrame,
		_: f32,
		out: &mut ActionWriter<'_>,
	) -> Result<(), String> {
		if let Some(action) = self.0.lock().unwrap().clone() {
			out.push(match action {
				BoundaryAction::Collider(entity, enabled) => {
					SimulationAction::SetCollisionEnabled { entity, enabled }
				}
				BoundaryAction::Remove(handle) => SimulationAction::Despawn { handle },
			})?;
		}
		Ok(())
	}
}

#[test]
fn modules_cannot_enable_a_second_body_under_the_controller() {
	let mut child = box_at("child", [0.; 3], [0.1; 3]);
	child.parent = Some(key("player"));
	child.collision_enabled = false;
	let data = room(vec![child]);
	let script = BoundaryScript::default();
	let mut s = SimulationSession::with_factory(project(&data), Arc::new(data), &script).unwrap();
	tick(&mut s);
	let before = s.world().data().clone();
	*script.0.lock().unwrap() = Some(BoundaryAction::Collider(key("child"), true));
	assert!(s.tick(1. / 60.).unwrap_err().contains("owns"));
	assert_eq!(&before, s.world().data());
}

#[test]
fn removed_or_disabled_support_is_cleared_before_publishing_motion() {
	for remove in [false, true] {
		let data = scene(vec![player()]);
		let script = BoundaryScript::default();
		let mut s =
			SimulationSession::with_factory(project(&data), Arc::new(data), &script).unwrap();
		let item = engine_core::editor::PrefabData {
			schema_version: 3,
			entities: vec![box_at("floor", [0., -0.5, 0.], [20., 0.5, 20.])],
			gameplay: None,
			reference_slots: Default::default(),
		};
		let candidate = s
			.prepare_runtime_spawn(&item, &"a".repeat(64), [0.; 3], None)
			.unwrap();
		s.commit_runtime_spawn(candidate).unwrap();
		let support = s
			.world()
			.data()
			.entities
			.iter()
			.find(|e| e.key != key("player"))
			.unwrap()
			.key
			.clone();
		tick(&mut s);
		assert!(s.character_motion()[0].grounded);
		*script.0.lock().unwrap() = Some(if remove {
			BoundaryAction::Remove(s.runtime_handle(&support).unwrap())
		} else {
			BoundaryAction::Collider(support, false)
		});
		tick(&mut s);
		assert!(!s.character_motion()[0].grounded);
		*script.0.lock().unwrap() = None;
		tick(&mut s);
		assert!(s.character_motion()[0].vertical_speed < 0.);
	}
}

#[test]
fn coyote_deadline_is_inclusive_and_expired_buffer_cannot_jump_on_landing() {
	for (wait_ticks, allowed) in [(4, true), (5, false)] {
		let mut data = room(vec![]);
		data.entities[1] = box_at("floor", [-2., -0.5, 0.], [2.5, 0.5, 2.]);
		let mut s = session(data);
		s.set_character_input(&key("player"), [1., 0.], false)
			.unwrap();
		for _ in 0..60 {
			tick(&mut s);
			if !s.character_motion()[0].grounded {
				break;
			}
		}
		assert!(!s.character_motion()[0].grounded);
		for _ in 0..wait_ticks {
			tick(&mut s);
		}
		s.set_character_input(&key("player"), [1., 0.], true)
			.unwrap();
		tick(&mut s);
		assert_eq!(s.character_motion()[0].jumped_at_tick.is_some(), allowed);
	}
	let mut data = room(vec![]);
	data.entities[0].transform.position[1] = 3.;
	data.entities[0]
		.character_controller
		.as_mut()
		.unwrap()
		.settings
		.jump_buffer_ticks = 0;
	let mut s = session(data);
	s.set_character_input(&key("player"), [0.; 2], true)
		.unwrap();
	s.set_character_input(&key("player"), [0.; 2], false)
		.unwrap();
	for _ in 0..120 {
		tick(&mut s);
	}
	assert!(s.character_motion()[0].grounded);
	assert_eq!(s.character_motion()[0].jumped_at_tick, None);
}

#[test]
fn ceiling_contact_clips_upward_velocity_and_lands_once() {
	let mut s = session(room(vec![box_at("ceiling", [0., 2., 0.], [2., 0.1, 2.])]));
	tick(&mut s);
	s.set_character_input(&key("player"), [0.; 2], true)
		.unwrap();
	tick(&mut s);
	assert!(position(&s)[1] <= 1.0002);
	assert_eq!(s.character_motion()[0].vertical_speed, 0.);
	for _ in 0..60 {
		tick(&mut s);
		assert!(position(&s)[1] <= 1.0002);
	}
	let landed = s.character_motion()[0].landed_at_tick;
	assert!(landed.is_some());
	assert!(s.character_motion()[0].grounded);
	for _ in 0..60 {
		tick(&mut s);
	}
	assert_eq!(s.character_motion()[0].landed_at_tick, landed);
	assert_eq!(s.character_motion()[0].jumped_at_tick, Some(2));
}
