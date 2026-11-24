//! Single-local-player input ownership for one runtime world.
use super::SimulationSession;
use crate::{
	EntityKey,
	character_controller::CharacterRuntime,
	components::PlayerControllerSettings,
	input::{GameKeyboardInput, PhysicalGamepadSample},
	runtime_lifecycle::{RuntimeEntityHandle, RuntimeLifecycle},
	world::RuntimeWorld,
};
use std::{collections::HashSet, sync::Arc};

/// The owner of a local input stream captured before scheduling native delivery.
/// Reacquire this token after a focus reset, possession change, or scene change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalInputScope {
	pub target: RuntimeEntityHandle,
	pub possession_generation: u64,
	pub context_epoch: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PossessionChange {
	pub target: Option<RuntimeEntityHandle>,
}

#[derive(Clone)]
pub(super) struct LocalPossession {
	pub owner: Option<RuntimeEntityHandle>,
	pub managed: bool,
	pub generation: u64,
	prepared_controls: Arc<PlayerControllerSettings>,
	context_epoch: u64,
	last_sequence: Option<u64>,
}

impl LocalPossession {
	pub(super) fn new(world: &RuntimeWorld, lifecycle: &RuntimeLifecycle) -> Self {
		let player = world.data().local_player();
		Self {
			owner: player.and_then(|actor| lifecycle.handle(&actor.key)),
			managed: player.is_some(),
			generation: 0,
			prepared_controls: Arc::new(
				player
					.and_then(|actor| actor.player_controller.as_ref())
					.map(|controller| controller.settings.clone())
					.unwrap_or_default(),
			),
			context_epoch: 0,
			last_sequence: None,
		}
	}

	pub(super) fn validate(
		&self,
		change: &PossessionChange,
		world: &RuntimeWorld,
		lifecycle: &RuntimeLifecycle,
		characters: &CharacterRuntime,
	) -> Result<(), String> {
		let Some(target) = &change.target else {
			return Ok(());
		};
		lifecycle.validate_handle(target)?;
		let actor = world
			.entity_index(&target.key)
			.map(|index| &world.data().entities[index])
			.ok_or("Possession target is missing")?;
		let controller = actor
			.player_controller
			.as_ref()
			.filter(|controller| controller.enabled && characters.contains(&target.key))
			.ok_or("Possession requires an enabled Player and Character Controller")?;
		if controller.settings.input != self.prepared_controls.input
			|| controller.settings.control_graph != self.prepared_controls.control_graph
		{
			return Err(
				"Possession target must share the prepared actions and Controls Graph".into(),
			);
		}
		Ok(())
	}

	pub(super) fn stage(
		pending: &mut Option<PossessionChange>,
		change: PossessionChange,
	) -> Result<(), String> {
		if pending.as_ref().is_some_and(|previous| previous != &change) {
			return Err("Conflicting local possession requests in one step".into());
		}
		*pending = Some(change);
		Ok(())
	}

	/// Only structural removal after planning can invalidate a staged target.
	/// Keep a surviving owner if the requested successor disappeared.
	pub(super) fn finish_step(
		&mut self,
		pending: Option<PossessionChange>,
		lifecycle: &RuntimeLifecycle,
	) -> Result<Option<PossessionChange>, String> {
		let live = |target: &RuntimeEntityHandle| lifecycle.validate_handle(target).is_ok();
		let next = match pending {
			Some(change) if change.target.as_ref().is_none_or(live) => change.target,
			_ if self.owner.as_ref().is_some_and(|owner| !live(owner)) => None,
			_ => return Ok(None),
		};
		if next == self.owner {
			return Ok(None);
		}
		self.generation = self
			.generation
			.checked_add(1)
			.ok_or("Local possession generation counter exhausted")?;
		let previous = PossessionChange {
			target: std::mem::replace(&mut self.owner, next),
		};
		self.managed = true;
		self.reset_context();
		Ok(Some(previous))
	}

	pub(super) fn replay_state(&self, awaiting_neutral: bool) -> serde_json::Value {
		serde_json::json!({
			"owner": self.owner.as_ref().map(|owner| &owner.key),
			"generation": self.generation,
			"awaiting_neutral": awaiting_neutral,
		})
	}

	pub(super) fn reset_context(&mut self) {
		self.context_epoch = self.context_epoch.saturating_add(1);
		self.last_sequence = None;
	}

	fn scope(&self) -> Option<LocalInputScope> {
		if self.context_epoch == u64::MAX {
			return None;
		}
		Some(LocalInputScope {
			target: self.owner.clone()?,
			possession_generation: self.generation,
			context_epoch: self.context_epoch,
		})
	}

	fn accepts(&self, scope: &LocalInputScope, sequence: u64) -> bool {
		self.context_epoch != u64::MAX
			&& self.owner.as_ref() == Some(&scope.target)
			&& self.generation == scope.possession_generation
			&& self.context_epoch == scope.context_epoch
			&& self.last_sequence.is_none_or(|last| sequence > last)
	}
}

impl SimulationSession {
	/// The live target; the authored local flag still describes the next Play run.
	pub fn local_player(&self) -> Option<&RuntimeEntityHandle> {
		self.possession.owner.as_ref()
	}

	pub fn local_possession_generation(&self) -> u64 {
		self.possession.generation
	}

	/// Capture a token for delayed or ordered local samples.
	pub fn local_input_scope(&self) -> Option<LocalInputScope> {
		if self.paused {
			return None;
		}
		self.possession.scope()
	}

	/// Stage a handoff for the next committed fixed-step boundary.
	pub fn request_local_possession(
		&mut self,
		expected_generation: u64,
		target: Option<RuntimeEntityHandle>,
	) -> Result<(), String> {
		if self.paused || self.possession.generation != expected_generation {
			return Err("Local possession request belongs to an inactive generation".into());
		}
		let change = PossessionChange { target };
		self.possession
			.validate(&change, &self.world, &self.lifecycle, &self.characters)?;
		LocalPossession::stage(&mut self.pending_possession, change)
	}

	/// Submit a resolved sample from the current owner. A stale or repeated
	/// sequence returns false before touching any character or action buffer.
	pub fn submit_local_controls(
		&mut self,
		scope: &LocalInputScope,
		sequence: u64,
		input: GameKeyboardInput,
	) -> Result<bool, String> {
		if self.paused || !self.possession.accepts(scope, sequence) {
			return Ok(false);
		}
		self.set_game_keyboard_input(&super::MovementTarget::AllControlled, input)?;
		self.possession.last_sequence = Some(sequence);
		Ok(true)
	}

	/// Submit physical codes for the prepared Input Actions asset.
	pub fn submit_local_action_codes(
		&mut self,
		scope: &LocalInputScope,
		sequence: u64,
		keyboard: &HashSet<String>,
		physical: PhysicalGamepadSample,
	) -> Result<bool, String> {
		if self.paused || !self.possession.accepts(scope, sequence) {
			return Ok(false);
		}
		let accepted = self.sample_action_codes(keyboard, physical)?;
		if accepted {
			self.possession.last_sequence = Some(sequence);
		}
		Ok(accepted)
	}

	pub(super) fn commit_possession_input(
		&mut self,
		previous: Option<&EntityKey>,
		next: &LocalPossession,
		characters: &mut CharacterRuntime,
	) {
		let require_neutral = self.inputs.awaiting_neutral
			|| !self.inputs.is_idle()
			|| self
				.conditional_input
				.as_ref()
				.is_some_and(super::conditional_input::ConditionalInputs::has_held_sample);
		for key in previous
			.into_iter()
			.chain(next.owner.as_ref().map(|target| &target.key))
		{
			characters.clear_entity_input(key);
		}
		if let Some(input) = &mut self.conditional_input {
			input.retarget(next.owner.clone());
		}
		self.inputs.reset_local_controls(
			next.owner.clone(),
			next.managed,
			require_neutral,
			self.control_graph.is_some(),
		);
	}
}
