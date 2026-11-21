use super::*;

#[test]
fn replay_switches_between_catalog_scenes_and_preserves_drafts_and_scene_tagged_history() {
	use engine_core::scene_catalog::{SceneDescriptor, SceneId};

	let fixture = Fixture::new();
	let main_source = fs::read(&fixture.scene).unwrap();
	let mut secondary = SceneData::from_json(std::str::from_utf8(&main_source).unwrap()).unwrap();
	secondary.entities.truncate(1);
	secondary.entities[0].name = "Secondary actor".into();
	secondary.entities[0].transform.position = [40.0, 0.0, 0.0];
	let secondary_path = fixture.root.join("assets/scenes/secondary.json");
	fs::write(&secondary_path, secondary.to_json().unwrap()).unwrap();
	let secondary_source = fs::read(&secondary_path).unwrap();
	let mut project = ProjectData::load(&fixture.project).unwrap();
	let main_id = project.startup_scene_id.clone();
	let secondary_id = SceneId::new("secondary").unwrap();
	project
		.scenes
		.insert(SceneDescriptor::new(secondary_id.clone(), "scenes/secondary.json").unwrap())
		.unwrap();
	fs::write(&fixture.project, project.to_json().unwrap()).unwrap();
	let manifest_source = fs::read(&fixture.project).unwrap();

	let actions = vec![
		json!({"type":"command","command":{"type":"rename","key":"actor","name":"Edited main"}}),
		json!({"type":"command","command":{"type":"switch_scene","scene_id":secondary_id.as_str()}}),
		json!({"type":"command","command":{"type":"rename","key":"actor","name":"Edited secondary"}}),
		json!({"type":"command","command":{"type":"switch_scene","scene_id":main_id.as_str()}}),
		json!({"type":"command","command":{"type":"undo"}}),
		json!({"type":"command","command":{"type":"undo"}}),
		json!({"type":"command","command":{"type":"redo"}}),
		json!({"type":"command","command":{"type":"redo"}}),
		json!({"type":"command","command":{"type":"switch_scene","scene_id":main_id.as_str()}}),
	];
	// Each prefix exposes an intermediate active document through the existing
	// state output. Global undo/redo activates the scene owning that record.
	for (checkpoint, command_count, expected_name, expected_position, expected_entities) in [
		(
			"secondary-undone",
			5,
			"Secondary actor",
			[40.0, 0.0, 0.0],
			1,
		),
		("main-undone", 6, "Actor", [0.0, 0.0, 0.0], 2),
		(
			"secondary-redone",
			8,
			"Edited secondary",
			[40.0, 0.0, 0.0],
			1,
		),
		("main-retained", 9, "Edited main", [0.0, 0.0, 0.0], 2),
	] {
		let replay = fixture.replay(actions[..command_count].to_vec());
		let state_path = fixture.root.join(format!("{checkpoint}.json"));
		let summary = success(
			fixture
				.command()
				.arg("--replay")
				.arg(&replay)
				.arg("--state-output")
				.arg(&state_path)
				.output()
				.unwrap(),
		);
		let final_scene = SceneData::from_json(&fs::read_to_string(&state_path).unwrap()).unwrap();
		assert_eq!(summary["commands"], command_count);
		assert_eq!(summary["mode"], "edit");
		assert_eq!(summary["fixed_steps"], 0);
		assert_eq!(summary["entities"], expected_entities);
		assert_eq!(final_scene.entities[0].name, expected_name);
		assert_eq!(
			final_scene.entities[0].transform.position,
			expected_position
		);
	}
	assert_eq!(fs::read(&fixture.scene).unwrap(), main_source);
	assert_eq!(fs::read(&secondary_path).unwrap(), secondary_source);
	assert_eq!(fs::read(&fixture.project).unwrap(), manifest_source);
}

#[test]
fn project_catalog_selects_startup_by_id_and_only_runs_that_scenes_behaviors() {
	use engine_core::{
		project::SceneBehavior,
		scene_catalog::{SceneDescriptor, SceneId},
	};

	let fixture = Fixture::new();
	let source_bytes = fs::read(&fixture.scene).unwrap();
	let mut secondary = SceneData::from_json(std::str::from_utf8(&source_bytes).unwrap()).unwrap();
	secondary.entities.truncate(1);
	secondary.entities[0].transform.position = [40.0, 0.0, 0.0];
	let secondary_path = fixture.root.join("assets/scenes/secondary.json");
	fs::write(&secondary_path, secondary.to_json().unwrap()).unwrap();
	let secondary_bytes = fs::read(&secondary_path).unwrap();
	let mut project = ProjectData::load(&fixture.project).unwrap();
	let secondary_id = SceneId::new("secondary").unwrap();
	project
		.scenes
		.insert(SceneDescriptor::new(secondary_id.clone(), "scenes/secondary.json").unwrap())
		.unwrap();
	project.behaviors.push(SceneBehavior {
		scene_id: secondary_id.clone(),
		behavior: BehaviorSpec::Spin {
			entity: EntityKey::new("actor").unwrap(),
			angular_velocity: [0.0, 2.0, 0.0],
		},
	});
	project.startup_scene_id = secondary_id;
	fs::write(&fixture.project, project.to_json().unwrap()).unwrap();
	let project_bytes = fs::read(&fixture.project).unwrap();
	let state_path = fixture.root.join("secondary-state.json");
	let summary = success(
		fixture
			.command()
			.arg("--steps")
			.arg("6")
			.arg("--state-output")
			.arg(&state_path)
			.output()
			.unwrap(),
	);
	let final_scene = SceneData::from_json(&fs::read_to_string(&state_path).unwrap()).unwrap();
	assert_eq!(summary["entities"], 1);
	assert_eq!(summary["fixed_steps"], 6);
	assert_eq!(final_scene.entities[0].transform.position, [40.0, 0.0, 0.0]);
	assert!((final_scene.entities[0].transform.rotation[1] - 0.2).abs() < 1e-5);
	assert!(
		final_scene.entities[0].transform.rotation[2].abs() < 1e-5,
		"inactive scene's spin ran against the overlapping entity key"
	);
	assert_eq!(fs::read(&fixture.scene).unwrap(), source_bytes);
	assert_eq!(fs::read(&secondary_path).unwrap(), secondary_bytes);
	assert_eq!(fs::read(&fixture.project).unwrap(), project_bytes);
}
