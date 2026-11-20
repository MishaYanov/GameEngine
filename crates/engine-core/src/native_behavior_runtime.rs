//! Native component instances plan bounded operations; state commits with the tick.
use crate::{
	components::NativeBehaviorSpec,
	runtime_lifecycle::RuntimeLifecycle,
	scene_data::MaterialOverrideData,
	simulation_session::{ActionWriter, SimulationAction},
	triggers::TriggerView,
	world::RuntimeWorld,
};

pub(crate) fn needs_tick(world: &RuntimeWorld) -> bool {
	world.data().entities.iter().any(|entity| {
		entity.native_behaviors.iter().any(|component| {
			component.enabled
				&& matches!(component.behavior, NativeBehaviorSpec::TimedSpawner { .. })
		})
	})
}

pub(crate) fn plan(
	world: &RuntimeWorld,
	overlaps: TriggerView<'_>,
	state: &mut RuntimeLifecycle,
	output: &mut ActionWriter<'_>,
) -> Result<(), String> {
	let mut count = 0usize;
	for (index, entity) in world.data().entities.iter().enumerate() {
		for component in &entity.native_behaviors {
			if !component.enabled {
				continue;
			}
			count += 1;
			if count > 4096 {
				return Err("Active native behavior budget exceeded".into());
			}
			let key = (entity.key.clone(), component.id.clone());
			let mut instance = state.states.get(&key).cloned().unwrap_or_default();
			match &component.behavior {
				NativeBehaviorSpec::KeyDoor {
					actor,
					key_trigger,
					door,
					key_visual,
				} => {
					if !instance.activated && overlaps.contains(key_trigger, actor) {
						output.push(SimulationAction::SetCollisionEnabled {
							entity: door.clone(),
							enabled: false,
						})?;
						output.push(SimulationAction::SetMaterial {
							entity: door.clone(),
							material: Some(MaterialOverrideData {
								base_color_factor: [0.1, 0.9, 0.25, 1.0],
							}),
						})?;
						output.push(SimulationAction::SetCollisionEnabled {
							entity: key_trigger.clone(),
							enabled: false,
						})?;
						if let Some(visual) = key_visual {
							output.push(SimulationAction::SetVisible {
								entity: visual.clone(),
								visible: false,
							})?;
						}
						instance.activated = true;
					}
				}
				NativeBehaviorSpec::Collectible {
					actor,
					trigger,
					visual,
					despawn_owner,
				} => {
					if !instance.activated && overlaps.contains(trigger, actor) {
						if *despawn_owner && state.instance_for(&entity.key).is_some() {
							output.push(SimulationAction::Despawn {
								handle: state
									.handle(&entity.key)
									.ok_or("Collectible owner is unavailable")?,
							})?;
						} else {
							output.push(SimulationAction::SetCollisionEnabled {
								entity: trigger.clone(),
								enabled: false,
							})?;
							if let Some(visual) = visual {
								output.push(SimulationAction::SetVisible {
									entity: visual.clone(),
									visible: false,
								})?;
							}
						}
						instance.activated = true;
					}
				}
				NativeBehaviorSpec::TimedSpawner {
					prefab_path,
					interval_ticks,
					max_alive,
					lifetime_ticks,
					spawn_offset,
					bindings,
				} => {
					instance.elapsed_ticks = instance
						.elapsed_ticks
						.saturating_add(1)
						.min(*interval_ticks);
					if instance.elapsed_ticks >= *interval_ticks
						&& state.alive_for(&entity.key, &component.id) < *max_alive as usize
						&& !state.has_request(&entity.key, &component.id)
					{
						let position = (world.world_matrices()[index].w_axis.truncate()
							+ glam::Vec3::from_array(*spawn_offset))
						.to_array();
						state.enqueue(
							&entity.key,
							&component.id,
							prefab_path,
							bindings,
							position,
							*lifetime_ticks,
						)?;
						instance.elapsed_ticks = 0;
					}
				}
			}
			state.states.insert(key, instance);
		}
	}
	Ok(())
}
