//! Fixed-step motion, authored launch input and contact for spawned projectile Props.

use crate::{
	EntityKey,
	collision::CollisionWorld,
	components::{NativeBehaviorSpec, ParticleBurstSettings, ProjectileSettings},
	editor::PrefabData,
	runtime_lifecycle::{PreparedRuntimeSpawn, RuntimeEntityHandle, RuntimeLifecycle},
	scene_data::EntityData,
	simulation_session::{ActionWriter, InputFrame, SimulationAction},
	world::RuntimeWorld,
};
use glam::{EulerRot, Quat, Vec3};
use std::collections::BTreeMap;

#[derive(Clone, Copy)]
pub struct ProjectileHookStep {
	pub tick: u64,
	pub step_seconds: f32,
}

#[derive(Debug, Clone)]
pub struct ProjectileHit {
	pub projectile: RuntimeEntityHandle,
	pub target: RuntimeEntityHandle,
	pub position: [f32; 3],
	pub normal: [f32; 3],
}

pub(crate) struct PlannedProjectileHit<'a> {
	pub event: ProjectileHit,
	pub impact: Option<&'a ParticleBurstSettings>,
}

#[derive(Clone)]
pub(crate) struct ProjectileInstance {
	pub entity: EntityKey,
	pub launcher: Option<RuntimeEntityHandle>,
	pub settings: ProjectileSettings,
	direction: Vec3,
	spawned_tick: u64,
}

impl ProjectileInstance {
	pub(crate) fn new(prepared: &PreparedRuntimeSpawn, tick: u64) -> Result<Option<Self>, String> {
		if !prepared
			.entities
			.iter()
			.any(|entity| projectile_settings(entity).is_some())
		{
			return Ok(None);
		}
		let root = &prepared.entities[launch_root(&prepared.entities)?];
		let settings = projectile_settings(root)
			.expect("checked enabled Projectile")
			.clone();
		let direction = prepared
			.request
			.as_ref()
			.and_then(|request| request.projectile_direction)
			.map(Vec3::from_array)
			.unwrap_or_else(|| authored_rotation(root) * Vec3::NEG_Z);
		let direction = direction
			.try_normalize()
			.ok_or("Projectile requires a finite launch direction")?;
		Ok(Some(Self {
			entity: root.key.clone(),
			launcher: prepared
				.request
				.as_ref()
				.map(|request| request.owner.clone()),
			settings,
			direction,
			spawned_tick: tick,
		}))
	}
}

impl PrefabData {
	/// Check the assembled Prop selected in a Projectile Launcher.
	pub fn validate_projectile_launch(&self) -> Result<(), String> {
		if self.gameplay.is_some() || !self.reference_slots.is_empty() {
			return Err("Projectile Launcher requires a Prop with no external reference slots or scene-role gameplay".into());
		}
		launch_root(&self.expanded_entities()?).map(|_| ())
	}
}

pub(crate) fn orient_launch(
	entities: &mut [EntityData],
	direction: [f32; 3],
) -> Result<(), String> {
	let root_index = launch_root(entities)?;
	let root = &mut entities[root_index];
	let direction = Vec3::from_array(direction)
		.try_normalize()
		.ok_or("Projectile launch direction is invalid")?;
	let rotation = Quat::from_rotation_arc(Vec3::NEG_Z, direction);
	let (x, y, z) = (rotation * authored_rotation(root)).to_euler(EulerRot::XYZ);
	root.transform.rotation = [x, y, z];
	root.transform.position = (rotation * Vec3::from_array(root.transform.position)).to_array();
	Ok(())
}

fn launch_root(entities: &[EntityData]) -> Result<usize, String> {
	let mut roots = entities
		.iter()
		.enumerate()
		.filter(|(_, entity)| entity.parent.is_none());
	let (index, root) = roots
		.next()
		.ok_or("Projectile Prop requires a root object")?;
	if roots.next().is_some() || projectile_settings(root).is_none() {
		return Err("Projectile Launcher requires a Prop with one enabled Projectile root".into());
	}
	Ok(index)
}

pub(crate) fn plan_launches(
	world: &RuntimeWorld,
	lifecycle: &mut RuntimeLifecycle,
	input: &InputFrame,
) -> Result<(), String> {
	let Some(local) = input.local_player() else {
		return Ok(());
	};
	let Some(index) = world.entity_index(&local.key) else {
		return Ok(());
	};
	let entity = &world.data().entities[index];
	for component in entity
		.native_behaviors
		.iter()
		.filter(|component| component.enabled)
	{
		let NativeBehaviorSpec::ProjectileLauncher { settings } = &component.behavior else {
			continue;
		};
		if !input.action_pressed(&settings.action)
			|| lifecycle.alive_for(&entity.key, &component.id) >= settings.max_alive as usize
			|| lifecycle.has_request(&entity.key, &component.id)
		{
			continue;
		}
		let matrix = world.world_matrices()[index];
		let heading = (-matrix.z_axis.truncate())
			.try_normalize()
			.ok_or("Projectile Launcher needs a valid forward axis")?;
		let rotation = Quat::from_rotation_arc(Vec3::NEG_Z, heading);
		let position =
			matrix.w_axis.truncate() + rotation * Vec3::from_array(settings.spawn_offset);
		lifecycle.enqueue(
			&entity.key,
			&component.id,
			&settings.prefab_path,
			&BTreeMap::new(),
			position.to_array(),
			0,
			Some(heading.to_array()),
		)?;
	}
	Ok(())
}

pub(crate) fn plan_motion<'a>(
	world: &RuntimeWorld,
	collisions: &CollisionWorld,
	lifecycle: &'a RuntimeLifecycle,
	step: ProjectileHookStep,
	remaining: &mut usize,
	actions: &mut ActionWriter<'_>,
) -> Result<Vec<PlannedProjectileHit<'a>>, String> {
	let mut hits = Vec::new();
	for projectile in lifecycle.projectiles() {
		let handle = lifecycle
			.handle(&projectile.entity)
			.ok_or("Projectile is no longer active")?;
		if step.tick.saturating_sub(projectile.spawned_tick)
			> u64::from(projectile.settings.lifetime_ticks)
		{
			actions.push(SimulationAction::Despawn { handle })?;
			continue;
		}
		let index = world
			.entity_index(&projectile.entity)
			.ok_or("Projectile world object is missing")?;
		let mut transform = world.data().entities[index].transform.clone();
		let origin = Vec3::from_array(transform.position);
		let delta = projectile.direction * (projectile.settings.speed * step.step_seconds);
		if let Some(contact) =
			collisions.projectile_contact(world, lifecycle, projectile, origin, delta, remaining)?
		{
			transform.position = contact.position;
			actions.push(SimulationAction::SetTransform {
				entity: projectile.entity.clone(),
				transform,
			})?;
			hits.push(PlannedProjectileHit {
				event: ProjectileHit {
					projectile: handle.clone(),
					target: lifecycle
						.handle(&contact.target)
						.ok_or("Projectile hit target is unavailable")?,
					position: contact.position,
					normal: contact.normal,
				},
				impact: projectile.settings.impact.as_ref(),
			});
			actions.push(SimulationAction::Despawn { handle })?;
		} else {
			transform.position = (origin + delta).to_array();
			actions.push(SimulationAction::SetTransform {
				entity: projectile.entity.clone(),
				transform,
			})?;
		}
	}
	Ok(hits)
}

fn authored_rotation(entity: &EntityData) -> Quat {
	let [x, y, z] = entity.transform.rotation;
	Quat::from_euler(EulerRot::XYZ, x, y, z)
}

fn projectile_settings(entity: &EntityData) -> Option<&ProjectileSettings> {
	entity
		.native_behaviors
		.iter()
		.find_map(|component| match &component.behavior {
			NativeBehaviorSpec::Projectile { settings } if component.enabled => Some(settings),
			_ => None,
		})
}
