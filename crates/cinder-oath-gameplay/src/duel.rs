use std::mem::size_of;

use engine_core::{
	EntityKey,
	animation::AnimationPlayback,
	game_progress::{GameProgress, ProgressView},
	scene_catalog::SceneId,
	scene_data::{EntityData, TransformData},
	simulation_session::{
		ActionWriter, GameModule, GameplayContext, GameplayStatus, InputFrame, SimulationAction,
	},
	world::RuntimeWorld,
};
use glam::Vec3;

const PLAYER_HEALTH: u8 = 3;
const WARDEN_HEALTH: u8 = 3;
const PLAYER_REACH: f32 = 2.5;
const PLAYER_FRONT_DOT: f32 = 0.70;
const WARDEN_REACH: f32 = 1.82;
const WARDEN_APPROACH_SPEED: f32 = 2.2;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
	#[default]
	Intro,
	Duel,
	Victory,
	Defeat,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Clip {
	#[default]
	Idle = 0,
	Walk = 1,
	Attack = 2,
	Defeat = 3,
}

impl Clip {
	fn looping(self) -> bool {
		matches!(self, Self::Idle | Self::Walk)
	}
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum WardenMotion {
	#[default]
	Approach,
	Swing {
		ticks_left: u32,
	},
}

/// Committed facts shared by the HUD, audio, and replay hosts for one run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CinderProgress {
	phase: Phase,
	intro_ticks: u32,
	player_health: u8,
	warden_health: u8,
	player_swing: u32,
	warden_motion: WardenMotion,
	player_clip: Clip,
	warden_clip: Clip,
	swings: u32,
	enemy_hits: u32,
	player_hits: u32,
}

impl CinderProgress {
	/// Start an authored arena entry with a fresh duel and full health.
	pub fn arena_entry() -> Self {
		Self {
			phase: Phase::Duel,
			..Self::default()
		}
	}

	pub fn phase(&self) -> Phase {
		self.phase
	}

	/// Number of accepted sword swings since the current run began.
	pub fn swings(&self) -> u32 {
		self.swings
	}

	/// Number of sword strikes that damaged the Warden.
	pub fn enemy_hits(&self) -> u32 {
		self.enemy_hits
	}

	/// Number of Warden strikes that damaged the player.
	pub fn player_hits(&self) -> u32 {
		self.player_hits
	}
}

impl Default for CinderProgress {
	fn default() -> Self {
		Self {
			phase: Phase::Intro,
			intro_ticks: 0,
			player_health: PLAYER_HEALTH,
			warden_health: WARDEN_HEALTH,
			player_swing: 0,
			warden_motion: WardenMotion::Approach,
			player_clip: Clip::Idle,
			warden_clip: Clip::Idle,
			swings: 0,
			enemy_hits: 0,
			player_hits: 0,
		}
	}
}

/// Only this module can construct a replacement for its run state.
pub struct Advance(CinderProgress);

impl GameProgress for CinderProgress {
	type Action = Advance;

	fn validate(&self) -> Result<(), String> {
		if self.player_health > PLAYER_HEALTH
			|| self.warden_health > WARDEN_HEALTH
			|| self.enemy_hits != u32::from(WARDEN_HEALTH - self.warden_health)
			|| self.player_hits != u32::from(PLAYER_HEALTH - self.player_health)
			|| self.enemy_hits > self.swings
			|| (self.phase == Phase::Victory) != (self.warden_health == 0)
			|| (self.phase == Phase::Defeat) != (self.player_health == 0)
		{
			return Err("Inconsistent Cinder Oath run state".into());
		}
		Ok(())
	}

	fn retained_bytes(&self) -> usize {
		size_of::<Self>()
	}

	fn action_bytes(_: &Advance) -> usize {
		size_of::<Advance>()
	}

	fn apply(&mut self, action: &Advance) -> Result<(), String> {
		*self = action.0;
		Ok(())
	}
}

#[derive(Clone, Copy)]
pub(super) struct Timing {
	intro_ticks: u32,
	swing_ticks: u32,
	player_impact_ticks_left: u32,
	warden_swing_ticks: u32,
	warden_impact_ticks_left: u32,
}

impl Timing {
	pub(super) fn new(hz: u32) -> Self {
		let ticks = |seconds: f32| (seconds * hz as f32).round() as u32;
		let swing = ticks(0.76).max(2);
		let warden_swing = ticks(1.22).max(2);
		Self {
			intro_ticks: ticks(3.7).max(1),
			swing_ticks: swing,
			player_impact_ticks_left: swing - ticks(0.28).clamp(1, swing - 1),
			warden_swing_ticks: warden_swing,
			warden_impact_ticks_left: warden_swing - ticks(0.36).clamp(1, warden_swing - 1),
		}
	}
}

pub(super) struct Intro {
	track: EntityKey,
	start: TransformData,
	travel: Vec3,
	arena: SceneId,
	timing: Timing,
}

impl Intro {
	pub(super) fn new(
		track: &EntityData,
		travel: [f32; 3],
		arena: SceneId,
		timing: Timing,
	) -> Self {
		Self {
			track: track.key.clone(),
			start: track.transform.clone(),
			travel: Vec3::from_array(travel),
			arena,
			timing,
		}
	}
}

impl GameModule for Intro {
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
		context: GameplayContext<'_>,
		output: &mut ActionWriter<'_>,
	) -> Result<(), String> {
		let current = read_progress(context.progress)?;
		if current.phase != Phase::Intro {
			return Ok(());
		}
		let mut next = current;
		next.intro_ticks += 1;
		if next.intro_ticks >= self.timing.intro_ticks || context.input.action_pressed("start") {
			next.phase = Phase::Duel;
			output.push(SimulationAction::RequestScene {
				scene_id: self.arena.clone(),
			})?;
		} else {
			let t = next.intro_ticks as f32 / self.timing.intro_ticks as f32;
			let distance = t * t * (3.0 - 2.0 * t);
			let mut transform = self.start.clone();
			transform.position =
				(Vec3::from_array(self.start.position) + self.travel * distance).to_array();
			output.push(SimulationAction::SetTransform {
				entity: self.track.clone(),
				transform,
			})?;
		}
		output.push_progress::<CinderProgress>(Advance(next))
	}

	fn status(&self, _: ProgressView<'_>) -> Option<GameplayStatus> {
		Some(GameplayStatus {
			objective: "CINDER OATH".into(),
			detail: Some("Enter to skip the intro".into()),
			score: None,
			completed: false,
		})
	}
}

pub(super) struct Duel {
	player: EntityKey,
	enemy: EntityKey,
	sword: EntityKey,
	player_speed: f32,
	timing: Timing,
}

impl Duel {
	pub(super) fn new(
		player: EntityKey,
		enemy: EntityKey,
		sword: EntityKey,
		player_speed: f32,
		timing: Timing,
	) -> Self {
		Self {
			player,
			enemy,
			sword,
			player_speed,
			timing,
		}
	}

	fn update_sword(
		&self,
		context: &GameplayContext<'_>,
		state: &mut CinderProgress,
		player: Vec3,
		warden: Vec3,
	) {
		if state.player_swing == 0 {
			if context.input.action_pressed("attack") {
				state.player_swing = self.timing.swing_ticks;
				state.swings = state.swings.saturating_add(1);
			}
			return;
		}
		state.player_swing -= 1;
		if state.player_swing != self.timing.player_impact_ticks_left {
			return;
		}
		let forward = Vec3::from_array(context.camera.forward());
		let eye = Vec3::from_array(context.camera.position);
		let aim = (warden + Vec3::Y * 1.05 - eye).normalize_or_zero();
		let gap = horizontal(warden - player).length_squared();
		if gap <= PLAYER_REACH * PLAYER_REACH
			&& forward.dot(aim) >= PLAYER_FRONT_DOT
			&& context.collisions.interaction_segment_clear(
				context.world,
				&self.player,
				&self.enemy,
			) {
			state.warden_health -= 1;
			state.enemy_hits += 1;
			if state.warden_health == 0 {
				state.phase = Phase::Victory;
			}
		}
	}

	fn update_warden(
		&self,
		context: &GameplayContext<'_>,
		state: &mut CinderProgress,
		warden_to_player: Vec3,
		output: &mut ActionWriter<'_>,
	) -> Result<(), String> {
		let gap = warden_to_player.length();
		if state.phase == Phase::Victory {
			state.warden_clip = Clip::Defeat;
			return output.push(SimulationAction::SetCollisionEnabled {
				entity: self.enemy.clone(),
				enabled: false,
			});
		}
		match state.warden_motion {
			WardenMotion::Approach => {
				let direction = warden_to_player.normalize_or_zero();
				let (index, _) = position(context.world, &self.enemy)?;
				let mut transform = context.world.data().entities[index].transform.clone();
				// The Warden's authored model faces local -Z.
				let yaw = (-direction.x).atan2(-direction.z);
				if direction != Vec3::ZERO && transform.rotation[1] != yaw {
					transform.rotation[1] = yaw;
					output.push(SimulationAction::SetTransform {
						entity: self.enemy.clone(),
						transform,
					})?;
				}
				if gap <= WARDEN_REACH
					&& context.collisions.interaction_segment_clear(
						context.world,
						&self.enemy,
						&self.player,
					) {
					state.warden_motion = WardenMotion::Swing {
						ticks_left: self.timing.warden_swing_ticks,
					};
					state.warden_clip = Clip::Attack;
				} else {
					state.warden_clip = Clip::Walk;
					output.push(SimulationAction::Move {
						entity: self.enemy.clone(),
						world_delta: (direction
							* (WARDEN_APPROACH_SPEED * context.step_seconds).min(gap))
						.to_array(),
					})?;
				}
			}
			WardenMotion::Swing { ticks_left } => {
				let remaining = ticks_left - 1;
				if remaining == self.timing.warden_impact_ticks_left {
					let (warden, _) = position(context.world, &self.enemy)?;
					let forward = horizontal(
						context.world.world_matrices()[warden].transform_vector3(-Vec3::Z),
					)
					.normalize_or_zero();
					if gap <= WARDEN_REACH
						&& forward.dot(warden_to_player.normalize_or_zero()) >= PLAYER_FRONT_DOT
						&& context.collisions.interaction_segment_clear(
							context.world,
							&self.enemy,
							&self.player,
						) {
						state.player_health -= 1;
						state.player_hits += 1;
						if state.player_health == 0 {
							state.phase = Phase::Defeat;
						}
					}
				}
				if remaining == 0 {
					state.warden_motion = WardenMotion::Approach;
					state.warden_clip = Clip::Idle;
				} else {
					state.warden_motion = WardenMotion::Swing {
						ticks_left: remaining,
					};
				}
			}
		}
		Ok(())
	}
}

impl GameModule for Duel {
	fn needs_tick(&self, input: &InputFrame) -> bool {
		!input.is_idle()
	}

	fn needs_tick_with_progress(
		&self,
		_: &RuntimeWorld,
		_: &InputFrame,
		progress: ProgressView<'_>,
	) -> bool {
		progress
			.get::<CinderProgress>()
			.is_some_and(|state| matches!(state.phase, Phase::Intro | Phase::Duel))
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
		let current = read_progress(context.progress)?;
		output.push(SimulationAction::AttachToView {
			entity: self.sword.clone(),
		})?;
		if matches!(current.phase, Phase::Victory | Phase::Defeat) {
			return Ok(());
		}
		let mut next = current;
		next.phase = Phase::Duel;
		let (_, player) = position(context.world, &self.player)?;
		let (_, warden) = position(context.world, &self.enemy)?;
		let player_to_warden = horizontal(warden - player);
		self.update_sword(&context, &mut next, player, warden);
		self.update_warden(&context, &mut next, -player_to_warden, output)?;

		let movement = move_from_camera(&context, &self.player);
		next.player_clip = if next.phase == Phase::Defeat {
			Clip::Defeat
		} else if next.player_swing > 0 {
			Clip::Attack
		} else if movement != Vec3::ZERO {
			Clip::Walk
		} else {
			Clip::Idle
		};
		if movement != Vec3::ZERO && next.phase == Phase::Duel {
			let pace = if next.player_swing > 0 { 0.72 } else { 1.0 };
			output.push(SimulationAction::Move {
				entity: self.player.clone(),
				world_delta: (movement * self.player_speed * pace * context.step_seconds)
					.to_array(),
			})?;
		}
		write_clip(
			context.world,
			output,
			&self.sword,
			current.player_clip,
			next.player_clip,
		)?;
		write_clip(
			context.world,
			output,
			&self.enemy,
			current.warden_clip,
			next.warden_clip,
		)?;
		if current != next {
			output.push_progress::<CinderProgress>(Advance(next))?;
		}
		Ok(())
	}

	fn status(&self, progress: ProgressView<'_>) -> Option<GameplayStatus> {
		let state = progress.get::<CinderProgress>()?;
		let (objective, detail) = match state.phase {
			Phase::Victory => ("VICTORY", "The Cinder Warden has fallen".into()),
			Phase::Defeat => (
				"DEFEAT",
				"Restart to challenge the Cinder Warden again".into(),
			),
			_ => {
				let warning = if matches!(state.warden_motion, WardenMotion::Swing { ticks_left } if ticks_left > self.timing.warden_impact_ticks_left)
				{
					" - Warden winding up"
				} else {
					""
				};
				(
					"Defeat the Cinder Warden",
					format!(
						"Health {}/{PLAYER_HEALTH} - Warden {}/{WARDEN_HEALTH}{warning}",
						state.player_health, state.warden_health
					),
				)
			}
		};
		Some(GameplayStatus {
			objective: objective.into(),
			detail: Some(detail),
			score: Some(i32::from(WARDEN_HEALTH - state.warden_health)),
			completed: state.phase == Phase::Victory,
		})
	}
}

fn read_progress(progress: ProgressView<'_>) -> Result<CinderProgress, String> {
	progress
		.get::<CinderProgress>()
		.copied()
		.ok_or_else(|| "Cinder Oath run state is missing".into())
}

fn position(world: &RuntimeWorld, key: &EntityKey) -> Result<(usize, Vec3), String> {
	let index = world
		.entity_index(key)
		.ok_or_else(|| format!("Cinder Oath entity {key:?} is missing"))?;
	Ok((index, world.world_matrices()[index].w_axis.truncate()))
}

fn horizontal(vector: Vec3) -> Vec3 {
	Vec3::new(vector.x, 0.0, vector.z)
}

fn move_from_camera(context: &GameplayContext<'_>, player: &EntityKey) -> Vec3 {
	let [right, forward_xy, forward_xz] = context.input.movement(player);
	// Keyboard sampling supplies XY, while targeted headless input can supply XZ.
	let forward = if forward_xy == 0.0 {
		-forward_xz
	} else {
		forward_xy
	};
	let ahead = horizontal(Vec3::from_array(context.camera.forward())).normalize_or_zero();
	let rightward = Vec3::from_array(context.camera.right());
	(rightward * right + ahead * forward).clamp_length_max(1.0)
}

fn write_clip(
	world: &RuntimeWorld,
	output: &mut ActionWriter<'_>,
	key: &EntityKey,
	previous: Clip,
	next: Clip,
) -> Result<(), String> {
	if next != previous
		&& world.entity_index(key).is_some_and(|index| {
			world.data().entities[index]
				.animation
				.as_ref()
				.is_some_and(|component| {
					component.enabled
						&& matches!(
							component.settings.playback,
							AnimationPlayback::SingleClip { .. }
						)
				})
		}) {
		output.push(SimulationAction::PlayAnimation {
			entity: key.clone(),
			clip_index: next as u32,
			looping: next.looping(),
		})?;
	}
	Ok(())
}
