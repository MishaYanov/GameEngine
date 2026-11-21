//! Upright, root-level box controllers. Scene settings are separate from tick state.
mod motion;
use motion::solve;

use crate::{
	EntityKey,
	collision::CollisionWorld,
	components::ComponentId,
	project::ProjectData,
	scene_data::{ColliderData, EntityData},
	world::{RuntimeWorld, WorldChange},
};
use glam::Vec3;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

pub const MAX_CONTROLLERS: usize = 64;
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CharacterControllerSettings {
	pub walk_speed: f32,
	pub ground_acceleration: f32,
	pub air_acceleration: f32,
	pub gravity: f32,
	pub max_fall_speed: f32,
	pub jump_speed: f32,
	pub ground_probe: f32,
	pub step_height: f32,
	pub coyote_ticks: u32,
	pub jump_buffer_ticks: u32,
	pub jump_action: String,
}
impl Default for CharacterControllerSettings {
	fn default() -> Self {
		Self {
			walk_speed: 4.,
			ground_acceleration: 30.,
			air_acceleration: 8.,
			gravity: 20.,
			max_fall_speed: 30.,
			jump_speed: 7.,
			ground_probe: 0.02,
			step_height: 0.25,
			coyote_ticks: 5,
			jump_buffer_ticks: 5,
			jump_action: "jump".into(),
		}
	}
}
impl CharacterControllerSettings {
	pub fn validate(&self) -> Result<(), String> {
		let values = [
			(self.walk_speed, 0., 100.),
			(self.ground_acceleration, 0.01, 1000.),
			(self.air_acceleration, 0., 1000.),
			(self.gravity, 0.01, 1000.),
			(self.max_fall_speed, 0.01, 1000.),
			(self.jump_speed, 0.01, 100.),
			(self.ground_probe, 0.001, 0.1),
			(self.step_height, 0., 1.),
		];
		if values
			.iter()
			.any(|(v, lo, hi)| !v.is_finite() || v < lo || v > hi)
			|| self.coyote_ticks > 120
			|| self.jump_buffer_ticks > 120
		{
			return Err(
				"Character settings require finite bounded values and jump windows <=120 ticks"
					.into(),
			);
		}
		if self.jump_action.is_empty()
			|| self.jump_action.len() > 64
			|| !self
				.jump_action
				.bytes()
				.all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
		{
			return Err("Character jump action requires a bounded action ID".into());
		}
		Ok(())
	}
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CharacterControllerComponent {
	pub id: ComponentId,
	pub enabled: bool,
	pub settings: CharacterControllerSettings,
}
#[derive(Debug, Clone, Default, Serialize)]
pub struct CharacterState {
	pub velocity: [f32; 3],
	pub grounded: bool,
	pub support: Option<EntityKey>,
	pub planar_speed: f32,
	pub tick: u64,
	pub jumped_at_tick: Option<u64>,
	pub landed_at_tick: Option<u64>,
	pub impact_speed: f32,
	coyote_until: Option<u64>,
	jump_buffer_until: Option<u64>,
	jump_available: bool,
	initialized: bool,
}
#[derive(Debug, Clone, Serialize)]
pub struct CommittedMotion {
	pub entity: crate::runtime_lifecycle::RuntimeEntityHandle,
	pub tick: u64,
	pub grounded: bool,
	pub planar_speed: f32,
	pub vertical_speed: f32,
	pub jumped_at_tick: Option<u64>,
	pub landed_at_tick: Option<u64>,
}
impl CharacterState {
	pub fn motion(&self, entity: crate::runtime_lifecycle::RuntimeEntityHandle) -> CommittedMotion {
		CommittedMotion {
			entity,
			tick: self.tick,
			grounded: self.grounded,
			planar_speed: self.planar_speed,
			vertical_speed: self.velocity[1],
			jumped_at_tick: self.jumped_at_tick,
			landed_at_tick: self.landed_at_tick,
		}
	}
}
#[derive(Clone, Default, Serialize)]
struct CharacterInput {
	movement: [f32; 2],
	held: bool,
	pressed: bool,
}
#[derive(Clone, Default)]
pub(crate) struct CharacterRuntime {
	states: BTreeMap<EntityKey, CharacterState>,
	inputs: BTreeMap<EntityKey, CharacterInput>,
}
impl CharacterRuntime {
	pub fn new(world: &RuntimeWorld, project: &ProjectData) -> Result<Self, String> {
		let mut value = Self::default();
		value.reconcile(world, project)?;
		Ok(value)
	}
	pub fn reconcile(&mut self, world: &RuntimeWorld, project: &ProjectData) -> Result<(), String> {
		let mut keys = Vec::new();
		for (index, entity) in world.data().entities.iter().enumerate() {
			let Some(component) = entity.character_controller.as_ref().filter(|c| c.enabled) else {
				continue;
			};
			component.settings.validate()?;
			if keys.len() == MAX_CONTROLLERS {
				return Err("Character controller count exceeds 64".into());
			}
			if entity.parent.is_some()
				|| entity.transform.rotation.iter().any(|v| v.abs() > 1e-6)
				|| entity
					.transform
					.scale
					.iter()
					.any(|v| (*v - 1.).abs() > 1e-6)
			{
				return Err("Character body must be an upright unit-scale scene root; Unpack then detach linked members".into());
			}
			let Some(ColliderData::Box {
				half_extents,
				interaction,
				..
			}) = &entity.collider
			else {
				return Err("Character body requires an enabled non-sensor box collider".into());
			};
			if !entity.collision_enabled
				|| interaction.sensor
				|| component.settings.ground_probe
					>= half_extents.iter().copied().fold(f32::INFINITY, f32::min)
				|| component.settings.step_height >= half_extents[1] * 2.
			{
				return Err("Character collider/probe/step dimensions are unsupported".into());
			}
			for child in world.subtree_indices(index)? {
				if child != index
					&& world.data().entities[child].collider.is_some()
					&& world.data().entities[child].collision_enabled
				{
					return Err("Character visual descendants cannot have enabled colliders".into());
				}
			}
			if !project.input_profile.as_ref().is_some_and(|p| {
				p.actions
					.iter()
					.any(|a| a.id == component.settings.jump_action)
			}) {
				return Err(format!(
					"Character jump action {:?} is absent from the input profile",
					component.settings.jump_action
				));
			}
			keys.push(entity.key.clone());
		}
		self.states.retain(|key, _| keys.contains(key));
		self.inputs.retain(|key, _| keys.contains(key));
		for key in keys {
			self.states.entry(key.clone()).or_default();
			self.inputs.entry(key).or_default();
		}
		Ok(())
	}
	pub fn keys(&self) -> impl Iterator<Item = &EntityKey> {
		self.states.keys()
	}
	pub fn contains(&self, key: &EntityKey) -> bool {
		self.states.contains_key(key)
	}
	/// Keep the admitted single-body contract intact when modules toggle colliders.
	pub fn forbids_collider_change(
		&self,
		world: &RuntimeWorld,
		key: &EntityKey,
		enabled: bool,
	) -> bool {
		if self.contains(key) {
			return !enabled;
		}
		if !enabled {
			return false;
		}
		let mut current = world.entity_index(key);
		while let Some(index) = current {
			let Some(parent) = &world.data().entities[index].parent else {
				break;
			};
			if self.contains(parent) {
				return true;
			}
			current = world.entity_index(parent);
		}
		false
	}
	pub fn state(&self, key: &EntityKey) -> Option<&CharacterState> {
		self.states.get(key)
	}
	/// Complete future-affecting controller state in stable key order. Authored
	/// IDs/settings are part of the separately hashed runtime scene document.
	pub fn replay_state(&self) -> serde_json::Value {
		serde_json::json!({"states": self.states, "inputs": self.inputs})
	}
	pub fn needs_tick(&self) -> bool {
		!self.states.is_empty()
	}
	pub fn clear_input(&mut self) {
		for input in self.inputs.values_mut() {
			*input = CharacterInput::default();
		}
	}
	pub fn set_input(
		&mut self,
		key: &EntityKey,
		movement: [f32; 2],
		jump: bool,
	) -> Result<(), String> {
		if !movement.iter().all(|v| v.is_finite()) {
			return Err("Character input must be finite".into());
		}
		let input = self
			.inputs
			.get_mut(key)
			.ok_or("Input target has no enabled character controller")?;
		let length = (f64::from(movement[0]).powi(2) + f64::from(movement[1]).powi(2))
			.sqrt()
			.max(1.);
		input.movement = movement.map(|v| (f64::from(v) / length) as f32);
		input.pressed |= jump && !input.held;
		input.held = jump;
		Ok(())
	}
	pub fn set_movement(&mut self, key: &EntityKey, movement: [f32; 2]) -> Result<(), String> {
		let held = self
			.inputs
			.get(key)
			.ok_or("Character input target missing")?
			.held;
		self.set_input(key, movement, held)
	}
	pub fn sample_keyboard(
		&mut self,
		world: &RuntimeWorld,
		target: Option<&EntityKey>,
		movement: [f32; 2],
		actions: &crate::input::HeldGameActions,
	) -> Result<(), String> {
		let keys = self.keys().cloned().collect::<Vec<_>>();
		for key in keys {
			if target.is_some_and(|target| target != &key) {
				continue;
			}
			let index = world
				.entity_index(&key)
				.ok_or("Character input owner missing")?;
			let settings = &world.data().entities[index]
				.character_controller
				.as_ref()
				.unwrap()
				.settings;
			self.set_input(
				&key,
				[movement[0], -movement[1]],
				actions.ids.contains(&settings.jump_action),
			)?;
		}
		Ok(())
	}
	pub fn prune(&mut self, world: &RuntimeWorld) {
		self.states
			.retain(|key, _| world.entity_index(key).is_some());
		self.inputs
			.retain(|key, _| world.entity_index(key).is_some());
		for state in self.states.values_mut() {
			if state.support.as_ref().is_some_and(|key| {
				world
					.entity_index(key)
					.is_none_or(|index| !world.data().entities[index].collision_enabled)
			}) {
				state.support = None;
				state.grounded = false;
			}
		}
	}
	pub fn step(
		&mut self,
		world: &mut RuntimeWorld,
		collisions: &mut CollisionWorld,
		tick: u64,
		dt: f32,
		budget: &mut usize,
		before: &mut HashMap<EntityKey, EntityData>,
	) -> Result<WorldChange, String> {
		let mut changes = WorldChange::default();
		for (key, state) in &mut self.states {
			let index = world.entity_index(key).ok_or("Character owner missing")?;
			let settings = world.data().entities[index]
				.character_controller
				.as_ref()
				.ok_or("Character component missing")?
				.settings
				.clone();
			let input = self.inputs.get_mut(key).expect("character input registry");
			let press = std::mem::take(&mut input.pressed);
			let (applied, next) = solve(
				world,
				collisions,
				key,
				state,
				input.movement,
				press,
				&settings,
				tick,
				dt,
				budget,
			)?;
			before
				.entry(key.clone())
				.or_insert_with(|| world.data().entities[index].clone());
			let mut transform = world.data().entities[index].transform.clone();
			transform.position = (Vec3::from_array(transform.position) + applied).to_array();
			let change = world.set_transform(key, transform)?;
			collisions.synchronize(world, &change)?;
			changes.merge(change);
			*state = next;
		}
		Ok(changes)
	}
}
