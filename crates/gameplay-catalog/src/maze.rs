//! The Hollow Maze rules shared by Editor Play, the Player, and replay.

mod board;

use std::{collections::HashSet, mem::size_of, sync::Arc};

use engine_core::{
	EntityKey, SceneData,
	animation::AnimationPlayback,
	command::{GameplayModuleDescriptorDto, GameplayRoleDescriptorDto},
	game_progress::{GameProgress, ProgressState, ProgressView},
	project::{BehaviorSettings, ProjectData},
	scene_catalog::SceneId,
	scene_data::EntityData,
	simulation_session::{
		ActionWriter, ConfiguredBehaviorFactory, GameModule, GameModuleFactory, GameplayContext,
		GameplayStatus, InputFrame, PreparedModuleConfig, SimulationAction,
	},
	world::RuntimeWorld,
};

use self::board::{Board, Cell, Direction};

pub(super) const ID: &str = "knight_maze";

const ALL_SEALS: u8 = 0b111;
const TURN_SECONDS: f32 = 0.14;
const JUMP_SECONDS: f32 = 0.30;
const JUMP_HEIGHT_METERS: f32 = 1.35;
const ROLE_LABELS: [(&str, &str, &str); 6] = [
	("player", "Knight", "Root at P with Controlled Move."),
	(
		"monster",
		"Monster",
		"Root at M. It moves every second turn.",
	),
	("seal_1", "First seal", "Root at cell 1."),
	("seal_2", "Second seal", "Root at cell 2."),
	("seal_3", "Third seal", "Root at cell 3."),
	("gate", "Exit gate", "Root with a solid collider at E."),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
	Searching,
	Escaped,
	Caught,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Command {
	Move(Direction),
	Jump(Direction),
	Wait,
}

impl Command {
	fn direction(self) -> Option<Direction> {
		match self {
			Self::Move(direction) | Self::Jump(direction) => Some(direction),
			Self::Wait => None,
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Motion {
	player: Cell,
	monster: Cell,
	jumping: bool,
	elapsed_steps: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Run {
	phase: Phase,
	player: Cell,
	monster: Cell,
	seals: u8,
	chase_due: bool,
	motion: Option<Motion>,
	facing: Direction,
	held_directions: u8,
	buffered: Option<Command>,
}

impl Run {
	fn new(board: &Board) -> Self {
		Self {
			phase: Phase::Searching,
			player: board.player,
			monster: board.monster,
			seals: 0,
			chase_due: false,
			motion: None,
			facing: Direction::East,
			held_directions: 0,
			buffered: None,
		}
	}

	fn exit_open(self) -> bool {
		self.seals == ALL_SEALS
	}
}

impl GameProgress for Run {
	type Action = Self;

	fn validate(&self) -> Result<(), String> {
		if self.seals & !ALL_SEALS != 0
			|| (self.phase == Phase::Escaped && !self.exit_open())
			|| (self.phase != Phase::Searching && self.motion.is_some())
		{
			return Err("Inconsistent Hollow Maze run state".into());
		}
		Ok(())
	}

	fn retained_bytes(&self) -> usize {
		size_of::<Self>()
	}

	fn action_bytes(_: &Self::Action) -> usize {
		size_of::<Self>()
	}

	fn apply(&mut self, action: &Self::Action) -> Result<(), String> {
		*self = *action;
		Ok(())
	}
}

struct Factory {
	scene: SceneId,
	board: Arc<Board>,
	turn_steps: u32,
	jump_steps: u32,
}

impl Factory {
	fn from_project(project: &ProjectData) -> Result<Self, String> {
		let settings = project
			.gameplay_settings
			.as_ref()
			.and_then(serde_json::Value::as_object)
			.ok_or("The Hollow Maze requires gameplay settings")?;
		let [Some(version), Some(scene), Some(rows), Some(tile_size)] =
			["schema_version", "scene", "rows", "tile_size"].map(|key| settings.get(key))
		else {
			return Err(
				"The Hollow Maze settings need schema_version, scene, rows, and tile_size".into(),
			);
		};
		if settings.len() != 4 || version.as_u64() != Some(1) {
			return Err(
				"Unsupported Hollow Maze settings. Use schema version 1 and its four fields".into(),
			);
		}
		let scene: SceneId = serde_json::from_value(scene.clone())
			.map_err(|error| format!("Invalid Hollow Maze scene ID: {error}"))?;
		project.scene_path(&scene)?;
		let rows: Vec<String> = serde_json::from_value(rows.clone())
			.map_err(|error| format!("Invalid Hollow Maze rows: {error}"))?;
		let tile_size: f32 = serde_json::from_value(tile_size.clone())
			.map_err(|error| format!("Invalid Hollow Maze tile size: {error}"))?;
		Ok(Self {
			scene,
			board: Arc::new(Board::parse(&rows, tile_size)?),
			turn_steps: (project.fixed_hz as f32 * TURN_SECONDS).round().max(2.0) as u32,
			jump_steps: (project.fixed_hz as f32 * JUMP_SECONDS).round().max(2.0) as u32,
		})
	}
}

impl GameModuleFactory for Factory {
	fn create_progress(&self) -> Result<ProgressState, String> {
		ProgressState::new(Run::new(&self.board))
	}

	fn create(&self, _: &ProjectData, _: &SceneId) -> Result<Box<dyn GameModule>, String> {
		Err("The Hollow Maze requires a prepared scene".into())
	}

	fn create_prepared(
		&self,
		config: PreparedModuleConfig<'_>,
	) -> Result<Box<dyn GameModule>, String> {
		if config.scene_id() != &self.scene {
			return ConfiguredBehaviorFactory.create_prepared(config);
		}

		let board = &self.board;
		let cells = [
			board.player,
			board.monster,
			board.seals[0],
			board.seals[1],
			board.seals[2],
			board.exit,
		];
		let mut entities = Vec::new();
		let mut seen = HashSet::new();
		for ((role, _, _), cell) in ROLE_LABELS.into_iter().zip(cells) {
			let entity = bound_root(config.document(), role, board.center(cell))?;
			if !seen.insert(&entity.key) {
				return Err("Each Hollow Maze role needs its own entity".into());
			}
			entities.push(entity);
		}
		let [player, monster, seal_one, seal_two, seal_three, gate] = entities.as_slice() else {
			return Err("The Hollow Maze requires six scene bindings".into());
		};
		if !config.behaviors().iter().any(|behavior| {
			behavior.entity == player.key
				&& matches!(behavior.settings, BehaviorSettings::ControlledMove { speed, .. } if speed > 0.0)
		}) {
			return Err("Assign Controlled Move to the Hollow Maze Knight".into());
		}
		if !gate.collision_enabled
			|| gate
			.collider
			.as_ref()
			.is_none_or(|collider| collider.interaction().sensor)
		{
			return Err("The Hollow Maze gate needs an enabled solid collider".into());
		}
		Ok(Box::new(Maze {
			board: Arc::clone(board),
			player: player.key.clone(),
			monster: monster.key.clone(),
			seals: [
				seal_one.key.clone(),
				seal_two.key.clone(),
				seal_three.key.clone(),
			],
			gate: gate.key.clone(),
			player_height: player.transform.position[1],
			turn_steps: self.turn_steps,
			jump_steps: self.jump_steps,
		}))
	}
}

struct Maze {
	board: Arc<Board>,
	player: EntityKey,
	monster: EntityKey,
	seals: [EntityKey; 3],
	gate: EntityKey,
	player_height: f32,
	turn_steps: u32,
	jump_steps: u32,
}

impl Maze {
	fn begin_turn(&self, run: Run, command: Command) -> Option<Motion> {
		let player = match command {
			Command::Move(direction) => {
				self.board
					.neighbor(run.player, direction, run.exit_open())?
			}
			Command::Jump(direction) => self
				.board
				.jump_destination(run.player, direction, run.exit_open())
				.unwrap_or(run.player),
			Command::Wait => run.player,
		};
		let monster = if run.chase_due && player != run.monster {
			self.board
				.shortest_step(run.monster, player, run.exit_open())
				.unwrap_or(run.monster)
		} else {
			run.monster
		};
		Some(Motion {
			player,
			monster,
			jumping: matches!(command, Command::Jump(_)),
			elapsed_steps: 0,
		})
	}

	fn advance_turn(
		&self,
		world: &RuntimeWorld,
		run: &mut Run,
		mut motion: Motion,
		output: &mut ActionWriter<'_>,
	) -> Result<(), String> {
		let steps = if motion.jumping {
			self.jump_steps
		} else {
			self.turn_steps
		};
		let starting = motion.elapsed_steps == 0;
		motion.elapsed_steps += 1;
		let finished = motion.elapsed_steps == steps;
		let fraction = motion.elapsed_steps as f32 / steps as f32;
		let clip = (starting || finished).then_some(u32::from(starting));
		if run.player != motion.player || motion.jumping {
			let height = motion.jumping.then_some({
				self.player_height + JUMP_HEIGHT_METERS * 4.0 * fraction * (1.0 - fraction)
			});
			move_actor(
				world,
				output,
				&self.player,
				interpolated_center(&self.board, run.player, motion.player, fraction),
				run.facing,
				height,
			)?;
			if let Some(clip) = clip {
				play_clip(world, output, &self.player, clip)?;
			}
		}
		if run.monster != motion.monster {
			move_actor(
				world,
				output,
				&self.monster,
				interpolated_center(&self.board, run.monster, motion.monster, fraction),
				Direction::between(run.monster, motion.monster),
				None,
			)?;
			if let Some(clip) = clip {
				play_clip(world, output, &self.monster, clip)?;
			}
		}
		if finished {
			self.finish_turn(run, motion, output)?;
		} else {
			run.motion = Some(motion);
		}
		Ok(())
	}

	fn finish_turn(
		&self,
		run: &mut Run,
		motion: Motion,
		output: &mut ActionWriter<'_>,
	) -> Result<(), String> {
		run.player = motion.player;
		run.monster = motion.monster;
		run.chase_due = !run.chase_due;

		if run.player == run.monster {
			run.phase = Phase::Caught;
			return Ok(());
		}

		if let Some(index) = self.board.seals.iter().position(|&cell| cell == run.player)
			&& run.seals & (1 << index) == 0
		{
			run.seals |= 1 << index;
			output.push(SimulationAction::SetVisible {
				entity: self.seals[index].clone(),
				visible: false,
			})?;
			if run.exit_open() {
				output.push(SimulationAction::SetVisible {
					entity: self.gate.clone(),
					visible: false,
				})?;
				output.push(SimulationAction::SetCollisionEnabled {
					entity: self.gate.clone(),
					enabled: false,
				})?;
			}
		}
		if run.exit_open() && run.player == self.board.exit {
			run.phase = Phase::Escaped;
		}
		Ok(())
	}
}

impl GameModule for Maze {
	fn needs_tick(&self, input: &InputFrame) -> bool {
		!input.is_idle()
	}

	fn needs_tick_with_progress(
		&self,
		_: &RuntimeWorld,
		input: &InputFrame,
		progress: ProgressView<'_>,
	) -> bool {
		progress.get::<Run>().is_some_and(|run| {
			run.phase == Phase::Searching
				&& (self.needs_tick(input)
				|| run.held_directions != 0
				|| run.motion.is_some()
				|| run.buffered.is_some())
		})
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
		context: GameplayContext<'_>,
		output: &mut ActionWriter<'_>,
	) -> Result<(), String> {
		let current = *context
			.progress
			.get::<Run>()
			.ok_or("The Hollow Maze run was not initialized")?;
		if current.phase != Phase::Searching {
			return Ok(());
		}
		let mut next = current;
		let (held, pressed) = Direction::input_edge(
			current.held_directions,
			context.input.movement(&self.player),
		);
		next.held_directions = held;
		next.buffered = next.buffered.or_else(|| {
			let facing = || {
				pressed
					.or_else(|| Direction::first_in(held))
					.unwrap_or(next.facing)
			};
			if context.input.action_pressed("jump") {
				Some(Command::Jump(facing()))
			} else if context.input.action_pressed("step") {
				Some(Command::Move(facing()))
			} else {
				pressed.map(Command::Move).or_else(|| {
					context
						.input
						.action_pressed("wait")
						.then_some(Command::Wait)
				})
			}
		});
		let mut motion = next.motion.take();
		if motion.is_none()
			&& let Some(command) = next.buffered.take()
		{
			let previous_facing = next.facing;
			next.facing = command.direction().unwrap_or(previous_facing);
			motion = self.begin_turn(next, command);
			if motion.is_none() && next.facing != previous_facing {
				move_actor(
					context.world,
					output,
					&self.player,
					self.board.center(next.player),
					next.facing,
					None,
				)?;
			}
		}
		if let Some(motion) = motion {
			self.advance_turn(context.world, &mut next, motion, output)?;
		}
		if next != current {
			output.push_progress::<Run>(next)?;
		}
		Ok(())
	}

	fn status(&self, progress: ProgressView<'_>) -> Option<GameplayStatus> {
		let run = *progress.get::<Run>()?;
		let (objective, detail) = match run.phase {
			Phase::Searching => {
				let objective = if run.exit_open() {
					"Gate open. Reach the green exit.".into()
				} else {
					format!("Find the three seals ({}/3)", run.seals.count_ones())
				};
				let warning = if run.chase_due {
					"Monster moves next."
				} else {
					"Monster waits next."
				};
				(
					objective,
					format!("WASD/LMB step. Space/RMB jump. Q wait. {warning}"),
				)
			}
			Phase::Escaped => (
				"You escaped the Hollow Maze".into(),
				"All three seals are safe.".into(),
			),
			Phase::Caught => (
				"The monster caught the knight".into(),
				"Try a different route or wait at a junction.".into(),
			),
		};
		Some(GameplayStatus {
			objective,
			detail: Some(detail),
			score: None,
			completed: run.phase != Phase::Searching,
		})
	}
}

pub(super) fn resolve(project: &ProjectData) -> Result<Arc<dyn GameModuleFactory>, String> {
	Ok(Arc::new(Factory::from_project(project)?))
}

pub(super) fn descriptor() -> GameplayModuleDescriptorDto {
	GameplayModuleDescriptorDto {
		id: ID.into(),
		label: "The Hollow Maze".into(),
		description:
		"A turn-based knight puzzle with barriers to jump, three seals, and a pursuing monster."
			.into(),
		default_roles: ROLE_LABELS
			.into_iter()
			.map(
				|(name, label, requirement_hint)| GameplayRoleDescriptorDto {
					name: name.into(),
					label: label.into(),
					requirement_hint: requirement_hint.into(),
				},
			)
			.collect(),
		scene_roles: Default::default(),
	}
}

fn bound_root<'a>(
	document: &'a SceneData,
	role: &str,
	center: [f32; 2],
) -> Result<&'a EntityData, String> {
	let entity = document
		.bindings
		.get(role)
		.and_then(|key| document.entities.iter().find(|entity| entity.key == *key))
		.ok_or_else(|| format!("Assign the Hollow Maze {role} role"))?;
	let [x, _, z] = entity.transform.position;
	if !entity.active
		|| entity.parent.is_some()
		|| (x - center[0]).abs() > 0.05
		|| (z - center[1]).abs() > 0.05
	{
		return Err(format!(
			"Put the active Hollow Maze {role} root at grid center ({}, {})",
			center[0], center[1]
		));
	}
	Ok(entity)
}

fn interpolated_center(board: &Board, from: Cell, to: Cell, fraction: f32) -> [f32; 2] {
	let [x0, z0] = board.center(from);
	let [x1, z1] = board.center(to);
	[x0 + (x1 - x0) * fraction, z0 + (z1 - z0) * fraction]
}

fn move_actor(
	world: &RuntimeWorld,
	output: &mut ActionWriter<'_>,
	actor: &EntityKey,
	[x, z]: [f32; 2],
	facing: Direction,
	height: Option<f32>,
) -> Result<(), String> {
	let index = world
		.entity_index(actor)
		.ok_or("Hollow Maze actor is missing")?;
	let mut transform = world.data().entities[index].transform.clone();
	transform.position[0] = x;
	transform.position[1] = height.unwrap_or(transform.position[1]);
	transform.position[2] = z;
	transform.rotation[1] = facing.yaw();
	output.push(SimulationAction::SetTransform {
		entity: actor.clone(),
		transform,
	})
}

fn play_clip(
	world: &RuntimeWorld,
	output: &mut ActionWriter<'_>,
	actor: &EntityKey,
	clip_index: u32,
) -> Result<(), String> {
	let animation = world
		.entity_index(actor)
		.and_then(|index| world.data().entities[index].animation.as_ref());
	if animation.is_some_and(|component| {
		component.enabled
			&& matches!(
				component.settings.playback,
				AnimationPlayback::SingleClip { .. }
			)
	}) {
		output.push(SimulationAction::PlayAnimation {
			entity: actor.clone(),
			clip_index,
			looping: true,
		})?;
	}
	Ok(())
}

#[cfg(test)]
mod tests;
