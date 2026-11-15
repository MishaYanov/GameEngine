mod support;
use engine_core::{
	BackendErrorCode, Engine, EngineBuilder, SceneData,
	command::{EditorCommandDto as Command, EditorCommandRequest},
	events::BackendEventKind,
	game_progress::{GameProgress, MAX_PROGRESS_ACTIONS, ProgressAction, ProgressState},
	game_session::GameSession,
	project::ProjectData,
	scene_catalog::{SceneDescriptor, SceneId},
	simulation_session::{
		ActionWriter, GameModule, GameModuleFactory, GameplayContext, InputFrame, SimulationAction,
	},
	world::RuntimeWorld,
};
use std::sync::Arc;
use support::*;

#[derive(Clone, PartialEq, Default)]
struct Score(i32);
impl GameProgress for Score {
	type Action = i32;
	fn validate(&self) -> Result<(), String> {
		if self.0 < 0 {
			Err("negative score".into())
		} else {
			Ok(())
		}
	}
	fn retained_bytes(&self) -> usize {
		size_of::<Self>()
	}
	fn action_bytes(_: &i32) -> usize {
		size_of::<i32>()
	}
	fn apply(&mut self, amount: &i32) -> Result<(), String> {
		self.0 += amount;
		Ok(())
	}
}
#[derive(Clone, PartialEq)]
struct Other;
impl GameProgress for Other {
	type Action = i32;
	fn validate(&self) -> Result<(), String> {
		Ok(())
	}
	fn retained_bytes(&self) -> usize {
		0
	}
	fn action_bytes(_: &i32) -> usize {
		0
	}
	fn apply(&mut self, _: &i32) -> Result<(), String> {
		Ok(())
	}
}
fn award(amount: i32) -> ProgressAction {
	ProgressAction::new::<Score>(amount).unwrap()
}

#[test]
fn progress_batches_validate_atomically_and_distinguish_state_types_with_the_same_action_type() {
	let mut state = ProgressState::new(Score(2)).unwrap();
	assert!(state.apply_batch(&[award(3), award(-10)]).is_err());
	assert_eq!(state.view().get::<Score>().unwrap().0, 2);
	assert_eq!(state.view().revision(), 0);
	assert!(
		state
			.apply_batch(&[award(3), ProgressAction::new::<Other>(1).unwrap()])
			.is_err()
	);
	assert_eq!(state.view().get::<Score>().unwrap().0, 2);
	let excessive: Vec<_> = (0..=MAX_PROGRESS_ACTIONS).map(|_| award(1)).collect();
	assert!(state.apply_batch(&excessive).is_err());
	assert_eq!(state.view().revision(), 0);
	assert!(!state.apply_batch(&[award(0)]).unwrap());
	assert!(state.apply_batch(&[award(3)]).unwrap());
	assert_eq!(state.view().get::<Score>().unwrap().0, 5);
	assert_eq!(state.view().revision(), 1);
}

struct Factory(bool);
impl GameModuleFactory for Factory {
	fn create_progress(&self) -> Result<ProgressState, String> {
		ProgressState::new(Score::default())
	}
	fn create(&self, _: &ProjectData, _: &SceneId) -> Result<Box<dyn GameModule>, String> {
		Ok(Box::new(Module(self.0)))
	}
}
struct Module(bool);
impl GameModule for Module {
	fn needs_tick(&self, _: &InputFrame) -> bool {
		self.0
	}
	fn plan(
		&self,
		_: &RuntimeWorld,
		_: &InputFrame,
		_: f32,
		_: &mut ActionWriter<'_>,
	) -> Result<(), String> {
		Ok(())
	}
	fn plan_with_progress(
		&self,
		_: GameplayContext<'_>,
		output: &mut ActionWriter<'_>,
	) -> Result<(), String> {
		output.push_progress::<Score>(3)?;
		output.push(SimulationAction::Move {
			entity: key("actor"),
			world_delta: [1.0, 0.0, 0.0],
		})?;
		output.push(SimulationAction::Move {
			entity: key("missing"),
			world_delta: [1.0, 0.0, 0.0],
		})
	}
}
fn session(reject: bool) -> (GameSession, SceneId, Arc<engine_core::SceneData>) {
	let document = Arc::new(scene(vec![entity("actor", [0.0; 3])]));
	let mut project = ProjectData::for_scene("Game", "scenes/main.json", &document).unwrap();
	let next = SceneId::new("next").unwrap();
	project
		.scenes
		.insert(SceneDescriptor::new(next.clone(), "scenes/next.json").unwrap())
		.unwrap();
	let startup = project.startup_scene_id.clone();
	(
		GameSession::with_factory_for_scene(
			Arc::new(project),
			startup,
			Arc::clone(&document),
			Arc::new(Factory(reject)),
		)
		.unwrap(),
		next,
		document,
	)
}

#[test]
fn rejected_world_actions_roll_back_progress_and_world_together_without_advancing_tick() {
	let (mut game, _, document) = session(true);
	assert!(game.tick(0.1).unwrap_err().contains("target is missing"));
	assert_eq!(game.progress::<Score>().unwrap().0, 0);
	assert_eq!(game.progress_revision(), 0);
	assert_eq!(game.active().document(), document);
	assert_eq!(game.active().tick_index(), 0);
	assert!(game.active().paused());
}

#[test]
fn scene_commit_keeps_latest_progress_while_new_game_resets_it_and_invalidates_old_tokens() {
	let (mut game, next, document) = session(false);
	let scene = game
		.prepare_scene(next.clone(), Arc::clone(&document))
		.unwrap();
	game.apply_progress_actions(&[award(7)]).unwrap();
	game.commit_scene(scene).unwrap();
	assert_eq!(game.progress::<Score>().unwrap().0, 7);
	assert_eq!(game.progress_revision(), 1);
	let old = game
		.prepare_scene(
			game.active().project().startup_scene_id.clone(),
			Arc::clone(&document),
		)
		.unwrap();
	let reset = game.prepare_new_game(next, document).unwrap();
	game.active_mut().set_paused(true);
	game.commit_scene(reset).unwrap();
	assert_eq!(game.progress::<Score>().unwrap().0, 0);
	assert_eq!(game.progress_revision(), 0);
	assert_eq!(game.scene_generation(), 0);
	assert!(!game.active().paused());
	assert!(
		game.commit_scene(old)
			.unwrap_err()
			.contains("another game session")
	);
}

struct SceneRequestFactory {
	destinations: Vec<SceneId>,
	reject_after_request: bool,
}

impl GameModuleFactory for SceneRequestFactory {
	fn create_progress(&self) -> Result<ProgressState, String> {
		ProgressState::new(Score::default())
	}

	fn create(
		&self,
		project: &ProjectData,
		scene_id: &SceneId,
	) -> Result<Box<dyn GameModule>, String> {
		Ok(Box::new(SceneRequestModule {
			destinations: if scene_id == &project.startup_scene_id {
				self.destinations.clone()
			} else {
				Vec::new()
			},
			reject_after_request: self.reject_after_request,
		}))
	}
}

struct SceneRequestModule {
	destinations: Vec<SceneId>,
	reject_after_request: bool,
}

impl GameModule for SceneRequestModule {
	fn needs_tick(&self, _: &InputFrame) -> bool {
		true
	}

	fn plan(
		&self,
		_: &RuntimeWorld,
		_: &InputFrame,
		_: f32,
		_: &mut ActionWriter<'_>,
	) -> Result<(), String> {
		Ok(())
	}

	fn plan_with_progress(
		&self,
		_: GameplayContext<'_>,
		output: &mut ActionWriter<'_>,
	) -> Result<(), String> {
		if self.destinations.is_empty() {
			return Ok(());
		}
		output.push_progress::<Score>(3)?;
		output.push(SimulationAction::Move {
			entity: key("actor"),
			world_delta: [1.0, 0.0, 0.0],
		})?;
		for scene_id in &self.destinations {
			output.push(SimulationAction::RequestScene {
				scene_id: scene_id.clone(),
			})?;
		}
		if self.reject_after_request {
			output.push(SimulationAction::Move {
				entity: key("missing"),
				world_delta: [1.0, 0.0, 0.0],
			})?;
		}
		Ok(())
	}
}

fn transition_project() -> (ProjectData, Arc<SceneData>, SceneId, SceneId) {
	let document = Arc::new(scene(vec![entity("actor", [0.0; 3])]));
	let mut project = ProjectData::for_scene("Game", "scenes/main.json", &document).unwrap();
	let next = SceneId::new("next").unwrap();
	let alternate = SceneId::new("alternate").unwrap();
	for (id, path) in [
		(&next, "scenes/next.json"),
		(&alternate, "scenes/alternate.json"),
	] {
		project
			.scenes
			.insert(SceneDescriptor::new(id.clone(), path).unwrap())
			.unwrap();
	}
	(project, document, next, alternate)
}

#[test]
fn scene_request_is_published_once_and_failed_preparation_leaves_the_source_playable() {
	let (project, document, next, _) = transition_project();
	let source = project.startup_scene_id.clone();
	let mut game = GameSession::with_factory_for_scene(
		Arc::new(project),
		source.clone(),
		Arc::clone(&document),
		Arc::new(SceneRequestFactory {
			destinations: vec![next.clone(), next.clone()],
			reject_after_request: false,
		}),
	)
	.unwrap();

	let outcome = game.tick(0.1).unwrap();
	assert_eq!(outcome.requested_scene.as_ref(), Some(&next));
	assert_eq!((outcome.tick, outcome.steps), (1, 1));
	assert!((outcome.dropped_seconds - 5.0 / 60.0).abs() < 1e-12);
	assert_eq!(game.active().scene_id(), &source);
	assert_eq!(
		game.active().world().data().entities[0].transform.position[0],
		1.0
	);
	assert_eq!(game.progress::<Score>().unwrap().0, 3);
	assert_eq!(game.pending_scene(), Some(&next));

	let mut invalid = document.as_ref().clone();
	invalid.camera.near_plane = -1.0;
	assert!(game.prepare_pending_scene(Arc::new(invalid)).is_err());
	assert_eq!(game.pending_scene(), Some(&next));
	assert_eq!(game.active().scene_id(), &source);
	assert!(game.needs_tick());

	// The module keeps requesting while the host is awaiting an explicit retry.
	let outcome = game.tick(3.0 / 60.0).unwrap();
	assert!(outcome.requested_scene.is_none());
	assert_eq!((outcome.tick, outcome.steps), (4, 3));
	assert_eq!(
		game.active().world().data().entities[0].transform.position[0],
		4.0
	);

	let candidate = game.prepare_pending_scene(Arc::clone(&document)).unwrap();
	let stale = game.prepare_pending_scene(Arc::clone(&document)).unwrap();
	game.commit_scene(candidate).unwrap();
	assert_eq!(game.active().scene_id(), &next);
	assert_eq!(game.active().tick_index(), 0);
	assert_eq!(game.progress::<Score>().unwrap().0, 12);
	assert!(game.pending_scene().is_none());
	assert!(game.commit_scene(stale).is_err());

	let restart = game
		.prepare_new_game(source.clone(), Arc::clone(&document))
		.unwrap();
	game.commit_scene(restart).unwrap();
	assert_eq!(game.progress::<Score>().unwrap().0, 0);
	assert!(game.pending_scene().is_none());

	assert_eq!(game.tick(1.0 / 60.0).unwrap().requested_scene, Some(next));
	let stale = game.prepare_pending_scene(Arc::clone(&document)).unwrap();
	let restart = game.prepare_new_game(source, document).unwrap();
	game.commit_scene(restart).unwrap();
	assert_eq!(game.progress::<Score>().unwrap().0, 0);
	assert!(game.pending_scene().is_none());
	assert!(game.commit_scene(stale).is_err());
}

#[test]
fn rejected_scene_requests_roll_back_world_and_progress_without_leaving_pending_work() {
	let (project, document, next, alternate) = transition_project();
	let source = project.startup_scene_id.clone();
	for (destinations, reject_after_request, reason) in [
		(vec![source.clone()], false, "current runtime scene"),
		(
			vec![SceneId::new("unknown").unwrap()],
			false,
			"not registered",
		),
		(
			vec![next.clone(), alternate],
			false,
			"Conflicting runtime scene requests",
		),
		(vec![next], true, "target is missing"),
	] {
		let mut game = GameSession::with_factory_for_scene(
			Arc::new(project.clone()),
			source.clone(),
			Arc::clone(&document),
			Arc::new(SceneRequestFactory {
				destinations,
				reject_after_request,
			}),
		)
		.unwrap();
		assert!(game.tick(0.1).unwrap_err().contains(reason));
		assert_eq!(game.active().document(), document);
		assert_eq!(game.active().tick_index(), 0);
		assert!(game.active().paused());
		assert_eq!(game.progress::<Score>().unwrap().0, 0);
		assert_eq!(game.progress_revision(), 0);
		assert!(game.pending_scene().is_none());
	}
}

fn execute(engine: &mut Engine, command: Command) {
	engine
		.execute(EditorCommandRequest {
			expected_revision: engine.session().revision(),
			command,
		})
		.unwrap();
}

#[test]
fn runtime_scene_commit_uses_the_retained_destination_and_preserves_authoring_state() {
	let (project, document, next, alternate) = transition_project();
	let source = project.startup_scene_id.clone();
	let mut engine = EngineBuilder::new(document.as_ref().clone(), project)
		.with_scenes(vec![
			(next.clone(), document.as_ref().clone()),
			(alternate, document.as_ref().clone()),
		])
		.module_factory(Arc::new(SceneRequestFactory {
			destinations: vec![next.clone()],
			reject_after_request: false,
		}))
		.build()
		.unwrap();
	execute(
		&mut engine,
		Command::SwitchScene {
			scene_id: next.clone(),
		},
	);
	execute(
		&mut engine,
		Command::Rename {
			key: "actor".into(),
			name: "Unsaved destination name".into(),
		},
	);
	execute(
		&mut engine,
		Command::SwitchScene {
			scene_id: source.clone(),
		},
	);
	execute(
		&mut engine,
		Command::Select {
			keys: vec!["actor".into()],
		},
	);
	execute(&mut engine, Command::StartPlay);
	let before = engine.snapshot().unwrap();
	let revision = before.revision;

	assert_eq!(
		engine.tick(1.0 / 60.0).unwrap().requested_scene.as_ref(),
		Some(&next)
	);
	assert_eq!(engine.pending_play_scene(), Some(&next));
	let (candidate, target_document) = engine.prepare_pending_play_scene().unwrap();
	assert_eq!(target_document.entities[0].name, "Unsaved destination name");
	let (stale, _) = engine.prepare_pending_play_scene().unwrap();
	let cursor = engine.change_cursor();
	let render_revision = engine.session().scene_revision();

	engine.commit_play_scene(candidate).unwrap();
	assert_eq!(engine.session().play_scene_id(), Some(&next));
	assert_eq!(engine.session().render_document(), target_document);
	assert!(engine.pending_play_scene().is_none());
	assert_eq!(engine.session().scene_revision(), render_revision + 1);
	assert_eq!(engine.session().play_progress::<Score>().unwrap().0, 3);
	let after = engine.snapshot().unwrap();
	assert_eq!(after.active_scene_id, source);
	assert_eq!(after.document, before.document);
	assert_eq!(after.project, before.project);
	assert_eq!(after.selection, before.selection);
	assert_eq!(after.dirty, before.dirty);
	assert_eq!(after.project_dirty, before.project_dirty);
	assert_eq!(after.can_undo, before.can_undo);
	assert_eq!(after.can_redo, before.can_redo);
	assert_eq!(after.revision, revision);

	let page = engine.changes_since(cursor).unwrap();
	assert_eq!(page.events.len(), 1);
	assert_eq!(page.events[0].scene_id.as_ref(), Some(&next));
	assert!(
		matches!(&page.events[0].event, BackendEventKind::RuntimeSceneChanged { from, to } if from == &source && to == &next)
	);
	let cursor = engine.change_cursor();
	assert_eq!(
		engine.commit_play_scene(stale).unwrap_err().code,
		BackendErrorCode::CommandRejected
	);
	assert_eq!(engine.change_cursor(), cursor);

	engine.tick(1.0 / 60.0).unwrap();
	let page = engine.changes_since(cursor).unwrap();
	assert!(matches!(
		page.events[0].event,
		BackendEventKind::SimulationAdvanced { tick: 1, .. }
	));
	assert_eq!(page.events[0].scene_id.as_ref(), Some(&next));

	engine
		.execute(EditorCommandRequest {
			expected_revision: revision,
			command: Command::StopPlay,
		})
		.unwrap();
	assert_eq!(engine.session().active_scene_id(), &source);
	assert_eq!(
		engine.session().render_document().as_ref(),
		&before.document
	);
	assert_eq!(
		engine.session().scene_document(&next).unwrap(),
		target_document
	);
}
