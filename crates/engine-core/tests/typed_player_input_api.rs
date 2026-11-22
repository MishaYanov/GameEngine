mod support;
use engine_core::{
	EngineBuilder,
	asset_catalog::AssetId,
	components::{ComponentData, InputActionsRef},
	input::{ActionBinding, InputActionMap, PhysicalGamepadSample},
	scene_data::ColliderData,
};
use engine_core::{
	Key,
	command::{EditorCommandDto as Command, EditorCommandRequest},
	input::GamepadSample,
	project::ProjectData,
};
use std::{collections::HashSet, sync::Arc};
use support::{entity, scene};

const STEP: f64 = 1.0 / 60.0;

#[test]
fn typed_action_samples_survive_short_taps_and_use_the_committed_camera_basis() {
	let mut floor = entity("floor", [0.0, -0.5, 0.0]);
	floor.collider = Some(ColliderData::Box {
		half_extents: [20.0, 0.5, 20.0],
		offset: [0.0; 3],
		interaction: Default::default(),
	});
	let mut document = scene(vec![floor]);
	document.camera.yaw = std::f32::consts::FRAC_PI_2;
	let project = ProjectData::for_scene("Input Test", "scenes/main.json", &document).unwrap();
	let mut map = InputActionMap::starter();
	map.actions[2].bindings[0] = ActionBinding::Key { key: Key::J };
	let mut engine = EngineBuilder::new(document, project)
		.input_actions_resolver(Arc::new(move |_| Ok(Some(map.clone()))))
		.build()
		.unwrap();
	let run = |engine: &mut engine_core::Engine, command| {
		engine
			.execute(EditorCommandRequest {
				expected_revision: engine.session().revision(),
				command,
			})
			.unwrap();
	};
	run(&mut engine, Command::CreatePlayer);
	let actor = engine
		.session()
		.authored_document()
		.entities
		.last()
		.unwrap()
		.key
		.clone();
	let mut component = engine
		.session()
		.authored_document()
		.entities
		.last()
		.unwrap()
		.component_records()
		.into_iter()
		.find(|record| matches!(&record.data, ComponentData::PlayerController { .. }))
		.unwrap();
	let ComponentData::PlayerController { settings } = &mut component.data else {
		unreachable!()
	};
	settings.input = InputActionsRef::Asset {
		asset_id: AssetId::generate(),
	};
	run(
		&mut engine,
		Command::ReplaceComponent {
			key: actor.as_str().into(),
			component,
		},
	);
	run(
		&mut engine,
		Command::SetLocalPlayer {
			key: Some(actor.clone()),
		},
	);
	run(&mut engine, Command::StartPlay);
	for _ in 0..4 {
		engine.tick(STEP).unwrap();
	}
	let position = |engine: &engine_core::Engine| {
		engine
			.session()
			.render_document()
			.entities
			.iter()
			.find(|entity| entity.key == actor)
			.unwrap()
			.transform
			.position
	};
	let origin = position(&engine);
	let keys = HashSet::from(["KeyW".to_owned(), "KeyJ".to_owned()]);
	engine
		.set_action_controls_from_codes(
			&keys,
			PhysicalGamepadSample::default(),
			GamepadSample::default(),
		)
		.unwrap();
	engine
		.set_action_controls_from_codes(
			&HashSet::from(["KeyW".to_owned()]),
			Default::default(),
			Default::default(),
		)
		.unwrap();
	let accepted = engine.tick(STEP * 4.0).unwrap();
	assert_eq!(accepted.steps, 4);
	assert_eq!(accepted.accepted_action_presses.len(), 1);
	assert_eq!(accepted.accepted_action_presses[0].id, "jump");
	let moved = position(&engine);
	assert!(moved[0] > origin[0] && (moved[2] - origin[2]).abs() < 1e-4);
	assert!(moved[1] > origin[1]);
	assert!(
		engine
			.tick(STEP * 2.0)
			.unwrap()
			.accepted_action_presses
			.is_empty()
	);

	run(&mut engine, Command::SetPaused { paused: true });
	run(&mut engine, Command::SetPaused { paused: false });
	assert!(
		engine
			.tick(STEP)
			.unwrap()
			.accepted_action_presses
			.is_empty()
	);
	run(&mut engine, Command::StopPlay);
	assert_eq!(
		engine
			.session()
			.authored_document()
			.entities
			.last()
			.unwrap()
			.transform
			.position,
		[0.0, 0.9, 0.0]
	);
}
