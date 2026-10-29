mod support;
use engine_core::{
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
