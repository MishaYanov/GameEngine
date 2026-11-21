use super::{MachineAction, MachineDef, MachineTrigger};
use crate::{
	EntityKey, components::NativeBehaviorSpec, gameplay_events::ComponentRef,
	scene_data::EntityData,
};
use std::collections::{HashMap, HashSet};

impl MachineDef {
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
