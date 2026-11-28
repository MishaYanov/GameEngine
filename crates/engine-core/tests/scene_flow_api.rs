mod support;

use engine_core::{
	SceneData,
	command::{EditorCommandDto, EditorCommandRequest},
	editor::EditorSession,
	game_session::GameSession,
	project::{
		BehaviorSpace, BehaviorSpec, ProjectData, SceneRoute, SceneRouteActivation, SceneRouteId,
	},
	scene_catalog::{SceneDescriptor, SceneId},
	scene_data::{ColliderData, ColliderInteraction},
	simulation_session::ConfiguredBehaviorFactory,
};
use serde_json::json;
use std::sync::Arc;
use support::{entity, key, scene};

struct Fixture {
	project: ProjectData,
	source: Arc<SceneData>,
	destination: Arc<SceneData>,
	next: SceneId,
	route: SceneRouteId,
}

impl Fixture {
	fn new(actor_x: f32) -> Self {
		let actor =
			entity("actor", [actor_x, 0.0, 0.0]).with_collider(Some(ColliderData::Sphere {
				radius: 0.25,
				offset: [0.0; 3],
				interaction: ColliderInteraction::default(),
			}));
		let sensor = entity("gate", [0.0; 3]).with_collider(Some(ColliderData::Box {
			half_extents: [0.5; 3],
			offset: [0.0; 3],
			interaction: ColliderInteraction {
				sensor: true,
				..Default::default()
			},
		}));
		let source = Arc::new(scene(vec![actor, sensor]));
		let destination = Arc::new(scene(vec![
			entity("actor", [10.0, 0.0, 0.0]),
			entity("platform", [100.0, 10.0, 0.0]),
			entity("entrance", [2.0, 0.5, 3.0]).with_parent(Some(key("platform"))),
		]));
		let first = SceneId::new("first").unwrap();
		let next = SceneId::new("next").unwrap();
		let route = SceneRouteId::new("go-to-arena").unwrap();
		let mut project =
			ProjectData::for_scene_with_id("Flow", first, "scenes/first.json", &source).unwrap();
		project.fixed_hz = 10;
		project.add_behavior(BehaviorSpec::ControlledMove {
			entity: key("actor"),
			speed: 4.0,
			space: BehaviorSpace::World,
		});
		project
			.scenes
			.insert(SceneDescriptor::new(next.clone(), "scenes/next.json").unwrap())
			.unwrap();
		project.scene_routes.push(
			serde_json::from_value::<SceneRoute>(json!({
				"id": route,
				"from": project.startup_scene_id,
				"to": next,
				"activate": {"kind": "enter_sensor", "sensor": "gate", "actor": "actor"},
				"preload": {"kind": "approach", "distance": 2.0, "release_distance": 3.0},
				"arrival": "entrance",
				"arrival_actor": "actor",
			}))
				.unwrap(),
		);
		Self {
			project,
			source,
			destination,
			next,
			route,
		}
	}

	fn play(&self) -> GameSession {
		GameSession::with_factory_for_scene(
			Arc::new(self.project.clone()),
			self.project.startup_scene_id.clone(),
			Arc::clone(&self.source),
			Arc::new(ConfiguredBehaviorFactory),
		)
			.unwrap()
	}
}

#[test]
fn scene_flow_validates_roundtrip_colliders_and_arrivals() {
	let fixture = Fixture::new(0.0);
	let manifest = fixture.project.to_json().unwrap();
	let project = ProjectData::from_json(&manifest).unwrap();
	assert_eq!(project, fixture.project);
	assert!(project.validate_scene(&fixture.source).is_ok());
	assert!(
		project
			.validate_scene_for(&fixture.next, &fixture.destination)
			.is_ok()
	);

	let mut source = fixture.source.as_ref().clone();
	source.entities[1].collider = None;
	assert!(
		project
			.validate_scene(&source)
			.unwrap_err()
			.contains("sensor collider")
	);

	let mut destination = fixture.destination.as_ref().clone();
	destination.entities.pop();
	assert!(
		project
			.validate_scene_for(&fixture.next, &destination)
			.unwrap_err()
			.contains("arrival marker")
	);
}

#[test]
fn a_sensor_exit_holds_on_the_exact_step_and_uses_the_world_arrival_marker() {
	let fixture = Fixture::new(0.0);
	let mut game = fixture.play();
	assert!(game.request_route(&fixture.route).is_err());
	let outcome = game.tick(0.4).unwrap();
	assert_eq!(outcome.steps, 1);
	assert_eq!(outcome.requested_scene.as_ref(), Some(&fixture.next));
	assert_eq!(outcome.requested_route.as_ref(), Some(&fixture.route));
	assert!(game.active().paused());
	assert!(!game.needs_tick());
	let attempt = game.pending_transition().unwrap().clone();
	let candidate = game
		.prepare_transition(&attempt, Arc::clone(&fixture.destination))
		.unwrap();
	assert_eq!(
		candidate.world_data().entities[0].transform.position,
		[102.0, 10.5, 3.0]
	);
	assert_eq!(
		fixture.destination.entities[0].transform.position,
		[10.0, 0.0, 0.0]
	);

	game.commit_scene(candidate).unwrap();
	assert_eq!(game.active().scene_id(), &fixture.next);
	assert_eq!(game.scene_generation(), 1);
	assert!(!game.active().paused());
	assert!(game.pending_transition().is_none());
	assert!(
		game.prepare_transition(&attempt, fixture.destination)
			.is_err()
	);
}

#[test]
fn retry_binds_candidates_to_the_exact_attempt_and_cancel_restores_play() {
	let fixture = Fixture::new(0.0);
	let mut game = fixture.play();
	game.tick(0.1).unwrap();
	let first = game.pending_transition().unwrap().clone();
	let stale = game
		.prepare_transition(&first, Arc::clone(&fixture.destination))
		.unwrap();

	game.fail_transition(&first).unwrap();
	assert!(!game.active().paused());
	assert!(game.pending_transition().is_none());
	assert_eq!(game.failed_transition().unwrap().id(), first.id());
	let retry = game.retry_transition(&first).unwrap();
	assert_ne!(retry.id(), first.id());
	assert!(
		game.commit_scene(stale)
			.unwrap_err()
			.contains("no longer active")
	);
	assert!(game.cancel_transition(&first).is_err());
	assert!(game.active().paused());

	game.cancel_transition(&retry).unwrap();
	assert!(!game.active().paused());
	assert!(game.failed_transition().is_none());
	assert!(game.pending_transition().is_none());
	assert_eq!(game.active().scene_id(), &fixture.project.startup_scene_id);
	assert!(game.retry_transition(&first).is_err());
}

#[test]
fn editor_retry_rejects_a_stale_run_or_attempt_at_the_current_revision() {
	let mut fixture = Fixture::new(0.0);
	fixture.project.scene_routes[0].activate = SceneRouteActivation::Manual;
	fixture.project.scene_routes[0].preload = None;
	let mut editor =
		EditorSession::for_project(fixture.source.as_ref().clone(), fixture.project).unwrap();
	let execute = |editor: &mut EditorSession, command| {
		editor.execute_command(EditorCommandRequest {
			expected_revision: editor.revision(),
			command,
		})
	};
	let fail_route = |editor: &mut EditorSession| {
		execute(
			editor,
			EditorCommandDto::RequestSceneRoute {
				route_id: fixture.route.clone(),
			},
		)
			.unwrap();
		let attempt = editor.pending_scene_transition().unwrap().clone();
		editor
			.finish_unsuccessful_scene_transition(&attempt, true)
			.unwrap();
		let status = editor.play_status().unwrap();
		assert_eq!(status.failed_scene_id.as_ref(), Some(&fixture.next));
		assert_eq!(status.failed_scene_attempt_id, Some(attempt.id()));
		status
	};
	let retry = |status: &engine_core::command::PlayStatusDto| EditorCommandDto::RetrySceneRoute {
		run_id: status.run_id.clone(),
		scene_generation: status.scene_generation,
		attempt_id: status.failed_scene_attempt_id.unwrap(),
	};

	execute(&mut editor, EditorCommandDto::StartPlay).unwrap();
	let first = fail_route(&mut editor);
	let second = fail_route(&mut editor);
	assert!(
		execute(&mut editor, retry(&first))
			.unwrap_err()
			.contains("no longer available")
	);
	assert_eq!(editor.play_status().unwrap(), second);

	execute(&mut editor, EditorCommandDto::StopPlay).unwrap();
	execute(&mut editor, EditorCommandDto::StartPlay).unwrap();
	let restarted = fail_route(&mut editor);
	assert_eq!(
		first.failed_scene_attempt_id,
		restarted.failed_scene_attempt_id
	);
	assert!(
		execute(&mut editor, retry(&first))
			.unwrap_err()
			.contains("Play changed")
	);
	assert_eq!(editor.play_status().unwrap(), restarted);

	execute(&mut editor, retry(&restarted)).unwrap();
	assert_eq!(editor.pending_play_scene(), Some(&fixture.next));
	assert!(
		editor
			.play_status()
			.unwrap()
			.failed_scene_attempt_id
			.is_none()
	);
}

#[test]
fn preload_threshold_and_release_threshold_are_independent() {
	let fixture = Fixture::new(3.0);
	let mut game = fixture.play();
	assert!(game.approach_route().is_none());
	assert!(game.should_retain_preload(&fixture.route));
	game.active_mut()
		.set_controlled_input([-1.0, 0.0, 0.0])
		.unwrap();
	let outcome = game.tick(0.2).unwrap();
	assert!(outcome.requested_scene.is_none());
	assert_eq!(
		game.approach_route().map(|route| &route.id),
		Some(&fixture.route)
	);
}
