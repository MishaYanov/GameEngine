mod support;

use engine_core::{
	project::{ProjectData, RegionBounds, RegionNavigationStep, WorldRegion},
	scene_catalog::{SceneDescriptor, SceneId},
};
use support::scene;

fn id(value: &str) -> SceneId {
	SceneId::new(value).unwrap()
}

fn add(project: &mut ProjectData, name: &str, min: [f32; 3], max: [f32; 3]) {
	let scene = id(name);
	project
		.scenes
		.insert(SceneDescriptor::new(scene.clone(), format!("scenes/{name}.json")).unwrap())
		.unwrap();
	project.world_regions.push(WorldRegion {
		world: project.startup_scene_id.clone(),
		scene,
		bounds: RegionBounds { min, max },
		preload_distance: 1.0,
		release_distance: 2.0,
	});
}

fn project() -> ProjectData {
	let mut project =
		ProjectData::for_scene("World", "scenes/world.json", &scene(Vec::new())).unwrap();
	add(&mut project, "goal", [10.0, -1.0, 10.0], [20.0, 2.0, 20.0]);
	add(&mut project, "north", [0.0, -1.0, 10.0], [10.0, 2.0, 20.0]);
	add(&mut project, "home", [0.0, -1.0, 0.0], [10.0, 2.0, 10.0]);
	add(&mut project, "east", [10.0, -1.0, 0.0], [20.0, 2.0, 10.0]);
	project
}

#[test]
fn an_actor_waits_then_takes_the_same_route_independent_of_manifest_order() {
	let mut project = project();
	for _ in 0..2 {
		let navigator = project.region_navigator(&project.startup_scene_id).unwrap();
		let home = navigator.region_at([5.0, 0.0, 5.0]).unwrap();
		let goal = navigator.region_at([15.0, 0.0, 15.0]).unwrap();
		let route = navigator
			.plan(home, goal, &[id("home"), id("north")])
			.unwrap()
			.unwrap();
		assert_eq!(route.regions, vec![id("home"), id("east"), id("goal")]);
		assert_eq!(route.first_unloaded, Some(id("east")));
		assert_eq!(
			route.step,
			RegionNavigationStep::WaitForRegion { region: id("east") }
		);

		let route = navigator
			.plan(home, goal, &[id("home"), id("east")])
			.unwrap()
			.unwrap();
		assert_eq!(route.first_unloaded, Some(id("goal")));
		assert_eq!(
			route.step,
			RegionNavigationStep::Cross {
				region: id("east"),
				portal: [10.0, 0.5, 5.0]
			}
		);
		let east = navigator.region_at([10.0, 0.0, 5.0]).unwrap();
		let route = navigator
			.plan(east, goal, &[id("east"), id("goal")])
			.unwrap()
			.unwrap();
		assert_eq!(
			route.step,
			RegionNavigationStep::Cross {
				region: id("goal"),
				portal: [15.0, 0.5, 10.0]
			}
		);
		assert_eq!(
			navigator
				.plan(goal, goal, &[id("goal")])
				.unwrap()
				.unwrap()
				.step,
			RegionNavigationStep::Arrived
		);
		project.world_regions.reverse();
	}
}

#[test]
fn corners_vertical_neighbors_and_gaps_do_not_open_an_implicit_portal() {
	let mut project = project();
	add(
		&mut project,
		"corner",
		[20.0, -1.0, 20.0],
		[30.0, 2.0, 30.0],
	);
	add(&mut project, "overhead", [0.0, 2.0, 0.0], [10.0, 4.0, 10.0]);
	add(&mut project, "gap", [-10.1, -1.0, 0.0], [-0.1, 2.0, 10.0]);
	let navigator = project.region_navigator(&project.startup_scene_id).unwrap();
	for destination in ["corner", "overhead", "gap"] {
		assert!(
			navigator
				.plan(&id("home"), &id(destination), &[id("home")])
				.unwrap()
				.is_none()
		);
	}
	add(&mut project, "overlaps", [0.5, -1.0, 0.0], [1.5, 2.0, 10.0]);
	assert!(project.region_navigator(&project.startup_scene_id).is_err());
}
