use courtyard_gameplay::{CampaignProgress, resolve, settings::InteractionSettings};
use engine_core::{
	EngineBuilder, EntityKey, SceneData,
	command::{EditorCommandDto, EditorCommandRequest},
	game_session::GameSession,
	project::ProjectData,
	scene_catalog::{SceneDescriptor, SceneId},
};
use std::sync::Arc;

fn fixture() -> (ProjectData, Arc<SceneData>, Arc<SceneData>) {
	(
		ProjectData::from_json(include_str!("../../../apps/game/project.json")).unwrap(),
		Arc::new(
			SceneData::from_json(include_str!(
				"../../../apps/game/assets/scenes/courtyard.json"
			))
			.unwrap(),
		),
		Arc::new(
			SceneData::from_json(include_str!("../../../apps/game/assets/scenes/finish.json"))
				.unwrap(),
		),
	)
}
fn start(project: &ProjectData, data: Arc<SceneData>) -> GameSession {
	GameSession::with_factory_for_scene(
		Arc::new(project.clone()),
		project.startup_scene_id.clone(),
		data,
		resolve(project).unwrap(),
	)
	.unwrap()
}
fn drive(game: &mut GameSession, x: f32, steps: usize) {
	game.active_mut()
		.set_controlled_input([x, 0.0, 0.0])
		.unwrap();
	for _ in 0..steps {
		game.tick(1.0 / 60.0).unwrap();
		if game.pending_scene().is_some() || game.progress::<CampaignProgress>().unwrap().completed
		{
			break;
		}
	}
	game.active_mut().clear_input();
}
fn entity<'a>(data: &'a SceneData, key: &str) -> &'a engine_core::scene_data::EntityData {
	data.entities
		.iter()
		.find(|value| value.key.as_str() == key)
		.unwrap()
}

#[test]
fn bundled_authored_triggers_hide_pickup_unlock_door_follow_camera_complete_and_restore_on_restart()
{
	let (project, course, finish) = fixture();
	let mut game = start(&project, course.clone());
	let initial_camera = game.active().camera().position;
	drive(&mut game, -1.0, 120);
	assert!(game.pending_scene().is_none());
	assert!(
		!game
			.progress::<CampaignProgress>()
			.unwrap()
			.has_courtyard_key
	);
	assert!(entity(game.active().world().data(), "courtyard-exit").collision_enabled);
	drive(&mut game, 1.0, 240);
	assert_eq!(game.progress::<CampaignProgress>().unwrap().score, 1);
	let data = game.active().document();
	assert!(!entity(&data, "courtyard-key").renderable);
	assert!(!entity(&data, "courtyard-key-trigger").collision_enabled);
	assert!(!entity(&data, "courtyard-exit").collision_enabled);
	assert_eq!(
		entity(&data, "courtyard-exit")
			.material
			.as_ref()
			.unwrap()
			.base_color_factor,
		[0.1, 0.9, 0.25, 1.0]
	);
	assert_ne!(game.active().camera().position, initial_camera);
	drive(&mut game, -1.0, 360);
	assert_eq!(game.pending_scene().unwrap().as_str(), "finish");
	let prepared = game.prepare_pending_scene(finish.clone()).unwrap();
	game.commit_scene(prepared).unwrap();
	assert_eq!(game.active().camera().position, [0.0, 0.0, 5.0]);
	drive(&mut game, 1.0, 120);
	assert!(game.progress::<CampaignProgress>().unwrap().completed);
	let completed_position = entity(game.active().world().data(), "finish-player")
		.transform
		.position;
	for _ in 0..200 {
		game.tick(1.0 / 60.0).unwrap();
	}
	assert!(!game.needs_tick());
	assert_eq!(
		entity(game.active().world().data(), "finish-player")
			.transform
			.position,
		completed_position
	);
	let restart = game
		.prepare_new_game(project.startup_scene_id.clone(), course.clone())
		.unwrap();
	game.commit_scene(restart).unwrap();
	assert_eq!(game.active().document(), course);
	assert_eq!(game.active().camera().position, initial_camera);
	assert_eq!(game.progress::<CampaignProgress>().unwrap().score, 0);
	assert!(entity(game.active().world().data(), "courtyard-key").renderable);
	assert!(entity(game.active().world().data(), "courtyard-exit").collision_enabled);
}

#[test]
fn edited_volume_and_custom_destination_survive_project_roundtrip_and_are_used_by_editor_play() {
	let (mut project, mut course, finish) = fixture();
	let bonus = SceneId::new("bonus-level").unwrap();
	project
		.scenes
		.insert(SceneDescriptor::new(bonus.clone(), "scenes/bonus.json").unwrap())
		.unwrap();
	let mut movement = project
		.behaviors
		.iter()
		.find(|value| value.scene_id.as_str() == "finish")
		.unwrap()
		.clone();
	movement.scene_id = bonus.clone();
	project.behaviors.push(movement);
	let settings = project.gameplay_settings.as_mut().unwrap();
	settings["scenes"]["courtyard"]["destination_scene"] = serde_json::json!("bonus-level");
	settings["scenes"]["bonus-level"] =
		serde_json::json!({"kind":"finish","finish_trigger":"finish-trigger"});
	// Move just the pickup sensor; the visible key stays where it was authored.
	Arc::make_mut(&mut course)
		.entities
		.iter_mut()
		.find(|value| value.key.as_str() == "courtyard-key-trigger")
		.unwrap()
		.transform
		.position = [0.0; 3];
	let mut engine = EngineBuilder::new((*course).clone(), project.clone())
		.with_scenes(vec![(bonus.clone(), (*finish).clone())])
		.module_resolver(Arc::new(resolve))
		.build()
		.unwrap();
	let mut edited = project.clone();
	edited.name = "Authored interaction roundtrip".into();
	engine
		.execute(EditorCommandRequest {
			expected_revision: engine.session().revision(),
			command: EditorCommandDto::SetProject {
				project: edited.clone(),
			},
		})
		.unwrap();
	let reopened =
		ProjectData::from_json(&serde_json::to_string(engine.session().project()).unwrap())
			.unwrap();
	assert_eq!(reopened, edited);
	let reopened_scene =
		Arc::new(SceneData::from_json(&serde_json::to_string(course.as_ref()).unwrap()).unwrap());
	let mut game = start(&reopened, reopened_scene);
	game.tick(1.0 / 60.0).unwrap();
	assert!(
		game.progress::<CampaignProgress>()
			.unwrap()
			.has_courtyard_key
	);
	drive(&mut game, -1.0, 120);
	assert_eq!(game.pending_scene(), Some(&bonus));
	game.commit_scene(game.prepare_pending_scene(finish).unwrap())
		.unwrap();
	drive(&mut game, 1.0, 120);
	assert!(game.progress::<CampaignProgress>().unwrap().completed);
	engine
		.execute(EditorCommandRequest {
			expected_revision: engine.session().revision(),
			command: EditorCommandDto::StartPlay,
		})
		.unwrap();
	engine.tick(1.0 / 60.0).unwrap();
	assert_eq!(
		engine.play_status().unwrap().gameplay.unwrap().score,
		Some(1)
	);
	engine
		.execute(EditorCommandRequest {
			expected_revision: engine.session().revision(),
			command: EditorCommandDto::StopPlay,
		})
		.unwrap();
	assert_eq!(
		engine.session().authored_document().as_ref(),
		course.as_ref()
	);
}

#[test]
fn missing_sensors_filtered_player_invalid_destinations_and_unknown_settings_fail_preparation() {
	let (project, course, _) = fixture();
	for failure in ["missing", "solid", "filtered", "disabled"] {
		let mut data = (*course).clone();
		let sensor = data
			.entities
			.iter_mut()
			.find(|value| value.key.as_str() == "courtyard-key-trigger")
			.unwrap();
		match failure {
			"missing" => sensor.key = EntityKey::new("renamed").unwrap(),
			"solid" => {
				if let Some(engine_core::scene_data::ColliderData::Sphere { interaction, .. }) =
					sensor.collider.as_mut()
				{
					interaction.sensor = false;
				}
			}
			"filtered" => {
				if let Some(engine_core::scene_data::ColliderData::Sphere { interaction, .. }) =
					sensor.collider.as_mut()
				{
					interaction.mask = 0;
				}
			}
			_ => sensor.collision_enabled = false,
		}
		assert!(
			GameSession::with_factory_for_scene(
				Arc::new(project.clone()),
				project.startup_scene_id.clone(),
				Arc::new(data),
				resolve(&project).unwrap()
			)
			.is_err(),
			"{failure}"
		);
	}
	for invalid in [
		serde_json::json!({"schema_version":2,"scenes":{}}),
		serde_json::json!({"schema_version":1,"scenes":{},"typo":true}),
	] {
		let mut invalid_project = project.clone();
		invalid_project.gameplay_settings = Some(invalid);
		assert!(resolve(&invalid_project).is_err());
	}
	let mut invalid = project.clone();
	invalid.gameplay_settings.as_mut().unwrap()["scenes"]["courtyard"]["destination_scene"] =
		serde_json::json!("missing-scene");
	assert!(
		InteractionSettings::from_project(&invalid)
			.unwrap_err()
			.contains("destination")
	);
}

#[test]
fn already_overlapping_locked_exit_can_unlock_without_losing_the_edge() {
	let (project, mut course, _) = fixture();
	for value in &mut Arc::make_mut(&mut course).entities {
		if value.key.as_str().ends_with("-trigger") {
			value.transform.position = [0.0; 3];
		}
	}
	let mut game = start(&project, course);
	let outcome = game.tick(1.0 / 60.0).unwrap();
	assert_eq!(outcome.trigger_events.len(), 2);
	assert_eq!(game.pending_scene().unwrap().as_str(), "finish");
	assert_eq!(game.progress::<CampaignProgress>().unwrap().score, 1);
}

#[test]
fn gameplay_prefab_copies_remap_real_courtyard_settings_independently_and_undo_together() {
	let (project, course, finish) = fixture();
	let mut engine = EngineBuilder::new((*course).clone(), project)
		.with_scenes(vec![(SceneId::new("finish").unwrap(), (*finish).clone())])
		.module_resolver(Arc::new(resolve))
		.build()
		.unwrap();
	let command = |engine: &mut engine_core::Engine, command| {
		engine
			.execute(EditorCommandRequest {
				expected_revision: engine.session().revision(),
				command,
			})
			.unwrap()
	};
	command(
		&mut engine,
		EditorCommandDto::Select {
			keys: course
				.entities
				.iter()
				.map(|entity| entity.key.as_str().into())
				.collect(),
		},
	);
	let prefab = engine.export_prefab().unwrap();
	assert_eq!(prefab.schema_version, 3);
	let prefab = engine_core::editor::PrefabData::from_json(&prefab.to_json().unwrap()).unwrap();
	assert_eq!(prefab.gameplay.as_ref().unwrap().entity_references.len(), 3);
	let mut empty = (*course).clone();
	empty.entities.clear();
	empty.bindings.clear();
	empty.camera.follow = None;
	let mut copy_keys = Vec::new();
	for id in ["copy-a", "copy-b"] {
		let id = SceneId::new(id).unwrap();
		command(
			&mut engine,
			EditorCommandDto::AddScene {
				id: id.clone(),
				asset_path: format!("scenes/{}.json", id.as_str()),
				document: empty.clone(),
			},
		);
		command(
			&mut engine,
			EditorCommandDto::SwitchScene {
				scene_id: id.clone(),
			},
		);
		engine.instantiate_prefab(prefab.clone(), [0.0; 3]).unwrap();
		let data = engine.session().authored_document();
		let player = data.bindings["player"].clone();
		assert_ne!(&player, &course.bindings["player"]);
		copy_keys.push(player.clone());
		let settings = &engine
			.session()
			.project()
			.gameplay_settings
			.as_ref()
			.unwrap()["scenes"][id.as_str()];
		assert_ne!(settings["key_trigger"], "courtyard-key-trigger");
		assert!(
			data.entities
				.iter()
				.any(|entity| entity.key.as_str() == settings["key_trigger"].as_str().unwrap())
		);
		assert_eq!(settings["destination_scene"], "finish");
		assert!(
			engine
				.session()
				.project()
				.behaviors_for(&id)
				.any(|behavior| behavior.entity() == &player)
		);
		command(&mut engine, EditorCommandDto::StartPlay);
		command(
			&mut engine,
			EditorCommandDto::SetPlayInput {
				movement: [1.0, 0.0, 0.0],
			},
		);
		for _ in 0..120 {
			engine.tick(1.0 / 60.0).unwrap();
		}
		assert_eq!(
			engine.play_status().unwrap().gameplay.unwrap().score,
			Some(1)
		);
		command(&mut engine, EditorCommandDto::StopPlay);
		assert_eq!(engine.session().authored_document(), data);
		assert!(
			engine
				.instantiate_prefab(prefab.clone(), [5.0, 0.0, 0.0])
				.unwrap_err()
				.message
				.contains("already bound")
		);
		if id.as_str() == "copy-b" {
			command(&mut engine, EditorCommandDto::Undo);
			assert!(engine.session().authored_document().entities.is_empty());
			assert!(
				engine
					.session()
					.project()
					.behaviors_for(&id)
					.next()
					.is_none()
			);
			assert!(
				engine
					.session()
					.project()
					.gameplay_settings
					.as_ref()
					.unwrap()["scenes"]
					.get(id.as_str())
					.is_none()
			);
			command(&mut engine, EditorCommandDto::Redo);
			assert_eq!(engine.session().authored_document(), data);
		}
	}
	assert_ne!(copy_keys[0], copy_keys[1]);
}

#[test]
fn module_prefab_reference_schema_never_treats_kind_text_as_an_entity_identity() {
	use engine_core::simulation_session::GameModuleFactory;
	let settings = serde_json::json!({"kind":"course","key_trigger":"key-sensor","exit_trigger":"exit-sensor","destination_scene":"finish"});
	let references = courtyard_gameplay::CampaignFactory
		.prefab_references(&settings)
		.unwrap();
	assert_eq!(
		references
			.entities
			.keys()
			.map(String::as_str)
			.collect::<Vec<_>>(),
		["/exit_trigger", "/key_trigger"]
	);
	assert!(!references.entities.contains_key("/kind"));
	assert_eq!(references.scenes["/destination_scene"].as_str(), "finish");
}
