//! Deterministic adapters between authored machines and a staged simulation tick.
use crate::{
	components::NativeBehaviorSpec,
	gameplay_events::{ComponentEndpoint, ComponentRef, Envelope},
	runtime_lifecycle::RuntimeLifecycle,
	simulation_session::{ActionWriter, SimulationAction},
	state_machine::{self, MachineAction},
	triggers::{TriggerEvent, TriggerPhase},
	world::RuntimeWorld,
};
use std::{
	collections::{BTreeMap, HashSet},
	sync::Arc,
};

fn endpoint(
	world: &RuntimeWorld,
	lifecycle: &RuntimeLifecycle,
	reference: &ComponentRef,
) -> Result<ComponentEndpoint, String> {
	let index = world
		.entity_index(&reference.entity)
		.ok_or("Machine event entity no longer exists")?;
	if !world.data().entities[index]
		.native_behaviors
		.iter()
		.any(|component| {
			component.id == reference.component
				&& matches!(component.behavior, NativeBehaviorSpec::StateMachine { .. })
		}) {
		return Err("Machine event target attachment is missing or incompatible".into());
	}
	Ok(ComponentEndpoint {
		owner: lifecycle
			.handle(&reference.entity)
			.ok_or("Machine event owner no longer exists")?,
		component: reference.component.clone(),
	})
}
fn live(world: &RuntimeWorld, lifecycle: &RuntimeLifecycle, endpoint: &ComponentEndpoint) -> bool {
	lifecycle.validate_handle(&endpoint.owner).is_ok()
		&& world
			.entity_index(&endpoint.owner.key)
			.is_some_and(|index| {
				world.data().entities[index]
					.native_behaviors
					.iter()
					.any(|component| {
						component.id == endpoint.component
							&& component.enabled && matches!(
							component.behavior,
							NativeBehaviorSpec::StateMachine { .. }
						)
					})
			})
}

/// Inbox is frozen before routes run. Routes for every machine precede all entry
/// effects; every emission is queued for the next fixed step, including self sends.
pub(crate) fn plan(
	world: &RuntimeWorld,
	lifecycle: &mut RuntimeLifecycle,
	step: u64,
	triggers: &[TriggerEvent],
	delivered: &mut usize,
	onsets: &mut Vec<crate::scene_audio::CommittedAudioOnset>,
	output: &mut ActionWriter<'_>,
) -> Result<usize, String> {
	let mut inboxes = BTreeMap::<_, Vec<Envelope>>::new();
	for event in lifecycle.events.take_due(step, delivered)? {
		if live(world, lifecycle, &event.source) && live(world, lifecycle, &event.target) {
			crate::scene_audio::record_onset(
				world,
				&event.target.owner.key,
				step,
				crate::scene_audio::AudioOnsetCause::GameplaySignal {
					signal: event.signal,
				},
				onsets,
			);
			inboxes
				.entry((
					event.target.owner.key.clone(),
					event.target.component.clone(),
				))
				.or_default()
				.push(event);
		} else {
			lifecycle.stale_events_discarded = lifecycle
				.stale_events_discarded
				.saturating_add(1)
				.min(9_007_199_254_740_991);
		}
	}
	let machines = Arc::clone(&lifecycle.machines);
	let mut emitted = 0;
	route_sensors(world, lifecycle, step, triggers, &mut emitted)?;
	for (address, definition) in machines.iter() {
		let current = lifecycle
			.machine_states
			.get(address)
			.cloned()
			.unwrap_or_default();
		let evaluated = state_machine::evaluate(
			definition,
			&current,
			step,
			inboxes.get(address).map(Vec::as_slice).unwrap_or(&[]),
		)?;
		let owner = endpoint(
			world,
			lifecycle,
			&ComponentRef {
				entity: address.0.clone(),
				component: address.1.clone(),
			},
		)?;
		for effect in evaluated.effects {
			match effect {
				MachineAction::Emit { target, signal } => {
					let target = endpoint(world, lifecycle, &target)?;
					lifecycle.events.emit_next(
						step,
						&mut emitted,
						Envelope {
							source: owner.clone(),
							target,
							signal,
						},
					)?;
				}
				MachineAction::SetVisible { entity, visible } => {
					output.push(SimulationAction::SetVisible { entity, visible })?
				}
				MachineAction::SetCollisionEnabled { entity, enabled } => {
					output.push(SimulationAction::SetCollisionEnabled { entity, enabled })?
				}
				MachineAction::DespawnOwner => output.push(SimulationAction::Despawn {
					handle: owner.owner.clone(),
				})?,
			}
		}
		lifecycle
			.machine_states
			.insert(address.clone(), evaluated.next);
	}
	Ok(emitted)
}

fn route_sensors(
	world: &RuntimeWorld,
	lifecycle: &mut RuntimeLifecycle,
	step: u64,
	triggers: &[TriggerEvent],
	emitted: &mut usize,
) -> Result<(), String> {
	// Lookup only: emissions still follow sorted machine/input order. Duplicate
	// directed Enter edges represent one accepted contact for each input route.
	let entered: HashSet<_> = triggers
		.iter()
		.filter(|event| event.phase == TriggerPhase::Enter)
		.map(|event| (&event.sensor, &event.other))
		.collect();
	let machines = Arc::clone(&lifecycle.machines);
	for ((key, id), definition) in machines.iter() {
		let owner = endpoint(
			world,
			lifecycle,
			&ComponentRef {
				entity: key.clone(),
				component: id.clone(),
			},
		)?;
		for input in &definition.inputs {
			if entered.contains(&(&input.sensor, &input.actor)) {
				lifecycle.events.emit_next(
					step,
					emitted,
					Envelope {
						source: owner.clone(),
						target: owner.clone(),
						signal: input.signal,
					},
				)?;
			}
		}
	}
	Ok(())
}

/// Controller authority translates accepted local edges into next-step self
/// signals for the body's enabled machines. The sender is each recipient's
/// machine endpoint; a controller attachment is never forged as a machine.
pub(crate) fn route_motion(
	world: &RuntimeWorld,
	lifecycle: &mut RuntimeLifecycle,
	characters: &crate::character_controller::CharacterRuntime,
	step: u64,
	emitted: &mut usize,
) -> Result<(), String> {
	let machines = Arc::clone(&lifecycle.machines);
	for key in characters.keys() {
		let state = characters
			.state(key)
			.ok_or("Character state disappeared while routing motion")?;
		if state.tick != step {
			continue;
		}
		for (matches, signal) in [
			(
				state.jumped_at_tick == Some(step),
				crate::gameplay_events::Signal::Jumped,
			),
			(
				state.landed_at_tick == Some(step),
				crate::gameplay_events::Signal::Landed,
			),
		] {
			if !matches {
				continue;
			}
			for (owner, id) in machines.keys().filter(|(owner, _)| owner == key) {
				let target = endpoint(
					world,
					lifecycle,
					&ComponentRef {
						entity: owner.clone(),
						component: id.clone(),
					},
				)?;
				lifecycle.events.emit_next(
					step,
					emitted,
					Envelope {
						source: target.clone(),
						target,
						signal,
					},
				)?;
			}
		}
	}
	Ok(())
}

#[cfg(test)]
#[path = "../tests/unit/state_machine_runtime.rs"]
mod tests;
