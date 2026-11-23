//! Prepared instances and the narrow API used by compiled project scripts.

use std::{
	collections::{BTreeMap, BTreeSet},
	sync::Arc,
};

use crate::{
	EntityKey,
	components::{
		ComponentId, ScriptComponent, ScriptDescriptor, ScriptIssue, ScriptReference,
		ScriptReferenceKind, ScriptSettings,
	},
	runtime_lifecycle::{RuntimeEntityHandle, RuntimeLifecycle},
	scene_data::{EntityData, TransformData},
	triggers::{TriggerEvent, TriggerPhase},
	world::RuntimeWorld,
};

use super::{ActionWriter, InputFrame, SimulationAction};

const MAX_ACTIVE_SCRIPTS: usize = 4096;
const MAX_TRIGGER_DELIVERIES_PER_STEP: usize = 4096;

#[derive(Clone, Debug)]
pub struct ObjectScriptTrigger {
	pub sensor: RuntimeEntityHandle,
	pub other: RuntimeEntityHandle,
	pub phase: TriggerPhase,
}

#[derive(Clone, Copy, Debug)]
pub struct ObjectScriptAction<'a> {
	pub id: &'a str,
	pub pressed: bool,
}

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

	fn on_trigger(
		&mut self,
		_context: &ObjectScriptContext<'_>,
		_event: &ObjectScriptTrigger,
		_commands: &mut ObjectScriptCommands<'_, '_>,
	) -> Result<(), String> {
		Ok(())
	}

	fn on_action(
		&mut self,
		_context: &ObjectScriptContext<'_>,
		_event: ObjectScriptAction<'_>,
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
	receive_local_actions: bool,
	sensors: BTreeSet<EntityKey>,
}

impl Instance {
	fn prepare(
		world: &RuntimeWorld,
		lifecycle: &RuntimeLifecycle,
		entity: &EntityData,
		attachment: &ScriptComponent,
		descriptor: &ScriptDescriptor,
		registration: ObjectScriptRegistration,
	) -> Result<Self, ScriptIssue> {
		descriptor.validate_attachment(
			&attachment.settings,
			|key| {
				world
					.entity_index(key)
					.map(|index| &world.data().entities[index])
			},
			|_| false,
		)?;
		let invalid = |message: &str| ScriptIssue::InvalidConfiguration(message.into());
		let mut references = BTreeMap::new();
		let mut sensors = BTreeSet::new();
		for field in &descriptor.references {
			let reference = &attachment.settings.references[&field.key];
			let ScriptReference::Entity { entity: target } = reference else {
				return Err(invalid("Script reference must be bound to an entity"));
			};
			let handle = lifecycle
				.handle(target)
				.ok_or_else(|| invalid("Script reference runtime handle is missing"))?;
			references.insert(field.key.clone(), handle);
			if field.requires == ScriptReferenceKind::TriggerCollider {
				sensors.insert(target.clone());
			}
		}
		Ok(Self {
			owner: lifecycle
				.handle(&entity.key)
				.ok_or_else(|| invalid("Script owner runtime handle is missing"))?,
			instance: (registration.instantiate)(&attachment.settings)
				.map_err(ScriptIssue::InvalidConfiguration)?,
			started: false,
			references: Arc::new(references),
			write_owner_transform: descriptor.write_owner_transform,
			receive_local_actions: descriptor.receive_local_actions,
			sensors,
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
	subscribers: Arc<BTreeMap<EntityKey, BTreeSet<(EntityKey, ComponentId)>>>,
	pending: BTreeMap<(EntityKey, ComponentId), Vec<ObjectScriptTrigger>>,
	local_owner: Option<RuntimeEntityHandle>,
	held_local_actions: BTreeSet<String>,
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
							ScriptIssue::MissingRegistration(attachment.settings.script_id.clone())
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
				let instance =
					prepare().map_err(|error| error.at_attachment(&entity.key, &attachment.id))?;
				for sensor in &instance.sensors {
					Arc::make_mut(&mut self.subscribers)
						.entry(sensor.clone())
						.or_default()
						.insert(address.clone());
				}
				self.active.insert(address, instance);
			}
		}
		Ok(())
	}

	pub fn needs_tick(&self, input: &InputFrame) -> bool {
		!self.pending.is_empty()
			|| self
				.held_local_actions
				.iter()
				.any(|id| !input.action_held(id))
			|| self
				.active
				.values()
				.any(|instance| !instance.started || instance.instance.needs_update())
	}

	/// Stage directed edges for the following fixed step and the same candidate commit.
	pub fn stage_triggers(
		&mut self,
		lifecycle: &RuntimeLifecycle,
		events: &[TriggerEvent],
	) -> Result<(), String> {
		let mut deliveries = 0;
		for event in events {
			let Some(targets) = self.subscribers.get(&event.sensor) else {
				continue;
			};
			let (Some(sensor), Some(other)) = (
				lifecycle.handle(&event.sensor),
				lifecycle.handle(&event.other),
			) else {
				continue;
			};
			let edge = ObjectScriptTrigger {
				sensor,
				other,
				phase: event.phase,
			};
			for address in targets {
				if deliveries == MAX_TRIGGER_DELIVERIES_PER_STEP {
					return Err("Script trigger delivery budget exceeded".into());
				}
				self.pending
					.entry(address.clone())
					.or_default()
					.push(edge.clone());
				deliveries += 1;
			}
		}
		Ok(())
	}

	pub fn plan_tick(
		&mut self,
		world: &RuntimeWorld,
		lifecycle: &RuntimeLifecycle,
		tick: u64,
		step_seconds: f32,
		input: &InputFrame,
		local_owner: Option<&EntityKey>,
		actions: &mut ActionWriter<'_>,
	) -> Result<(), String> {
		let local_owner = local_owner
			.and_then(|key| lifecycle.handle(key))
			.filter(|owner| {
				self.active
					.values()
					.any(|state| state.receive_local_actions && state.owner == *owner)
			});
		if self.local_owner != local_owner {
			self.held_local_actions.clear();
			self.local_owner = local_owner;
		}
		let held: BTreeSet<String> = if self.local_owner.is_some() {
			input.game_actions.ids.iter().cloned().collect()
		} else {
			BTreeSet::new()
		};
		let mut presses: Vec<_> = if self.local_owner.is_some() {
			input.action_presses.iter().map(String::as_str).collect()
		} else {
			Vec::new()
		};
		presses.sort_unstable();
		let mut releases: BTreeSet<_> = self
			.held_local_actions
			.difference(&held)
			.map(String::as_str)
			.collect();
		releases.extend(presses.iter().copied().filter(|id| !held.contains(*id)));
		let pending = std::mem::take(&mut self.pending);
		for (address, state) in &mut self.active {
			let (key, attachment) = address;
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
			if let Some(events) = pending.get(address) {
				for event in events {
					if lifecycle.validate_handle(&event.sensor).is_ok()
						&& lifecycle.validate_handle(&event.other).is_ok()
					{
						state
							.instance
							.on_trigger(&context, event, &mut commands)
							.map_err(|cause| error("on_trigger", cause))?;
					}
				}
			}
			if state.receive_local_actions && Some(&state.owner) == self.local_owner.as_ref() {
				for event in presses
					.iter()
					.map(|&id| ObjectScriptAction { id, pressed: true })
					.chain(
						releases
							.iter()
							.map(|&id| ObjectScriptAction { id, pressed: false }),
					) {
					state
						.instance
						.on_action(&context, event, &mut commands)
						.map_err(|cause| error("on_action", cause))?;
				}
			}
			if state.instance.needs_update() {
				state
					.instance
					.on_fixed_update(&context, &mut commands)
					.map_err(|cause| error("on_fixed_update", cause))?;
			}
		}
		self.held_local_actions = held;
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
				for sensor in &instance.sensors {
					let subscribers = Arc::make_mut(&mut self.subscribers);
					if let Some(listeners) = subscribers.get_mut(sensor) {
						listeners.remove(&address);
						if listeners.is_empty() {
							subscribers.remove(sensor);
						}
					}
				}
				self.pending.remove(&address);
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
