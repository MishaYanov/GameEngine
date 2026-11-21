use super::{MAX_TRANSITIONS_PER_STEP, MachineAction, MachineDef, MachineTrigger, SensorInput};
use crate::gameplay_events::{ComponentRef, Envelope, Signal};
use std::collections::BTreeMap;

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
