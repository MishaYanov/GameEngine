mod support;
use engine_core::{
	Engine, EngineBuilder,
	command::{EditorCommandDto as Command, EditorCommandRequest},
	project::{BehaviorSpec, ProjectData, SceneBehavior},
	scene_catalog::{SceneDescriptor, SceneId},
};
use support::*;

fn fixture() -> (Engine, SceneId, SceneId) {
	let first = scene(vec![entity("actor", [0.0; 3])]);
	let second = scene(vec![entity("actor", [10.0, 0.0, 0.0])]);
	let mut project = ProjectData::for_scene("Two scenes", "scenes/first.json", &first).unwrap();
	let first_id = project.startup_scene_id.clone();
	let second_id = SceneId::new("second").unwrap();
	project
		.scenes
		.insert(SceneDescriptor::new(second_id.clone(), "scenes/second.json").unwrap())
		.unwrap();
	for (scene_id, speed) in [(&first_id, 1.0), (&second_id, 5.0)] {
		project.behaviors.push(SceneBehavior {
			scene_id: scene_id.clone(),
			behavior: BehaviorSpec::ControlledMove {
				entity: key("actor"),
				speed,
				space: Default::default(),
			},
		});
	}
	let engine = EngineBuilder::new(first, project)
		.with_scenes(vec![(second_id.clone(), second)])
		.build()
		.unwrap();
	(engine, first_id, second_id)
}
fn run(engine: &mut Engine, command: Command) {
	engine
		.execute(EditorCommandRequest {
			expected_revision: engine.session().revision(),
			command,
		})
		.unwrap();
}
fn switch(engine: &mut Engine, id: &SceneId) {
	run(
		engine,
		Command::SwitchScene {
			scene_id: id.clone(),
		},
	);
}
fn select(engine: &mut Engine, keys: &[&str]) {
	run(
		engine,
		Command::Select {
			keys: keys.iter().map(|value| (*value).into()).collect(),
		},
	);
}
fn rejected_without_mutation(engine: &mut Engine, command: Command) {
	let before = serde_json::to_value(engine.snapshot().unwrap()).unwrap();
	let cursor = engine.change_cursor();
	assert!(
		engine
			.execute(EditorCommandRequest {
				expected_revision: engine.session().revision(),
				command
			})
			.is_err()
	);
	assert_eq!(
		serde_json::to_value(engine.snapshot().unwrap()).unwrap(),
		before
	);
	assert_eq!(engine.change_cursor(), cursor);
}

#[path = "multi_scene_api/navigation.rs"]
mod navigation;

#[path = "multi_scene_api/baselines.rs"]
mod baselines;

#[path = "multi_scene_api/history.rs"]
mod history;

#[path = "multi_scene_api/play.rs"]
mod play;
