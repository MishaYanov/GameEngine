//! Prepared instances and the narrow API used by compiled project scripts.

use std::{collections::BTreeMap, sync::Arc};

use crate::{
	EntityKey,
	components::{
		ComponentId, ScriptComponent, ScriptDescriptor, ScriptReference, ScriptReferenceKind,
		ScriptSettings,
	},
	runtime_lifecycle::{RuntimeEntityHandle, RuntimeLifecycle},
	scene_data::{EntityData, TransformData},
	world::RuntimeWorld,
};

use super::{ActionWriter, SimulationAction};

const MAX_ACTIVE_SCRIPTS: usize = 4096;

pub trait ObjectScript: Send {
	/// Copy execution state for a candidate frame. The copy must own its mutable state.
	fn clone_instance(&self) -> Box<dyn ObjectScript>;

	fn needs_update(&self) -> bool {
		true
	}

	fn on_start(
		&mut self,
		_context: &ObjectScriptContext<'_>,
		_commands: &mut ObjectScriptCommands<'_, '_>,
	) -> Result<(), String> {
		Ok(())
	}

	fn on_fixed_update(
		&mut self,
		_context: &ObjectScriptContext<'_>,
		_commands: &mut ObjectScriptCommands<'_, '_>,
	) -> Result<(), String> {
		Ok(())
	}

	/// Called when the accepted instance retires. No world mutation is available here.
	fn on_stop(&mut self) {}
}

impl Clone for Box<dyn ObjectScript> {
	fn clone(&self) -> Self {
		self.clone_instance()
	}
}

#[derive(Clone, Copy)]
pub struct ObjectScriptRegistration {
	pub descriptor: fn() -> ScriptDescriptor,
	pub instantiate: fn(&ScriptSettings) -> Result<Box<dyn ObjectScript>, String>,
}

pub struct ObjectScriptContext<'a> {
	world: &'a RuntimeWorld,
	lifecycle: &'a RuntimeLifecycle,
	owner: &'a RuntimeEntityHandle,
	references: &'a BTreeMap<String, RuntimeEntityHandle>,
	tick: u64,
	step_seconds: f32,
}

impl ObjectScriptContext<'_> {
	pub fn owner(&self) -> &RuntimeEntityHandle {
		self.owner
	}

	pub fn reference(&self, field: &str) -> Option<&RuntimeEntityHandle> {
		self.references
			.get(field)
			.filter(|handle| self.lifecycle.validate_handle(handle).is_ok())
	}

	pub fn transform(&self, target: &RuntimeEntityHandle) -> Result<&TransformData, String> {
		if target != self.owner && !self.references.values().any(|handle| handle == target) {
			return Err("Script may only read its owner or declared references".into());
		}
		self.lifecycle.validate_handle(target)?;
		let index = self
			.world
			.entity_index(&target.key)
			.ok_or("Script target is missing")?;
		Ok(&self.world.data().entities[index].transform)
	}

	pub fn tick(&self) -> u64 {
		self.tick
	}

	pub fn step_seconds(&self) -> f32 {
		self.step_seconds
	}
}

pub struct ObjectScriptCommands<'a, 'actions> {
	context: &'a ObjectScriptContext<'a>,
	actions: &'a mut ActionWriter<'actions>,
	write_owner_transform: bool,
}

impl ObjectScriptCommands<'_, '_> {
	/// Allocate a validated transform action for this attachment's owner.
	pub fn set_local_yaw(&mut self, radians: f32) -> Result<(), String> {
		if !self.write_owner_transform || !radians.is_finite() {
			return Err("Script cannot write this local rotation".into());
		}
		let mut transform = self.context.transform(self.context.owner)?.clone();
		transform.rotation[1] = radians;
		self.actions.push(SimulationAction::SetTransform {
			entity: self.context.owner.key.clone(),
			transform,
		})
	}
}

#[derive(Clone)]
struct Instance {
	owner: RuntimeEntityHandle,
	instance: Box<dyn ObjectScript>,
	started: bool,
	references: Arc<BTreeMap<String, RuntimeEntityHandle>>,
	write_owner_transform: bool,
}

impl Instance {
	fn prepare(
		world: &RuntimeWorld,
		lifecycle: &RuntimeLifecycle,
		entity: &EntityData,
		attachment: &ScriptComponent,
		descriptor: &ScriptDescriptor,
		registration: ObjectScriptRegistration,
	) -> Result<Self, String> {
		descriptor.validate_settings(&attachment.settings)?;
		let mut references = BTreeMap::new();
		for field in &descriptor.references {
			let reference = &attachment.settings.references[&field.key];
			let ScriptReference::Entity { entity: target } = reference else {
				return Err(format!("Bind the {} slot before Play", field.label));
			};
			let index = world
				.entity_index(target)
				.ok_or_else(|| format!("Missing {} entity {target:?}", field.label))?;
			let source = &world.data().entities[index];
			if field.requires == ScriptReferenceKind::TriggerCollider
				&& !(source.collision_enabled
					&& source
						.collider
						.as_ref()
						.is_some_and(|collider| collider.interaction().sensor))
			{
				return Err(format!(
					"{} must have an enabled trigger collider",
					field.label
				));
			}
			let handle = lifecycle
				.handle(target)
				.ok_or("Script reference runtime handle is missing")?;
			references.insert(field.key.clone(), handle);
		}
		Ok(Self {
			owner: lifecycle
				.handle(&entity.key)
				.ok_or("Script owner runtime handle is missing")?,
			instance: (registration.instantiate)(&attachment.settings)?,
			started: false,
			references: Arc::new(references),
			write_owner_transform: descriptor.write_owner_transform,
		})
	}

	fn stop(mut self) {
		if self.started {
			self.instance.on_stop();
		}
	}
}

#[derive(Clone, Default)]
pub(super) struct ObjectScripts {
	registrations: Arc<BTreeMap<String, (ScriptDescriptor, ObjectScriptRegistration)>>,
	active: BTreeMap<(EntityKey, ComponentId), Instance>,
}

impl ObjectScripts {
	pub fn prepare(
		world: &RuntimeWorld,
		lifecycle: &RuntimeLifecycle,
		registrations: &[ObjectScriptRegistration],
	) -> Result<Self, String> {
		let mut scripts = Self::default();
		for registration in registrations {
			let descriptor = (registration.descriptor)();
			if Arc::make_mut(&mut scripts.registrations)
				.insert(descriptor.id.clone(), (descriptor, *registration))
				.is_some()
			{
				return Err("Duplicate object script registration".into());
			}
		}
		scripts.admit(world, lifecycle, &world.data().entities)?;
		Ok(scripts)
	}

	pub fn admit(
		&mut self,
		world: &RuntimeWorld,
		lifecycle: &RuntimeLifecycle,
		entities: &[EntityData],
	) -> Result<(), String> {
		for entity in entities {
			for attachment in entity
				.scripts
				.iter()
				.filter(|attachment| attachment.enabled)
			{
				if self.active.len() >= MAX_ACTIVE_SCRIPTS {
					return Err("Active object script count exceeds its limit".into());
				}
				let address = (entity.key.clone(), attachment.id.clone());
				let prepare = || {
					let (descriptor, registration) = self
						.registrations
						.get(&attachment.settings.script_id)
						.ok_or_else(|| {
							format!("Unknown script {}", attachment.settings.script_id)
						})?;
					Instance::prepare(
						world,
						lifecycle,
						entity,
						attachment,
						descriptor,
						*registration,
					)
				};
				let instance = prepare().map_err(|error| {
					format!(
						"Object {} Script {}: {error}",
						entity.key.as_str(),
						attachment.id.as_str()
					)
				})?;
				self.active.insert(address, instance);
			}
		}
		Ok(())
	}

	pub fn needs_tick(&self) -> bool {
		self.active
			.values()
			.any(|instance| !instance.started || instance.instance.needs_update())
	}

	pub fn plan_tick(
		&mut self,
		world: &RuntimeWorld,
		lifecycle: &RuntimeLifecycle,
		tick: u64,
		step_seconds: f32,
		actions: &mut ActionWriter<'_>,
	) -> Result<(), String> {
		for ((key, attachment), state) in &mut self.active {
			let context = ObjectScriptContext {
				world,
				lifecycle,
				owner: &state.owner,
				references: &state.references,
				tick,
				step_seconds,
			};
			let mut commands = ObjectScriptCommands {
				context: &context,
				actions,
				write_owner_transform: state.write_owner_transform,
			};
			let error = |hook: &str, cause| {
				format!(
					"Object {} Script {} {hook}: {cause}",
					key.as_str(),
					attachment.as_str()
				)
			};
			if !state.started {
				state
					.instance
					.on_start(&context, &mut commands)
					.map_err(|cause| error("on_start", cause))?;
				state.started = true;
			}
			if state.instance.needs_update() {
				state
					.instance
					.on_fixed_update(&context, &mut commands)
					.map_err(|cause| error("on_fixed_update", cause))?;
			}
		}
		Ok(())
	}

	pub fn prune(&mut self, lifecycle: &RuntimeLifecycle, retirement: &mut Self) {
		let removed = self
			.active
			.iter()
			.filter_map(|(address, state)| {
				lifecycle
					.validate_handle(&state.owner)
					.is_err()
					.then(|| address.clone())
			})
			.collect::<Vec<_>>();
		for address in removed {
			if let Some(instance) = self.active.remove(&address) {
				retirement.active.insert(address, instance);
			}
		}
	}

	pub fn stop(&mut self) {
		for (_, state) in std::mem::take(&mut self.active) {
			state.stop();
		}
	}
}
