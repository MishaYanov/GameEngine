//! Gameplay for projects that opt in to the Courtyard Run module.

use engine_core::{
	EntityKey,
	command::{GameplayModuleDescriptorDto, GameplayRoleDescriptorDto, GameplayStatus},
	game_progress::{GameProgress, ProgressState, ProgressView},
	project::{BehaviorSettings, ProjectData},
	scene_catalog::SceneId,
	simulation_session::{
		ActionWriter, ConfiguredBehaviorFactory, GameModule, GameModuleFactory, GameplayContext,
		InputFrame, PreparedModuleConfig, SimulationAction, resolve_default_gameplay,
	},
	world::RuntimeWorld,
};
use std::{collections::BTreeMap, mem::size_of, sync::Arc};

pub const COURTYARD_RUN_ID: &str = "courtyard_run";
const FINISH_SCENE_ID: &str = "finish";

pub fn descriptor() -> GameplayModuleDescriptorDto {
	GameplayModuleDescriptorDto {
		id: COURTYARD_RUN_ID.into(),
		label: "Courtyard Run".into(),
		description: "Collect a key, reach the exit, then complete the finish scene.".into(),
		default_roles: vec![
			GameplayRoleDescriptorDto {
				name: "player".into(),
				label: "Player".into(),
				requirement_hint: "Required when Key or Exit is assigned. Needs controlled movement with positive speed.".into(),
			},
			GameplayRoleDescriptorDto {
				name: "key".into(),
				label: "Key".into(),
				requirement_hint: "Assigning Key or Exit requires Player, Key, and Exit on different entities.".into(),
			},
			GameplayRoleDescriptorDto {
				name: "exit".into(),
				label: "Exit".into(),
				requirement_hint: "Assigning Key or Exit requires all three roles and a scene with ID finish in the project.".into(),
			},
		],
		scene_roles: BTreeMap::from([(
			FINISH_SCENE_ID.into(),
			vec![
				GameplayRoleDescriptorDto {
					name: "player".into(),
					label: "Player".into(),
					requirement_hint: "Required in the finish scene. Needs controlled movement with positive speed.".into(),
				},
				GameplayRoleDescriptorDto {
					name: "finish".into(),
					label: "Finish".into(),
					requirement_hint: "Required in the finish scene and must reference a different entity from Player.".into(),
				},
			],
		)]),
	}
}

pub fn resolve(project: &ProjectData) -> Result<Arc<dyn GameModuleFactory>, String> {
	if project.gameplay_module.as_deref() == Some(COURTYARD_RUN_ID) {
		return Ok(Arc::new(CampaignFactory));
	}
	resolve_default_gameplay(project)
}

#[derive(Clone, Default, PartialEq, Eq)]
pub struct CampaignProgress {
	pub score: i32,
	pub has_courtyard_key: bool,
	pub courtyard_exit_reached: bool,
	pub completed: bool,
}

pub enum ProgressAction {
	AwardScore { amount: i32 },
	CollectCourtyardKey,
	ReachCourtyardExit,
	CompleteRun,
}

impl GameProgress for CampaignProgress {
	type Action = ProgressAction;
	fn validate(&self) -> Result<(), String> {
		if self.score < 0 {
			return Err("Campaign score must be nonnegative".into());
		}
		if self.courtyard_exit_reached && !self.has_courtyard_key {
			return Err("The courtyard exit cannot be reached without its key".into());
		}
		if self.completed && !self.courtyard_exit_reached {
			return Err("The run cannot be completed before reaching the courtyard exit".into());
		}
		Ok(())
	}
	fn retained_bytes(&self) -> usize {
		size_of::<Self>()
	}
	fn action_bytes(_: &ProgressAction) -> usize {
		size_of::<ProgressAction>()
	}
	fn apply(&mut self, action: &ProgressAction) -> Result<(), String> {
		match action {
			ProgressAction::AwardScore { amount } => {
				if *amount < 0 {
					return Err("Score awards must be nonnegative".into());
				}
				self.score = self
					.score
					.checked_add(*amount)
					.ok_or("Campaign score overflow")?;
			}
			ProgressAction::CollectCourtyardKey => {
				if !self.has_courtyard_key {
					let next_score = self.score.checked_add(1).ok_or("Campaign score overflow")?;

					self.score = next_score;
					self.has_courtyard_key = true;
				}
			}

			ProgressAction::ReachCourtyardExit => {
				if !self.has_courtyard_key {
					return Err("The courtyard exit requires the key".into());
				}

				self.courtyard_exit_reached = true;
			}
			ProgressAction::CompleteRun => {
				if !self.courtyard_exit_reached {
					return Err("Reach the courtyard exit before completing the run".into());
				}
				self.completed = true;
			}
		}
		Ok(())
	}
}

pub struct CampaignFactory;
impl GameModuleFactory for CampaignFactory {
	fn create_progress(&self) -> Result<ProgressState, String> {
		ProgressState::new(CampaignProgress::default())
	}

	fn create(&self, _: &ProjectData, _: &SceneId) -> Result<Box<dyn GameModule>, String> {
		Err("Courtyard Run requires prepared scene data".into())
	}

	fn create_prepared(
		&self,
		config: PreparedModuleConfig<'_>,
	) -> Result<Box<dyn GameModule>, String> {
		let (roles, finish) = if config.scene_id().as_str() == FINISH_SCENE_ID {
			// Finish scene: validate its player and finish marker.
			(None, Some(FinishRoles::from_config(&config)?))
		} else {
			// Other scenes: keep the existing course rules.
			(CourseRoles::from_config(&config)?, None)
		};

		let base = ConfiguredBehaviorFactory.create_prepared(config)?;

		Ok(Box::new(CampaignModule {
			base,
			roles,
			finish,
		}))
	}
}

struct FinishRoles {
	player: EntityKey,
	marker: EntityKey,
}

impl FinishRoles {
	fn from_config(config: &PreparedModuleConfig<'_>) -> Result<Self, String> {
		let bindings = &config.document().bindings;

		let required = |name: &str| {
			bindings.get(name).cloned().ok_or_else(|| {
				format!(
					"Scene {:?} requires a {name:?} binding",
					config.scene_id().as_str(),
				)
			})
		};

		let player = required("player")?;
		let marker = required("finish")?;

		if player == marker {
			return Err("Player and Finish must reference different entities".into());
		}

		let player_can_move = can_move(config, &player);

		if !player_can_move {
			return Err("The finish scene's Player needs a controlled movement \
                 behavior with positive speed"
				.into());
		}

		Ok(Self { player, marker })
	}
}

struct CampaignModule {
	base: Box<dyn GameModule>,
	roles: Option<CourseRoles>,
	finish: Option<FinishRoles>,
}

struct CourseRoles {
	player: EntityKey,
	key: EntityKey,
	exit: EntityKey,
	destination: SceneId,
}

impl CourseRoles {
	fn from_config(config: &PreparedModuleConfig<'_>) -> Result<Option<Self>, String> {
		let bindings = &config.document().bindings;

		if !bindings.contains_key("key") && !bindings.contains_key("exit") {
			return Ok(None);
		}

		let required = |name: &str| {
			bindings.get(name).cloned().ok_or_else(|| {
				format!(
					"Scene {:?} requires a {name:?} binding for Courtyard Run",
					config.scene_id().as_str(),
				)
			})
		};

		let player = required("player")?;
		let key = required("key")?;
		let exit = required("exit")?;

		if player == key || player == exit || key == exit {
			return Err("Player, Key, and Exit must reference different entities".into());
		}

		let player_can_move = can_move(config, &player);

		if !player_can_move {
			return Err("The Player role needs a controlled movement behavior \
                 with positive speed"
				.into());
		}

		let destination = SceneId::new(FINISH_SCENE_ID)?;
		config.project().scenes.get(&destination).map_err(|_| {
			format!(
				"Scene {:?} requires the Courtyard Run destination scene {:?} in the project catalog",
				config.scene_id().as_str(),
				destination.as_str(),
			)
		})?;

		Ok(Some(Self {
			player,
			key,
			exit,
			destination,
		}))
	}
}

fn can_move(config: &PreparedModuleConfig<'_>, player: &EntityKey) -> bool {
	config.behaviors().iter().any(|behavior| {
		behavior.entity == *player
			&& matches!(
				&behavior.settings,
				BehaviorSettings::ControlledMove { speed, .. }
					if *speed > 0.0
			)
	})
}

const INTERACTION_RADIUS: f32 = 0.35;

impl CampaignModule {
	fn pending_objective(
		&self,
		world: &RuntimeWorld,
		progress: &CampaignProgress,
	) -> Option<ProgressAction> {
		if progress.completed {
			return None;
		}

		if let Some(finish) = &self.finish {
			if progress.courtyard_exit_reached
				&& within_radius(world, &finish.player, &finish.marker, INTERACTION_RADIUS)
			{
				return Some(ProgressAction::CompleteRun);
			}

			return None;
		}

		let roles = self.roles.as_ref()?;

		if !progress.has_courtyard_key
			&& within_radius(world, &roles.player, &roles.key, INTERACTION_RADIUS)
		{
			return Some(ProgressAction::CollectCourtyardKey);
		}

		if progress.has_courtyard_key
			&& !progress.courtyard_exit_reached
			&& within_radius(world, &roles.player, &roles.exit, INTERACTION_RADIUS)
		{
			return Some(ProgressAction::ReachCourtyardExit);
		}

		None
	}
}

fn within_radius(world: &RuntimeWorld, first: &EntityKey, second: &EntityKey, radius: f32) -> bool {
	let (Some(first_index), Some(second_index)) =
		(world.entity_index(first), world.entity_index(second))
	else {
		return false;
	};

	let first_position = world.world_matrices()[first_index].w_axis.truncate();

	let second_position = world.world_matrices()[second_index].w_axis.truncate();

	first_position.distance_squared(second_position) <= radius * radius
}

impl GameModule for CampaignModule {
	fn status(&self, progress: ProgressView<'_>) -> Option<GameplayStatus> {
		let progress = progress.get::<CampaignProgress>()?;
		let objective = if self.roles.is_none() && self.finish.is_none() {
			return None;
		} else if progress.completed {
			"Courtyard Run complete"
		} else if self.finish.is_some() {
			if progress.courtyard_exit_reached {
				"Reach the finish marker"
			} else {
				"Start in the courtyard to unlock the finish"
			}
		} else if progress.courtyard_exit_reached {
			"Courtyard exit reached"
		} else if progress.has_courtyard_key {
			"Reach the courtyard exit"
		} else {
			"Find the courtyard key"
		};
		let detail = if progress.has_courtyard_key {
			"Courtyard key collected"
		} else {
			"Courtyard key not collected"
		};
		Some(GameplayStatus {
			objective: objective.into(),
			detail: Some(detail.into()),
			score: Some(progress.score),
			completed: progress.completed,
		})
	}

	fn needs_tick(&self, input: &InputFrame) -> bool {
		self.base.needs_tick(input)
	}
	fn needs_tick_with_progress(
		&self,
		world: &RuntimeWorld,
		input: &InputFrame,
		progress: ProgressView<'_>,
	) -> bool {
		let campaign = progress.get::<CampaignProgress>();
		if campaign.is_some_and(|value| value.completed) {
			return false;
		}
		self.base.needs_tick_with_progress(world, input, progress)
			|| campaign.is_some_and(|value| self.pending_objective(world, value).is_some())
	}
	fn plan(
		&self,
		world: &RuntimeWorld,
		input: &InputFrame,
		step: f32,
		output: &mut ActionWriter<'_>,
	) -> Result<(), String> {
		self.base.plan(world, input, step, output)
	}

	fn plan_with_progress(
		&self,
		context: GameplayContext<'_>,
		output: &mut ActionWriter<'_>,
	) -> Result<(), String> {
		let progress = context
			.progress
			.get::<CampaignProgress>()
			.ok_or("Campaign progress is unavailable")?;

		if progress.completed {
			return Ok(());
		}

		let objective = self.pending_objective(context.world, progress);

		if matches!(&objective, Some(ProgressAction::CompleteRun)) {
			return output.push_progress::<CampaignProgress>(ProgressAction::CompleteRun);
		}

		self.base.plan_with_progress(context, output)?;

		if let Some(action) = objective {
			let reached_exit = matches!(&action, ProgressAction::ReachCourtyardExit);
			output.push_progress::<CampaignProgress>(action)?;
			if reached_exit && let Some(roles) = &self.roles {
				output.push(SimulationAction::RequestScene {
					scene_id: roles.destination.clone(),
				})?;
			}
		}

		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use engine_core::{
		SceneData,
		game_progress::ProgressAction as TypedProgressAction,
		game_session::GameSession,
		project::{BehaviorSpace, BehaviorSpec},
		scene_data::{EntityData, TransformData},
	};

	fn course() -> (ProjectData, SceneId, Arc<SceneData>) {
		let project =
			ProjectData::from_json(include_str!("../../../apps/game/project.json")).unwrap();
		let scene_id = SceneId::new("courtyard").unwrap();
		let document = Arc::new(
			SceneData::from_json(include_str!(
				"../../../apps/game/assets/scenes/courtyard.json"
			))
			.unwrap(),
		);
		(project, scene_id, document)
	}

	#[test]
	fn module_descriptor_matches_the_registered_module_and_scene_role_sets() {
		let descriptor = descriptor();
		let (mut project, course_id, _) = course();
		project.gameplay_module = Some(descriptor.id.clone());
		assert!(resolve(&project).is_ok());
		assert!(!descriptor.label.is_empty());
		assert!(!descriptor.description.is_empty());
		let names = |roles: &[GameplayRoleDescriptorDto]| {
			roles
				.iter()
				.map(|role| role.name.clone())
				.collect::<Vec<_>>()
		};
		assert_eq!(names(&descriptor.default_roles), ["player", "key", "exit"]);
		assert_eq!(descriptor.scene_roles.len(), 1);
		assert_eq!(
			names(&descriptor.scene_roles[FINISH_SCENE_ID]),
			["player", "finish"]
		);
		assert!(!descriptor.scene_roles.contains_key(course_id.as_str()));
		for role in descriptor
			.default_roles
			.iter()
			.chain(descriptor.scene_roles.values().flatten())
		{
			assert!(!role.label.is_empty());
			assert!(!role.requirement_hint.is_empty());
		}
	}

	#[test]
	fn selecting_courtyard_run_accepts_unfinished_roles_and_play_checks_them() {
		use engine_core::{
			EngineBuilder,
			command::{EditorCommandDto, EditorCommandRequest},
		};
		let (mut project, _, document) = course();
		project.gameplay_module = None;
		let mut incomplete = (*document).clone();
		incomplete.bindings.retain(|name, _| name == "key");
		let mut engine = EngineBuilder::new(incomplete.clone(), project)
			.module_resolver(Arc::new(resolve))
			.build()
			.unwrap();
		let mut project = engine.session().project().clone();
		project.gameplay_module = Some(descriptor().id);
		engine
			.execute(EditorCommandRequest {
				expected_revision: engine.session().revision(),
				command: EditorCommandDto::SetProject { project },
			})
			.unwrap();
		let revision = engine.session().revision();
		assert_eq!(*engine.session().authored_document(), incomplete);
		let error = engine
			.execute(EditorCommandRequest {
				expected_revision: revision,
				command: EditorCommandDto::StartPlay,
			})
			.unwrap_err();
		assert!(error.message.contains("player"));
		assert!(!engine.session().is_playing());
		assert_eq!(engine.session().revision(), revision);
		assert_eq!(*engine.session().authored_document(), incomplete);
	}

	fn finish_document() -> SceneData {
		SceneData::from_json(include_str!("../../../apps/game/assets/scenes/finish.json")).unwrap()
	}

	fn progress_action(action: ProgressAction) -> TypedProgressAction {
		TypedProgressAction::new::<CampaignProgress>(action).unwrap()
	}

	fn start(
		project: &ProjectData,
		scene_id: &SceneId,
		document: Arc<SceneData>,
	) -> Result<GameSession, String> {
		GameSession::with_factory_for_scene(
			Arc::new(project.clone()),
			scene_id.clone(),
			document,
			resolve(project)?,
		)
	}

	fn step(game: &mut GameSession) -> engine_core::simulation_session::TickOutcome {
		let interval = 1.0 / f64::from(game.active().project().fixed_hz);
		game.tick(interval).unwrap()
	}

	fn drive(game: &mut GameSession, player: &EntityKey, x: f32, steps: u32) -> Option<SceneId> {
		game.active_mut().set_input(player, [x, 0.0, 0.0]).unwrap();
		let mut request = None;
		for _ in 0..steps {
			if let Some(scene_id) = step(game).requested_scene {
				assert!(request.replace(scene_id).is_none());
			}
		}
		game.active_mut().clear_input();
		request
	}

	fn assert_progress(game: &GameSession, expected: (i32, bool, bool)) {
		let progress = game.progress::<CampaignProgress>().unwrap();
		assert_eq!(
			(
				progress.score,
				progress.has_courtyard_key,
				progress.courtyard_exit_reached,
			),
			expected,
		);
	}

	#[test]
	fn bundled_course_requires_the_key_finishes_at_the_marker_and_restarts() {
		let (project, scene_id, document) = course();
		assert_eq!(project.startup_scene_id, scene_id);
		let authored = (*document).clone();
		let player = &authored.bindings["player"];
		let mut game = start(&project, &scene_id, Arc::clone(&document)).unwrap();
		assert_progress(&game, (0, false, false));
		let initial_status = game.status();
		assert_eq!(
			initial_status.gameplay.as_ref().unwrap(),
			&GameplayStatus {
				objective: "Find the courtyard key".into(),
				detail: Some("Courtyard key not collected".into()),
				score: Some(0),
				completed: false,
			},
		);

		assert!(drive(&mut game, player, -1.0, 90).is_none());
		assert_progress(&game, (0, false, false));
		assert_eq!(game.progress_revision(), 0);

		assert!(drive(&mut game, player, 1.0, 180).is_none());
		assert_progress(&game, (1, true, false));
		assert_eq!(game.progress_revision(), 1);
		let collected = game.status().gameplay.unwrap();
		assert_eq!(collected.objective, "Reach the courtyard exit");
		assert_eq!(collected.detail.as_deref(), Some("Courtyard key collected"));
		assert_eq!(collected.score, Some(1));

		assert!(drive(&mut game, player, -1.0, 90).is_none());
		assert!(drive(&mut game, player, 1.0, 90).is_none());
		assert_progress(&game, (1, true, false));
		assert_eq!(game.progress_revision(), 1);

		let finish = SceneId::new("finish").unwrap();
		assert_eq!(drive(&mut game, player, -1.0, 180), Some(finish.clone()));
		assert_eq!(game.pending_scene(), Some(&finish));
		let pending = game.status();
		assert_eq!(pending.pending_scene_id.as_ref(), Some(&finish));
		assert_eq!(
			pending.gameplay.unwrap().objective,
			"Courtyard exit reached"
		);
		assert_eq!(game.active().scene_id(), &scene_id);
		assert_progress(&game, (1, true, true));
		assert_eq!(game.progress_revision(), 2);
		assert_ne!(*game.active().document(), authored);
		assert_eq!(*document, authored);

		let finish_document = Arc::new(finish_document());
		let finish_player = &finish_document.bindings["player"];
		let transition = game
			.prepare_pending_scene(Arc::clone(&finish_document))
			.unwrap();
		game.commit_scene(transition).unwrap();
		assert_eq!(game.active().scene_id(), &finish);
		assert!(game.pending_scene().is_none());
		let at_finish = game.status();
		assert_eq!(at_finish.play_session_id, initial_status.play_session_id);
		assert_eq!(at_finish.run_id, initial_status.run_id);
		assert_eq!(
			at_finish.gameplay.unwrap().objective,
			"Reach the finish marker"
		);
		assert_progress(&game, (1, true, true));
		assert!(!step(&mut game).progress_changed);
		assert!(!game.progress::<CampaignProgress>().unwrap().completed);

		assert!(drive(&mut game, finish_player, 1.0, 90).is_none());
		assert!(game.progress::<CampaignProgress>().unwrap().completed);
		assert_progress(&game, (1, true, true));
		assert_eq!(game.progress_revision(), 3);
		let completion = game.status().gameplay.unwrap();
		assert_eq!(completion.objective, "Courtyard Run complete");
		assert!(completion.completed);
		assert_eq!(completion.score, Some(1));

		let final_frame = game.active().document();
		game.active_mut()
			.set_input(finish_player, [-1.0, 0.0, 0.0])
			.unwrap();
		assert!(!game.needs_tick());
		assert_eq!(step(&mut game).steps, 0);
		assert_eq!(game.active().document(), final_frame);

		let restart = game
			.prepare_new_game(scene_id.clone(), Arc::clone(&document))
			.unwrap();
		game.commit_scene(restart).unwrap();
		assert_eq!(game.active().scene_id(), &scene_id);
		assert_progress(&game, (0, false, false));
		assert!(!game.progress::<CampaignProgress>().unwrap().completed);
		assert_eq!(game.progress_revision(), 0);
		assert_eq!(game.active().document(), document);
		let restarted = game.status();
		assert_eq!(restarted.play_session_id, initial_status.play_session_id);
		assert_ne!(restarted.run_id, initial_status.run_id);
		assert_eq!(restarted.gameplay, initial_status.gameplay);
	}

	#[test]
	fn failed_finish_preparation_preserves_progress_and_stop_restores_authored_history() {
		use engine_core::{
			EngineBuilder,
			command::{EditorCommandDto as Command, EditorCommandRequest},
		};
		fn command(engine: &mut engine_core::Engine, command: Command) {
			engine
				.execute(EditorCommandRequest {
					expected_revision: engine.session().revision(),
					command,
				})
				.unwrap();
		}
		let (project, startup, document) = course();
		let player = document.bindings["player"].clone();
		let original_name = document
			.entities
			.iter()
			.find(|entity| entity.key == player)
			.unwrap()
			.name
			.clone();
		let mut invalid_finish = finish_document();
		invalid_finish.bindings.remove("finish");
		let mut engine = EngineBuilder::new((*document).clone(), project)
			.with_scenes(vec![(SceneId::new("finish").unwrap(), invalid_finish)])
			.module_resolver(Arc::new(resolve))
			.build()
			.unwrap();
		command(
			&mut engine,
			Command::Rename {
				key: player.as_str().into(),
				name: "Unsaved authored player".into(),
			},
		);
		command(
			&mut engine,
			Command::Select {
				keys: vec![player.as_str().into()],
			},
		);
		let authored = engine.snapshot().unwrap();
		command(&mut engine, Command::StartPlay);
		for (direction, steps) in [(1.0, 90), (-1.0, 180)] {
			command(
				&mut engine,
				Command::SetPlayInput {
					movement: [direction, 0.0, 0.0],
				},
			);
			for _ in 0..steps {
				engine.tick(1.0 / 60.0).unwrap();
			}
		}
		let accepted = engine.play_status().unwrap();
		assert_eq!(
			accepted.pending_scene_id.as_ref().unwrap().as_str(),
			"finish"
		);
		assert_eq!(accepted.gameplay.as_ref().unwrap().score, Some(1));
		let progress = engine
			.session()
			.play_progress::<CampaignProgress>()
			.unwrap()
			.clone();
		assert!(
			progress.has_courtyard_key && progress.courtyard_exit_reached && !progress.completed
		);
		let world = engine.session().render_document();
		let revision = engine.session().revision();
		let cursor = engine.change_cursor();
		let error = engine.prepare_pending_play_scene().err().unwrap();
		assert!(error.message.contains(r#"requires a "finish" binding"#));
		assert_eq!(engine.play_status().unwrap(), accepted);
		assert_eq!(engine.session().render_document(), world);
		assert!(engine.session().play_progress::<CampaignProgress>() == Some(&progress));
		assert_eq!(engine.session().revision(), revision);
		assert_eq!(engine.change_cursor(), cursor);
		let moved = engine.tick(1.0 / 60.0).unwrap();
		assert!(
			!moved.changes.is_empty(),
			"The source scene remains playable after preparation fails"
		);
		assert!(moved.requested_scene.is_none());
		assert!(engine.session().play_progress::<CampaignProgress>() == Some(&progress));
		command(&mut engine, Command::StopPlay);
		let stopped = engine.snapshot().unwrap();
		assert_eq!(stopped.active_scene_id, startup);
		assert_eq!(stopped.document, authored.document);
		assert_eq!(stopped.project, authored.project);
		assert_eq!(stopped.selection, authored.selection);
		assert!(stopped.dirty && stopped.can_undo);
		assert!(engine.play_status().is_none());
		command(&mut engine, Command::Undo);
		assert_eq!(
			engine
				.session()
				.authored_document()
				.entities
				.iter()
				.find(|entity| entity.key == player)
				.unwrap()
				.name,
			original_name
		);
	}

	#[test]
	fn only_the_bound_player_picks_up_the_key_in_world_space_even_after_input_release() {
		let (mut project, scene_id, document) = course();
		let mut document = (*document).clone();
		let player = document.bindings["player"].clone();
		let key = &document.bindings["key"];
		let marker = document
			.entities
			.iter()
			.find(|entity| &entity.key == key)
			.unwrap()
			.transform
			.position;
		let parent = EntityKey::new("offset-parent").unwrap();
		let spectator = EntityKey::new("spectator").unwrap();
		let mut parent_position = marker;
		parent_position[0] -= INTERACTION_RADIUS + 0.01;
		document.entities.extend([
			EntityData::new(parent.clone(), "Offset parent", "props/robot.glb")
				.with_renderable(false)
				.with_transform(TransformData {
					position: parent_position,
					..Default::default()
				}),
			EntityData::new(spectator.clone(), "Spectator", "props/robot.glb").with_transform(
				TransformData {
					position: marker,
					..Default::default()
				},
			),
		]);
		let player_entity = document
			.entities
			.iter_mut()
			.find(|entity| entity.key == player)
			.unwrap();
		player_entity.parent = Some(parent);
		player_entity.transform.position = [0.0; 3];
		project.add_behavior(BehaviorSpec::ControlledMove {
			entity: spectator,
			speed: 1.0,
			space: BehaviorSpace::World,
		});

		let mut game = start(&project, &scene_id, Arc::new(document)).unwrap();
		assert!(!game.needs_tick());
		assert_eq!(step(&mut game).steps, 0);
		assert_progress(&game, (0, false, false));

		assert!(drive(&mut game, &player, 1.0, 1).is_none());
		assert_progress(&game, (0, false, false));
		assert!(game.needs_tick());
		assert!(step(&mut game).progress_changed);
		assert_progress(&game, (1, true, false));
		assert!(!game.needs_tick());
	}

	#[test]
	fn invalid_course_roles_are_rejected_during_game_preparation() {
		let (mut project, scene_id, document) = course();
		for (role, expected) in [
			("player", "requires a \"player\" binding"),
			("key", "requires a \"key\" binding"),
			("exit", "requires a \"exit\" binding"),
		] {
			let mut incomplete = (*document).clone();
			incomplete.bindings.remove(role);
			let error = start(&project, &scene_id, Arc::new(incomplete))
				.err()
				.unwrap();
			assert!(error.contains(expected), "{error}");
		}

		let mut duplicate = (*document).clone();
		let player = duplicate.bindings["player"].clone();
		duplicate.bindings.insert("key".into(), player.clone());
		let error = start(&project, &scene_id, Arc::new(duplicate))
			.err()
			.unwrap();
		assert!(error.contains("different entities"), "{error}");

		let mut without_finish = project.clone();
		let finish = SceneId::new("finish").unwrap();
		without_finish.scenes.remove(&finish).unwrap();
		without_finish
			.behaviors
			.retain(|entry| entry.scene_id != finish);
		let error = start(&without_finish, &scene_id, Arc::clone(&document))
			.err()
			.unwrap();
		assert!(error.contains("destination scene \"finish\""), "{error}");

		project
			.behaviors
			.retain(|behavior| behavior.entity() != &player);
		project.add_behavior(BehaviorSpec::ControlledMove {
			entity: player,
			speed: 0.0,
			space: BehaviorSpace::World,
		});
		let error = start(&project, &scene_id, document).err().unwrap();
		assert!(error.contains("positive speed"), "{error}");
	}

	#[test]
	fn scenes_without_objectives_keep_movement_and_projects_without_campaign_have_no_progress() {
		let (mut project, scene_id, document) = course();
		let robot_scene_id = project
			.scenes
			.iter()
			.find(|scene| scene.asset_path() == "scenes/robots.json")
			.unwrap()
			.id()
			.clone();
		let robots = Arc::new(
			SceneData::from_json(include_str!("../../../apps/game/assets/scenes/robots.json"))
				.unwrap(),
		);
		let robot = &robots.bindings["robot_a"];
		let mut game = start(&project, &robot_scene_id, Arc::clone(&robots)).unwrap();
		assert!(drive(&mut game, robot, 1.0, 180).is_none());

		let world = game.active().world();
		let index = world.entity_index(robot).unwrap();
		assert!(world.world_matrices()[index].w_axis.x > 1.0);
		assert_progress(&game, (0, false, false));
		assert!(game.status().gameplay.is_none());

		project.gameplay_module = None;
		let generic = start(&project, &scene_id, document).unwrap();
		assert!(generic.progress::<CampaignProgress>().is_none());
	}

	#[test]
	fn finish_requires_the_route_and_bound_player_proximity_and_completes_once() {
		let (project, _, _) = course();
		let finish_id = SceneId::new("finish").unwrap();
		let authored = finish_document();
		for route_stage in 0..=2 {
			for nearby in [false, true] {
				let mut document = authored.clone();
				let player = document.bindings["player"].clone();
				let marker = &document.bindings["finish"];
				let marker_position = document
					.entities
					.iter()
					.find(|entity| &entity.key == marker)
					.unwrap()
					.transform
					.position;
				let player_entity = document
					.entities
					.iter_mut()
					.find(|entity| entity.key == player)
					.unwrap();
				player_entity.transform.position = marker_position;
				player_entity.transform.position[0] +=
					INTERACTION_RADIUS + if nearby { -0.01 } else { 0.01 };
				// An unrelated entity at the marker must not satisfy the player role.
				document.entities.push(
					EntityData::new(
						EntityKey::new("finish-spectator").unwrap(),
						"Spectator",
						"props/cube.glb",
					)
					.with_transform(TransformData {
						position: marker_position,
						..Default::default()
					}),
				);
				let mut game = start(&project, &finish_id, Arc::new(document)).unwrap();
				if route_stage >= 1 {
					game.apply_progress_actions(&[progress_action(
						ProgressAction::CollectCourtyardKey,
					)])
					.unwrap();
				}
				if route_stage == 2 {
					game.apply_progress_actions(&[progress_action(
						ProgressAction::ReachCourtyardExit,
					)])
					.unwrap();
				}
				let expected = route_stage == 2 && nearby;
				let revision = game.progress_revision();
				assert_eq!(game.needs_tick(), expected);
				assert_eq!(step(&mut game).progress_changed, expected);
				assert_eq!(
					game.progress::<CampaignProgress>().unwrap().completed,
					expected
				);
				assert_eq!(game.progress_revision(), revision + u64::from(expected));
				assert!(!game.needs_tick());
				assert!(!step(&mut game).progress_changed);
				assert_eq!(game.progress_revision(), revision + u64::from(expected));
			}
		}
	}

	#[test]
	fn invalid_finish_roles_are_rejected_during_game_preparation() {
		let (mut project, _, _) = course();
		let finish = SceneId::new(FINISH_SCENE_ID).unwrap();
		let authored = finish_document();
		for role in ["player", "finish"] {
			let mut incomplete = authored.clone();
			incomplete.bindings.remove(role);
			let error = start(&project, &finish, Arc::new(incomplete))
				.err()
				.unwrap();
			assert!(
				error.contains(&format!("requires a {role:?} binding")),
				"{error}"
			);
		}

		let player = authored.bindings["player"].clone();
		let mut duplicate = authored.clone();
		duplicate.bindings.insert("finish".into(), player);
		let error = start(&project, &finish, Arc::new(duplicate)).err().unwrap();
		assert!(error.contains("different entities"), "{error}");

		project
			.behaviors
			.retain(|behavior| behavior.scene_id != finish);
		let error = start(&project, &finish, Arc::new(authored)).err().unwrap();
		assert!(error.contains("positive speed"), "{error}");
	}

	#[test]
	fn reaching_finish_in_a_catchup_tick_freezes_the_rest_of_that_tick() {
		let (project, _, _) = course();
		let finish = SceneId::new(FINISH_SCENE_ID).unwrap();
		let interval = 1.0 / f64::from(project.fixed_hz);
		let mut document = finish_document();
		let player = document.bindings["player"].clone();
		let marker = &document.bindings["finish"];
		let marker_position = document
			.entities
			.iter()
			.find(|entity| &entity.key == marker)
			.unwrap()
			.transform
			.position;
		let player_entity = document
			.entities
			.iter_mut()
			.find(|entity| entity.key == player)
			.unwrap();
		player_entity.transform.position = marker_position;
		let before_x = marker_position[0] - INTERACTION_RADIUS - interval as f32 / 2.0;
		player_entity.transform.position[0] = before_x;

		let mut game = start(&project, &finish, Arc::new(document)).unwrap();
		game.apply_progress_actions(&[
			progress_action(ProgressAction::CollectCourtyardKey),
			progress_action(ProgressAction::ReachCourtyardExit),
		])
		.unwrap();
		game.active_mut()
			.set_input(&player, [1.0, 0.0, 0.0])
			.unwrap();

		let outcome = game.tick(interval * 5.0).unwrap();
		assert!(outcome.steps > 2);
		assert!(outcome.progress_changed);
		assert!(outcome.requested_scene.is_none());
		assert!(game.progress::<CampaignProgress>().unwrap().completed);
		let world = game.active().world();
		let after_x = world.world_matrices()[world.entity_index(&player).unwrap()]
			.w_axis
			.x;
		assert!((after_x - (before_x + interval as f32)).abs() < 1.0e-5);
		assert!(!game.needs_tick());
	}

	#[test]
	fn progress_actions_guard_the_key_reward_exit_and_completion() {
		let mut progress = CampaignFactory.create_progress().unwrap();
		let exit = || progress_action(ProgressAction::ReachCourtyardExit);
		let collect = || progress_action(ProgressAction::CollectCourtyardKey);
		let complete = || progress_action(ProgressAction::CompleteRun);

		assert!(
			ProgressState::new(CampaignProgress {
				completed: true,
				..Default::default()
			})
			.is_err()
		);
		assert!(progress.apply_batch(&[complete()]).is_err());
		assert!(progress.apply_batch(&[exit()]).is_err());
		assert_eq!(progress.view().revision(), 0);
		assert!(progress.apply_batch(&[collect(), collect()]).unwrap());
		assert_eq!(progress.view().get::<CampaignProgress>().unwrap().score, 1);
		assert!(!progress.apply_batch(&[collect()]).unwrap());
		assert!(progress.apply_batch(&[complete()]).is_err());
		assert_eq!(progress.view().revision(), 1);
		assert!(progress.apply_batch(&[exit()]).unwrap());
		assert!(!progress.apply_batch(&[exit()]).unwrap());
		assert_eq!(progress.view().revision(), 2);
		assert!(progress.apply_batch(&[complete()]).unwrap());
		assert!(!progress.apply_batch(&[complete()]).unwrap());
		assert_eq!(progress.view().revision(), 3);
	}
}
