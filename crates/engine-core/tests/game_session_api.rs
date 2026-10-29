mod support;
use engine_core::{
	game_session::GameSession,
	project::{
		BehaviorDefinition, BehaviorSettings, BehaviorSpace, BehaviorSpec, ProjectData,
		SceneBehavior,
	},
	scene_catalog::{SceneDescriptor, SceneId},
	simulation_session::ConfiguredBehaviorFactory,
};
use std::sync::Arc;
use support::*;

#[test]
fn prepared_scenes_reject_wrong_owner_and_stale_commits_and_preserve_current_pause_state() {
	let document = Arc::new(scene(vec![entity("actor", [0.0; 3])]));
	let mut project = ProjectData::for_scene("Game", "scenes/main.json", &document).unwrap();
	let next = SceneId::new("next").unwrap();
	project
		.scenes
		.insert(SceneDescriptor::new(next.clone(), "scenes/next.json").unwrap())
		.unwrap();
	project.behavior_definitions.push(BehaviorDefinition {
		id: "walk".into(),
		settings: BehaviorSettings::ControlledMove {
			speed: 2.0,
			space: BehaviorSpace::World,
		},
	});
	project.behaviors.push(SceneBehavior {
		scene_id: next.clone(),
		behavior: BehaviorSpec::Shared {
			entity: key("actor"),
			definition: "walk".into(),
		},
	});
	let project = Arc::new(project);
	let create = || {
		GameSession::with_factory_for_scene(
			Arc::clone(&project),
			project.startup_scene_id.clone(),
			Arc::clone(&document),
			Arc::new(ConfiguredBehaviorFactory),
		)
		.unwrap()
	};
	let mut game = create();
	let mut other = create();
	assert!(std::ptr::eq(game.active().project(), project.as_ref()));
	assert!(std::ptr::eq(other.active().project(), project.as_ref()));
	let foreign = game
		.prepare_scene(next.clone(), Arc::clone(&document))
		.unwrap();
	assert!(
		other
			.commit_scene(foreign)
			.unwrap_err()
			.contains("another game session")
	);
	assert_eq!(other.scene_generation(), 0);
	let first = game
		.prepare_scene(next.clone(), Arc::clone(&document))
		.unwrap();
	let stale = game
		.prepare_scene(next.clone(), Arc::clone(&document))
		.unwrap();
	game.active_mut().set_paused(true);
	game.commit_scene(first).unwrap();
	assert!(std::ptr::eq(game.active().project(), project.as_ref()));
	assert_eq!(game.active().scene_id(), &next);
	assert!(game.active().paused());
	assert_eq!(game.scene_generation(), 1);
	assert!(
		game.commit_scene(stale)
			.unwrap_err()
			.contains("out of sync")
	);
	assert_eq!(game.scene_generation(), 1);
	game.active_mut().set_paused(false);
	assert!(!game.active().needs_tick());
	game.active_mut()
		.set_controlled_input([1.0, 0.0, 0.0])
		.unwrap();
	game.active_mut().tick(0.1).unwrap();
	close(
		game.active().document().entities[0].transform.position[0],
		0.2,
	);
}
