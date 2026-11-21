//! Authored native state tables and their immutable compiled form.
mod evaluator;
mod validation;
pub(crate) use evaluator::{CompiledMachine, MachineInstance, compile, evaluate};

use crate::{
	EntityKey,
	gameplay_events::{ComponentRef, Signal},
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

pub const MAX_STATES: usize = 32;
pub const MAX_INPUTS: usize = 8;
pub const MAX_TRANSITIONS: usize = 128;
pub const MAX_ENTRY_ACTIONS: usize = 8;
pub const MAX_DEFINITION_BYTES: usize = 64 * 1024;
pub const MAX_TIMER_TICKS: u32 = 1_000_000;
pub const MAX_TRANSITIONS_PER_STEP: usize = 32;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachineDef {
	pub schema_version: u32,
	pub initial: String,
	pub inputs: Vec<SensorInput>,
	pub states: Vec<StateDef>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SensorInput {
	pub id: String,
	pub sensor: EntityKey,
	pub actor: EntityKey,
	pub signal: Signal,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateDef {
	pub id: String,
	pub on_enter: Vec<MachineAction>,
	pub transitions: Vec<TransitionDef>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransitionDef {
	pub when: MachineTrigger,
	pub next: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MachineTrigger {
	Signal {
		signal: Signal,
		source: Option<ComponentRef>,
	},
	AfterTicks {
		ticks: u32,
	},
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MachineAction {
	Emit {
		target: ComponentRef,
		signal: Signal,
	},
	SetVisible {
		entity: EntityKey,
		visible: bool,
	},
	SetCollisionEnabled {
		entity: EntityKey,
		enabled: bool,
	},
	DespawnOwner,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MachineDescriptor {
	pub definition_version: u32,
	pub signals: Vec<Signal>,
	pub action_kinds: Vec<String>,
	pub max_states: usize,
	pub max_inputs: usize,
	pub max_transitions: usize,
	pub max_entry_actions: usize,
	pub max_timer_ticks: u32,
	pub max_definition_bytes: usize,
}
pub fn descriptor() -> MachineDescriptor {
	MachineDescriptor {
		definition_version: 1,
		signals: vec![
			Signal::Activated,
			Signal::Released,
			Signal::Collected,
			Signal::Reset,
			Signal::Jumped,
			Signal::Landed,
		],
		action_kinds: [
			"emit",
			"set_visible",
			"set_collision_enabled",
			"despawn_owner",
		]
		.into_iter()
		.map(str::to_owned)
		.collect(),
		max_states: MAX_STATES,
		max_inputs: MAX_INPUTS,
		max_transitions: MAX_TRANSITIONS,
		max_entry_actions: MAX_ENTRY_ACTIONS,
		max_timer_ticks: MAX_TIMER_TICKS,
		max_definition_bytes: MAX_DEFINITION_BYTES,
	}
}

fn validate_id(value: &str) -> Result<(), String> {
	if value.is_empty()
		|| value.len() > 64
		|| !value
			.bytes()
			.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
	{
		return Err(
			"Machine IDs require 1–64 ASCII letters, digits, underscores or hyphens".into(),
		);
	}
	Ok(())
}
struct CountBytes(usize);
impl std::io::Write for CountBytes {
	fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
		let count = self
			.0
			.checked_add(bytes.len())
			.filter(|n| *n <= MAX_DEFINITION_BYTES)
			.ok_or_else(|| std::io::Error::other("Machine definition exceeds 64 KiB"))?;
		self.0 = count;
		Ok(bytes.len())
	}
	fn flush(&mut self) -> std::io::Result<()> {
		Ok(())
	}
}

impl MachineDef {
	pub fn validate(&self) -> Result<(), String> {
		if self.schema_version != 1
			|| self.states.is_empty()
			|| self.states.len() > MAX_STATES
			|| self.inputs.len() > MAX_INPUTS
		{
			return Err(
				"Machine requires schema 1, 1–32 states and at most 8 sensor inputs".into(),
			);
		}
		validate_id(&self.initial)?;
		let mut input_ids = HashSet::new();
		for input in &self.inputs {
			validate_id(&input.id)?;
			if !input_ids.insert(&input.id) || input.actor == input.sensor {
				return Err("Machine sensor IDs must be unique and actor/sensor distinct".into());
			}
		}
		let mut ids = HashSet::new();
		let mut transitions = 0usize;
		for state in &self.states {
			validate_id(&state.id)?;
			if !ids.insert(&state.id) {
				return Err("Machine state IDs must be unique".into());
			}
			transitions = transitions.saturating_add(state.transitions.len());
			if transitions > MAX_TRANSITIONS || state.on_enter.len() > MAX_ENTRY_ACTIONS {
				return Err(
					"Machine permits 128 total transitions and 8 entry actions per state".into(),
				);
			}
			let mut timer = false;
			for (index, transition) in state.transitions.iter().enumerate() {
				validate_id(&transition.next)?;
				if let MachineTrigger::AfterTicks { ticks } = transition.when {
					if timer || !(1..=MAX_TIMER_TICKS).contains(&ticks) {
						return Err("Machine state permits one timer in 1–1000000 ticks".into());
					}
					timer = true;
				}
				for previous in &state.transitions[..index] {
					if let (
						MachineTrigger::Signal {
							signal: a,
							source: x,
						},
						MachineTrigger::Signal {
							signal: b,
							source: y,
						},
					) = (&previous.when, &transition.when)
						&& a == b && (x.is_none() || y.is_none() || x == y)
					{
						return Err("Machine signal transitions overlap ambiguously".into());
					}
				}
			}
		}
		if !ids.contains(&self.initial)
			|| self
				.states
				.iter()
				.flat_map(|s| &s.transitions)
				.any(|t| !ids.contains(&t.next))
		{
			return Err("Machine initial state or transition destination is missing".into());
		}
		for key in self.entity_references() {
			key.validate()?;
			if key.as_str().len() > 256 || key.as_str().chars().any(char::is_control) {
				return Err("Machine entity reference exceeds its key budget".into());
			}
		}
		serde_json::to_writer(&mut CountBytes(0), self)
			.map_err(|e| format!("Machine definition budget: {e}"))?;
		Ok(())
	}
	pub fn entity_references(&self) -> Vec<&EntityKey> {
		let mut refs = Vec::new();
		for input in &self.inputs {
			refs.push(&input.sensor);
			refs.push(&input.actor);
		}
		for state in &self.states {
			for transition in &state.transitions {
				if let MachineTrigger::Signal {
					source: Some(source),
					..
				} = &transition.when
				{
					refs.push(&source.entity);
				}
			}
			for action in &state.on_enter {
				match action {
					MachineAction::Emit { target, .. } => refs.push(&target.entity),
					MachineAction::SetVisible { entity, .. }
					| MachineAction::SetCollisionEnabled { entity, .. } => refs.push(entity),
					MachineAction::DespawnOwner => {}
				}
			}
		}
		refs
	}
	pub fn remap(&mut self, remap: &HashMap<EntityKey, EntityKey>) {
		let replace = |key: &mut EntityKey| {
			if let Some(next) = remap.get(key) {
				*key = next.clone();
			}
		};
		for input in &mut self.inputs {
			replace(&mut input.sensor);
			replace(&mut input.actor);
		}
		for state in &mut self.states {
			for transition in &mut state.transitions {
				if let MachineTrigger::Signal {
					source: Some(source),
					..
				} = &mut transition.when
				{
					replace(&mut source.entity);
				}
			}
			for action in &mut state.on_enter {
				match action {
					MachineAction::Emit { target, .. } => replace(&mut target.entity),
					MachineAction::SetVisible { entity, .. }
					| MachineAction::SetCollisionEnabled { entity, .. } => replace(entity),
					MachineAction::DespawnOwner => {}
				}
			}
		}
	}
	pub fn retained_bytes(&self) -> usize {
		let mut bytes = self
			.initial
			.capacity()
			.saturating_add(
				self.inputs
					.capacity()
					.saturating_mul(std::mem::size_of::<SensorInput>()),
			)
			.saturating_add(
				self.states
					.capacity()
					.saturating_mul(std::mem::size_of::<StateDef>()),
			);
		for input in &self.inputs {
			bytes = bytes.saturating_add(input.id.capacity());
		}
		for state in &self.states {
			bytes = bytes
				.saturating_add(state.id.capacity())
				.saturating_add(
					state
						.on_enter
						.capacity()
						.saturating_mul(std::mem::size_of::<MachineAction>()),
				)
				.saturating_add(
					state
						.transitions
						.capacity()
						.saturating_mul(std::mem::size_of::<TransitionDef>()),
				);
			for transition in &state.transitions {
				bytes = bytes.saturating_add(transition.next.capacity());
				if let MachineTrigger::Signal {
					source: Some(source),
					..
				} = &transition.when
				{
					bytes = bytes.saturating_add(source.component.as_str().len());
				}
			}
			for action in &state.on_enter {
				if let MachineAction::Emit { target, .. } = action {
					bytes = bytes.saturating_add(target.component.as_str().len());
				}
			}
		}
		bytes
	}
}

#[cfg(test)]
#[path = "../tests/unit/state_machine.rs"]
mod tests;
