//! Authored native state tables and their immutable compiled form.
use crate::{
	EntityKey,
	components::NativeBehaviorSpec,
	gameplay_events::{ComponentRef, Envelope, Signal},
	scene_data::EntityData,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

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
	pub fn validate_scene(
		&self,
		entities: &HashMap<&str, &EntityData>,
		slots: &HashSet<EntityKey>,
	) -> Result<(), String> {
		let entity = |key: &EntityKey| {
			entities
				.get(key.as_str())
				.copied()
				.ok_or_else(|| "Machine target entity is missing".to_string())
		};
		let endpoint = |reference: &ComponentRef| -> Result<(), String> {
			if slots.contains(&reference.entity) {
				return Ok(());
			}
			if !entity(&reference.entity)?.native_behaviors.iter().any(|c| {
				c.id == reference.component
					&& matches!(c.behavior, NativeBehaviorSpec::StateMachine { .. })
			}) {
				return Err(format!(
					"Machine route requires state-machine attachment {} on {}",
					reference.component.as_str(),
					reference.entity.as_str()
				));
			}
			Ok(())
		};
		for input in &self.inputs {
			let sensor = if slots.contains(&input.sensor) {
				None
			} else {
				Some(
					entity(&input.sensor)?
						.collider
						.as_ref()
						.ok_or("Machine sensor requires a collider")?,
				)
			};
			let actor = if slots.contains(&input.actor) {
				None
			} else {
				Some(
					entity(&input.actor)?
						.collider
						.as_ref()
						.ok_or("Machine actor requires a collider")?,
				)
			};
			if sensor.is_some_and(|s| !s.interaction().sensor) {
				return Err("Machine input requires a Sensor collider".into());
			}
			if let (Some(actor), Some(sensor)) = (actor, sensor)
				&& !actor.interaction().detects(sensor.interaction())
			{
				return Err("Machine input collider filters must accept each other".into());
			}
		}
		for state in &self.states {
			for transition in &state.transitions {
				if let MachineTrigger::Signal {
					source: Some(source),
					..
				} = &transition.when
				{
					endpoint(source)?;
				}
			}
			for action in &state.on_enter {
				match action {
					MachineAction::Emit { target, .. } => endpoint(target)?,
					MachineAction::SetVisible { entity: key, .. }
						if !slots.contains(key) && !entity(key)?.has_mesh_component() =>
					{
						return Err("Machine visibility action requires a mesh renderer".into());
					}
					MachineAction::SetCollisionEnabled { entity: key, .. }
						if !slots.contains(key) && entity(key)?.collider.is_none() =>
					{
						return Err("Machine collision action requires a collider".into());
					}
					_ => {}
				}
			}
		}
		Ok(())
	}
}

#[derive(Clone, Debug)]
pub(crate) struct CompiledState {
	pub id: String,
	pub actions: Vec<MachineAction>,
	pub signals: Vec<(Signal, Option<ComponentRef>, usize)>,
	pub timer: Option<(u32, usize)>,
}
#[derive(Clone, Debug)]
pub(crate) struct CompiledMachine {
	pub initial: usize,
	pub inputs: Vec<SensorInput>,
	pub states: Vec<CompiledState>,
}
pub(crate) fn compile(definition: &MachineDef) -> Result<CompiledMachine, String> {
	definition.validate()?;
	let ids: BTreeMap<_, _> = definition
		.states
		.iter()
		.enumerate()
		.map(|(i, s)| (&s.id, i))
		.collect();
	let states = definition
		.states
		.iter()
		.map(|s| {
			let mut signals = Vec::new();
			let mut timer = None;
			for t in &s.transitions {
				match &t.when {
					MachineTrigger::Signal { signal, source } => {
						signals.push((*signal, source.clone(), ids[&t.next]))
					}
					MachineTrigger::AfterTicks { ticks } => timer = Some((*ticks, ids[&t.next])),
				}
			}
			CompiledState {
				id: s.id.clone(),
				actions: s.on_enter.clone(),
				signals,
				timer,
			}
		})
		.collect();
	let mut inputs = definition.inputs.clone();
	inputs.sort_by(|a, b| a.id.cmp(&b.id));
	Ok(CompiledMachine {
		initial: ids[&definition.initial],
		inputs,
		states,
	})
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct MachineInstance {
	pub state: usize,
	pub entered_step: u64,
	pub initialized: bool,
}
#[derive(Debug)]
pub(crate) struct MachinePlan {
	pub next: MachineInstance,
	pub effects: Vec<MachineAction>,
}
pub(crate) fn evaluate(
	compiled: &CompiledMachine,
	current: &MachineInstance,
	step: u64,
	inbox: &[Envelope],
) -> Result<MachinePlan, String> {
	let mut next = current.clone();
	let mut effects = Vec::new();
	let mut transitions = 0;
	let enter = |next: &mut MachineInstance, effects: &mut Vec<MachineAction>, target: usize| {
		next.state = target;
		next.entered_step = step;
		next.initialized = true;
		effects.extend(compiled.states[target].actions.iter().cloned());
	};
	if !next.initialized {
		enter(&mut next, &mut effects, compiled.initial);
	}
	for event in inbox {
		let target = compiled.states[next.state]
			.signals
			.iter()
			.find(|(signal, source, _)| {
				*signal == event.signal
					&& source.as_ref().is_none_or(|source| {
						source.entity == event.source.owner.key
							&& source.component == event.source.component
					})
			})
			.map(|(_, _, target)| *target);
		if let Some(target) = target {
			transitions += 1;
			if transitions > MAX_TRANSITIONS_PER_STEP {
				return Err("Machine transition budget exceeded".into());
			}
			enter(&mut next, &mut effects, target);
		}
	}
	let elapsed = step
		.checked_sub(next.entered_step)
		.ok_or("Machine entry step lies in the future")?;
	if let Some((ticks, target)) = compiled.states[next.state].timer
		&& elapsed >= u64::from(ticks)
	{
		if transitions >= MAX_TRANSITIONS_PER_STEP {
			return Err("Machine transition budget exceeded".into());
		}
		enter(&mut next, &mut effects, target);
	}
	Ok(MachinePlan { next, effects })
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{
		components::ComponentId, gameplay_events::ComponentEndpoint,
		runtime_lifecycle::RuntimeEntityHandle,
	};
	fn reference(key: &str) -> ComponentRef {
		ComponentRef {
			entity: EntityKey::new(key).unwrap(),
			component: ComponentId::new("logic").unwrap(),
		}
	}
	fn event(key: &str, signal: Signal) -> Envelope {
		let endpoint = ComponentEndpoint {
			owner: RuntimeEntityHandle {
				world_id: "test".into(),
				key: reference(key).entity,
				generation: 0,
			},
			component: reference(key).component,
		};
		Envelope {
			source: endpoint.clone(),
			target: endpoint,
			signal,
		}
	}
	fn state(id: &str, transitions: Vec<TransitionDef>) -> StateDef {
		StateDef {
			id: id.into(),
			on_enter: vec![],
			transitions,
		}
	}
	fn signal(source: Option<&str>, next: &str) -> TransitionDef {
		TransitionDef {
			when: MachineTrigger::Signal {
				signal: Signal::Activated,
				source: source.map(reference),
			},
			next: next.into(),
		}
	}
	fn definition(states: Vec<StateDef>) -> MachineDef {
		MachineDef {
			schema_version: 1,
			initial: states[0].id.clone(),
			inputs: vec![],
			states,
		}
	}
	#[test]
	fn inbox_order_folds_all_signals_before_the_single_timer() {
		let def = definition(vec![
			state(
				"closed",
				vec![signal(Some("left"), "left"), signal(Some("right"), "right")],
			),
			state("left", vec![signal(Some("right"), "open")]),
			state("right", vec![signal(Some("left"), "open")]),
			state(
				"open",
				vec![TransitionDef {
					when: MachineTrigger::AfterTicks { ticks: 2 },
					next: "closed".into(),
				}],
			),
		]);
		let compiled = compile(&def).unwrap();
		for inbox in [
			vec![
				event("left", Signal::Activated),
				event("right", Signal::Activated),
			],
			vec![
				event("right", Signal::Activated),
				event("left", Signal::Activated),
			],
		] {
			let opened = evaluate(&compiled, &MachineInstance::default(), 10, &inbox).unwrap();
			assert_eq!(compiled.states[opened.next.state].id, "open");
			assert_eq!(opened.next.entered_step, 10);
			let waiting = evaluate(&compiled, &opened.next, 11, &[]).unwrap();
			assert_eq!(waiting.next, opened.next);
			let closed = evaluate(&compiled, &opened.next, 12, &[]).unwrap();
			assert_eq!(compiled.states[closed.next.state].id, "closed");
			assert_eq!(closed.next.entered_step, 12);
		}
	}
	#[test]
	fn self_transition_reenters_and_signal_at_deadline_restarts_timer() {
		let mut def = definition(vec![
			state(
				"active",
				vec![
					signal(None, "active"),
					TransitionDef {
						when: MachineTrigger::AfterTicks { ticks: 2 },
						next: "expired".into(),
					},
				],
			),
			state("expired", vec![]),
		]);
		def.states[0].on_enter.push(MachineAction::SetVisible {
			entity: reference("visual").entity,
			visible: true,
		});
		let compiled = compile(&def).unwrap();
		let initial = evaluate(&compiled, &MachineInstance::default(), 1, &[]).unwrap();
		assert_eq!(initial.effects.len(), 1);
		let reentered = evaluate(
			&compiled,
			&initial.next,
			3,
			&[event("source", Signal::Activated)],
		)
		.unwrap();
		assert_eq!(reentered.next.state, 0);
		assert_eq!(reentered.next.entered_step, 3);
		assert_eq!(reentered.effects.len(), 1);
		assert_eq!(
			evaluate(&compiled, &reentered.next, 5, &[])
				.unwrap()
				.next
				.state,
			1
		);
	}
	#[test]
	fn transition_budget_is_exact_and_evaluation_does_not_mutate_input() {
		let compiled =
			compile(&definition(vec![state("loop", vec![signal(None, "loop")])])).unwrap();
		let current = MachineInstance::default();
		let events = vec![event("source", Signal::Activated); 32];
		assert!(evaluate(&compiled, &current, 1, &events).is_ok());
		let mut excess = events;
		excess.push(event("source", Signal::Activated));
		assert!(
			evaluate(&compiled, &current, 1, &excess)
				.unwrap_err()
				.contains("transition budget")
		);
		assert_eq!(current, MachineInstance::default());
	}
	#[test]
	fn malformed_tables_fail_before_compilation_and_unknown_fields_are_rejected() {
		let valid = definition(vec![state("ready", vec![])]);
		let mut invalid = valid.clone();
		invalid.states[0].transitions =
			vec![signal(None, "ready"), signal(Some("source"), "ready")];
		assert!(invalid.validate().unwrap_err().contains("overlap"));
		invalid = valid.clone();
		invalid.states[0].transitions = vec![signal(None, "missing")];
		assert!(invalid.validate().is_err());
		invalid = valid.clone();
		invalid.states[0].transitions = vec![TransitionDef {
			when: MachineTrigger::AfterTicks { ticks: 0 },
			next: "ready".into(),
		}];
		assert!(invalid.validate().is_err());
		invalid = valid.clone();
		invalid.states.push(invalid.states[0].clone());
		assert!(invalid.validate().is_err());
		invalid = valid.clone();
		invalid.states[0].id = "bad id".into();
		assert!(invalid.validate().is_err());
		let mut json = serde_json::to_value(valid).unwrap();
		json["runtime_state"] = serde_json::json!("ready");
		assert!(serde_json::from_value::<MachineDef>(json).is_err());
	}
	#[test]
	fn remap_changes_typed_references_without_rewriting_state_or_input_ids() {
		let mut def = definition(vec![state("same", vec![signal(Some("same"), "same")])]);
		def.inputs.push(SensorInput {
			id: "same".into(),
			sensor: reference("same").entity,
			actor: reference("actor").entity,
			signal: Signal::Activated,
		});
		def.states[0].on_enter = vec![
			MachineAction::Emit {
				target: reference("same"),
				signal: Signal::Reset,
			},
			MachineAction::SetVisible {
				entity: reference("same").entity,
				visible: false,
			},
		];
		def.remap(&HashMap::from([(
			reference("same").entity,
			reference("copy").entity,
		)]));
		assert_eq!(def.initial, "same");
		assert_eq!(def.states[0].id, "same");
		assert_eq!(def.inputs[0].id, "same");
		assert_eq!(def.states[0].transitions[0].next, "same");
		assert!(
			def.entity_references()
				.iter()
				.all(|key| key.as_str() != "same")
		);
	}
	#[test]
	fn byte_budget_counts_serialized_nested_definitions() {
		let target = reference(&"x".repeat(256));
		let states = (0..32)
			.map(|index| StateDef {
				id: format!("state-{index}"),
				on_enter: vec![
					MachineAction::Emit {
						target: target.clone(),
						signal: Signal::Activated
					};
					8
				],
				transitions: vec![],
			})
			.collect();
		let def = definition(states);
		assert!(serde_json::to_vec(&def).unwrap().len() > MAX_DEFINITION_BYTES);
		assert!(def.validate().unwrap_err().contains("64 KiB"));
	}
}
