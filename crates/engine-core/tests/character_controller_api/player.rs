use super::*;
use engine_core::{
	asset_catalog::AssetId,
	command::{EditorCommandDto as Command, EditorCommandRequest},
	components::PLAYER_CONTROLLER_TYPE,
	editor::{EditorSession, PrefabData},
	linked_prefab::LinkedPrefabSource,
};
use std::collections::HashSet;

fn conditional_controls(
	configure: impl FnOnce(&SceneId) -> engine_core::input::InputActionMap,
) -> SimulationSession {
	use engine_core::{
		asset_catalog::AssetId,
		components::{InputActionsRef, PlayerControllerComponent, PlayerControllerSettings},
		simulation_session::InputActionsResolver,
	};
	let mut data = room(vec![]);
	let project = project(&data);
	data.entities[0].player_controller = Some(Box::new(PlayerControllerComponent {
		id: ComponentId::new("player_controller").unwrap(),
		enabled: true,
		settings: PlayerControllerSettings {
			local: true,
			input: InputActionsRef::Asset {
				asset_id: AssetId::generate(),
			},
			..Default::default()
		},
	}));
	let mut run = SimulationSession::new(project, Arc::new(data)).unwrap();
	let map = configure(run.scene_id());
	let resolver: InputActionsResolver = Arc::new(move |_| Ok(Some(map.clone())));
	run.set_input_actions_resolver(&resolver).unwrap();
	run
}

fn motion_layouts() -> Vec<engine_core::input::ConditionalInputLayout> {
	use engine_core::input::{ConditionalInputLayout, InputLayoutCondition};
	[("land", true), ("airborne", false)]
		.into_iter()
		.map(|(id, value)| ConditionalInputLayout {
			id: id.into(),
			label: id.into(),
			when: InputLayoutCondition::CharacterGrounded { value },
			overrides: vec![],
		})
		.collect()
}

#[test]
fn conditional_layouts_wait_for_committed_motion_and_respect_first_matching_scene() {
	use engine_core::input::{ConditionalInputLayout, InputActionMap, InputLayoutCondition};
	let mut map = InputActionMap::starter();
	map.schema_version = 2;
	map.layouts = motion_layouts();
	let mut run = conditional_controls(|_| map.clone());
	assert_eq!(run.active_input_layout(), Some("default"));
	tick(&mut run);
	assert_eq!(run.active_input_layout(), Some("default"));
	tick(&mut run);
	assert_eq!(run.active_input_layout(), Some("land"));

	let mut keyboard = engine_core::InputState::default();
	keyboard.press(Key::Space);
	run.sample_action_keys(&keyboard, Default::default())
		.unwrap();
	tick(&mut run);
	tick(&mut run);
	assert_eq!(run.active_input_layout(), Some("airborne"));

	let mut world = conditional_controls(|scene_id| {
		map.layouts.insert(
			0,
			ConditionalInputLayout {
				id: "world".into(),
				label: "World".into(),
				when: InputLayoutCondition::SceneIs {
					scene_id: scene_id.clone(),
				},
				overrides: vec![],
			},
		);
		map
	});
	tick(&mut world);
	assert_eq!(world.active_input_layout(), Some("world"));
	tick(&mut world);
	assert_eq!(world.active_input_layout(), Some("world"));
}

#[test]
fn conditional_remap_requires_fresh_keyboard_and_gamepad_presses() {
	use engine_core::{
		InputState,
		input::{
			ActionBinding, FaceButton, InputActionMap, InputBindingOverride, PhysicalGamepadSample,
		},
	};
	for keyboard in [true, false] {
		let mut map = InputActionMap::starter();
		map.schema_version = 2;
		map.layouts = motion_layouts();
		map.layouts[1].overrides = vec![
			InputBindingOverride {
				action_id: "jump".into(),
				bindings: vec![],
			},
			InputBindingOverride {
				action_id: "attack".into(),
				bindings: vec![
					ActionBinding::Key { key: Key::Space },
					ActionBinding::GamepadButton {
						button: FaceButton::A,
					},
				],
			},
		];
		let mut run = conditional_controls(|_| map);
		tick(&mut run);
		tick(&mut run);
		let sample = |run: &mut SimulationSession, held: bool| {
			let mut input = InputState::default();
			let mut pad = PhysicalGamepadSample::default();
			if keyboard && held {
				input.press(Key::Space);
			} else if held {
				pad.face_buttons[FaceButton::A as usize] = true;
			}
			run.sample_action_keys(&input, pad).unwrap();
		};
		sample(&mut run, true);
		let grouped = run.tick(2. / 60.).unwrap();
		assert_eq!(run.active_input_layout(), Some("airborne"));
		assert_eq!(
			grouped
				.accepted_action_presses
				.iter()
				.map(|event| event.id.as_str())
				.collect::<Vec<_>>(),
			vec!["jump"]
		);

		run.clear_input();
		sample(&mut run, true);
		assert!(
			run.tick(1. / 60.)
				.unwrap()
				.accepted_action_presses
				.is_empty()
		);
		sample(&mut run, false);
		sample(&mut run, true);
		sample(&mut run, false);
		let grouped = run.tick(2. / 60.).unwrap();
		assert_eq!(
			grouped
				.accepted_action_presses
				.iter()
				.map(|event| event.id.as_str())
				.collect::<Vec<_>>(),
			vec!["attack"]
		);
	}
}

fn run(editor: &mut EditorSession, command: Command) -> Result<(), String> {
	editor.execute_command(EditorCommandRequest {
		expected_revision: editor.revision(),
		command,
	})
}

fn player_editor() -> EditorSession {
	EditorSession::new(scene(vec![box_at(
		"floor",
		[0., -0.5, 0.],
		[10., 0.5, 10.],
	)]))
	.unwrap()
}

fn assigned(editor: &EditorSession) -> Option<engine_core::EntityKey> {
	editor
		.authored_document()
		.local_player()
		.map(|actor| actor.key.clone())
}

#[test]
fn create_assign_duplicate_and_persist_a_player_with_one_undo_each() {
	let mut editor = player_editor();
	assert!(editor.project().input_profile.is_none());
	run(&mut editor, Command::CreatePlayer).unwrap();
	let actor = editor.selection()[0].clone();
	let created = editor.authored_document();
	let entity = created.entities.last().unwrap();
	assert!(entity.parent.is_none() && entity.collider.is_some() && !entity.has_mesh_component());
	assert_eq!(
		entity
			.component_records()
			.iter()
			.find(|record| record.type_id() == PLAYER_CONTROLLER_TYPE)
			.unwrap()
			.id
			.as_str(),
		"player"
	);
	assert!(assigned(&editor).is_none());
	let jump = &editor.project().input_profile.as_ref().unwrap().actions[0];
	assert_eq!((&*jump.id, jump.key), ("jump", Key::Space));
	run(&mut editor, Command::Undo).unwrap();
	assert_eq!(editor.authored_document().entities.len(), 1);
	assert!(editor.project().input_profile.is_none());
	run(&mut editor, Command::Redo).unwrap();
	assert_eq!(editor.authored_document(), created);
	assert_eq!(editor.selection(), std::slice::from_ref(&actor));

	run(
		&mut editor,
		Command::SetLocalPlayer {
			key: Some(actor.clone()),
		},
	)
	.unwrap();
	run(
		&mut editor,
		Command::DuplicateSelection {
			offset: [4.0, 0.0, 0.0],
		},
	)
	.unwrap();
	let duplicate = editor.selection()[0].clone();
	assert_eq!(assigned(&editor), Some(actor.clone()));
	let persisted = editor.authored_document();
	assert_eq!(
		engine_core::SceneData::from_json(&persisted.to_json().unwrap()).unwrap(),
		*persisted
	);
	assert_eq!(
		serde_json::from_value::<engine_core::SceneData>(
			serde_json::to_value(persisted.as_ref()).unwrap()
		)
		.unwrap(),
		*persisted
	);
	run(
		&mut editor,
		Command::SetLocalPlayer {
			key: Some(duplicate.clone()),
		},
	)
	.unwrap();
	assert_eq!(assigned(&editor), Some(duplicate));
	run(&mut editor, Command::Undo).unwrap();
	assert_eq!(assigned(&editor), Some(actor.clone()));
	let primary = persisted.local_player().unwrap();
	let controller = &primary.player_controller.as_ref().unwrap().id;
	run(
		&mut editor,
		Command::SetComponentEnabled {
			key: actor.as_str().into(),
			component_id: controller.clone(),
			enabled: false,
		},
	)
	.unwrap();
	assert!(assigned(&editor).is_none());
	run(&mut editor, Command::Undo).unwrap();
	assert_eq!(assigned(&editor), Some(actor));
}

#[test]
fn assigned_input_moves_only_the_player_and_its_linked_visual_child() {
	let mut editor = player_editor();
	run(&mut editor, Command::CreatePlayer).unwrap();
	let player = editor.selection()[0].clone();
	run(
		&mut editor,
		Command::SetLocalPlayer {
			key: Some(player.clone()),
		},
	)
	.unwrap();
	run(
		&mut editor,
		Command::DuplicateSelection {
			offset: [4.0, 0.0, 0.0],
		},
	)
	.unwrap();
	let duplicate = editor.selection()[0].clone();
	let source = LinkedPrefabSource {
		source_id: AssetId::generate(),
		path: "props/player-visual.json".into(),
		fingerprint: "a".repeat(64),
		prefab: PrefabData {
			schema_version: 3,
			entities: vec![entity("visual", [0.; 3])],
			gameplay: None,
			reference_slots: Default::default(),
			composition: Default::default(),
		},
	};
	run(
		&mut editor,
		Command::InstantiateLinkedPrefab {
			source,
			position: [0.0, 1.0, 0.0],
			bindings: Default::default(),
		},
	)
	.unwrap();
	let anchor = editor.selection()[0].clone();
	let visual = editor
		.authored_document()
		.entities
		.iter()
		.find(|entity| entity.parent.as_ref() == Some(&anchor))
		.unwrap()
		.key
		.clone();
	run(
		&mut editor,
		Command::ReparentObjects {
			keys: vec![anchor],
			parent: Some(player.clone()),
			keep_world: true,
		},
	)
	.unwrap();
	let authored = editor.authored_document();
	run(&mut editor, Command::StartPlay).unwrap();
	editor
		.set_game_keyboard_input_from_codes(&HashSet::from([Key::D.physical_code()]))
		.unwrap();
	for _ in 0..25 {
		editor.tick(1. / 60.).unwrap();
	}
	let world = editor.render_world().unwrap();
	let position = |key| {
		world.world_matrices()[world.entity_index(key).unwrap()]
			.w_axis
			.x
	};
	assert!(position(&player) > 0.2);
	assert_eq!(position(&duplicate), 4.0);
	close(position(&visual), position(&player));
	run(&mut editor, Command::StopPlay).unwrap();
	assert_eq!(editor.authored_document(), authored);
}

#[test]
fn routing_rejects_parented_motorless_duplicate_and_conflicting_ownership() {
	let mut editor = player_editor();
	run(&mut editor, Command::CreatePlayer).unwrap();
	let actor = editor.selection()[0].clone();
	run(
		&mut editor,
		Command::SetLocalPlayer {
			key: Some(actor.clone()),
		},
	)
	.unwrap();
	let before = editor.authored_document();
	let motor = before
		.local_player()
		.unwrap()
		.character_controller
		.as_ref()
		.unwrap()
		.id
		.clone();
	assert!(
		run(
			&mut editor,
			Command::SetComponentEnabled {
				key: actor.as_str().into(),
				component_id: motor,
				enabled: false
			}
		)
		.unwrap_err()
		.contains("enabled Character Controller")
	);
	assert!(
		run(
			&mut editor,
			Command::ReparentObjects {
				keys: vec![actor.clone()],
				parent: Some(key("floor")),
				keep_world: true
			}
		)
		.unwrap_err()
		.contains("scene-root")
	);
	let mut bad_project = editor.project().clone();
	bad_project.add_behavior(BehaviorSpec::ControlledMove {
		entity: actor.clone(),
		speed: 1.0,
		space: BehaviorSpace::World,
	});
	assert!(
		run(
			&mut editor,
			Command::SetProject {
				project: bad_project
			}
		)
		.unwrap_err()
		.contains("owns")
	);
	assert_eq!(editor.authored_document(), before);

	let mut conflict = (*before).clone();
	let mut second = conflict.local_player().unwrap().clone();
	second.key = key("second");
	conflict.entities.push(second);
	assert!(conflict.validate().unwrap_err().contains("only one"));
	let mut linked = editor.export_prefab().unwrap();
	for entity in &mut linked.entities {
		assert!(!entity.is_local_player());
	}
	assert!(linked.validate_linked_player_components().is_err());
}
