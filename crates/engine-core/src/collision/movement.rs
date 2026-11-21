use super::geometry::{Shape, bounds_overlap, contact_geometry, sweep_hit, translated};
use super::{CachedCollider, CollisionWorld, MAX_CANDIDATES, consume_candidates};
use crate::{Bounds3, EntityKey, SceneData};
use glam::Vec3;
use serde::{Deserialize, Serialize};

const EPSILON: f32 = 1.0e-6;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MovementResult {
	pub requested_delta: [f32; 3],
	pub applied_delta: [f32; 3],
	pub blocked_by: Option<EntityKey>,
	/// At most three swept contacts, in resolution order.
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub contacts: Vec<MovementContact>,
	pub changes: crate::world::WorldChange,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MovementContact {
	pub entity: EntityKey,
	/// Unit normal toward the obstacle. The positive normal component is removed.
	pub normal: [f32; 3],
	/// Time of impact in 0..=1 for this iteration's remaining motion, before skin.
	pub fraction: f64,
}

/// A speculative translation against the existing collision snapshot. No world
/// or collider records are mutated, and all casts share the caller's budget.
pub(crate) struct BodyCast {
	pub applied: Vec3,
	pub contact: Option<MovementContact>,
	pub flat_support: bool,
	pub obstacle_bounds: Option<Bounds3>,
}

impl CollisionWorld {
	pub(crate) fn body_bounds(&self, key: &EntityKey) -> Result<Bounds3, String> {
		self.indices
			.get(key)
			.map(|index| self.records[*index].bounds)
			.ok_or_else(|| "Character body has no enabled collider".into())
	}
	pub(crate) fn cast_body(
		&self,
		world: &crate::world::RuntimeWorld,
		key: &EntityKey,
		offset: Vec3,
		delta: Vec3,
		remaining: &mut usize,
	) -> Result<BodyCast, String> {
		if !offset.is_finite() || !delta.is_finite() {
			return Err("Character cast must be finite".into());
		}
		consume_candidates(remaining, 1)?;
		let index = *self
			.indices
			.get(key)
			.ok_or("Character collider is unavailable")?;
		let start = translated(&self.records[index].borrowed(), offset)?;
		let end = translated(&start, delta)?;
		let swept = start.bounds.union(end.bounds);
		let mut earliest: Option<(f64, Vec3, &CachedCollider)> = None;
		for entry in &self.ordered {
			let obstacle = &self.records[entry.index];
			if obstacle.bounds.min()[0] > swept.max()[0] {
				break;
			}
			consume_candidates(remaining, 1)?;
			if entry.index == index
				|| obstacle.interaction.sensor
				|| !start.interaction.detects(obstacle.interaction)
				|| !bounds_overlap(swept, obstacle.bounds)
			{
				continue;
			}
			if contact_geometry(&start, &obstacle.borrowed())
				.is_some_and(|(_, depth)| depth > 0.0001)
			{
				return Err("Character body starts penetrating a blocking collider".into());
			}
			if let Some((fraction, normal)) = sweep_hit(&start, &obstacle.borrowed(), delta)?
				&& earliest.as_ref().is_none_or(|(time, _, old)| {
					fraction < *time || fraction == *time && obstacle.key < old.key
				}) {
				earliest = Some((fraction, normal, obstacle));
			}
		}
		let Some((fraction, normal, obstacle)) = earliest else {
			return Ok(BodyCast {
				applied: delta,
				contact: None,
				flat_support: false,
				obstacle_bounds: None,
			});
		};
		let length = delta.as_dvec3().length();
		let travel = if length > 0.0 {
			(fraction - 0.0001 / length).max(0.0) as f32
		} else {
			0.0
		};
		let matrix = world.world_matrices()[obstacle.entity_index];
		let mut used = [false; 3];
		let aligned = [matrix.x_axis, matrix.y_axis, matrix.z_axis]
			.iter()
			.all(|column| {
				let Some(axis) = column.truncate().try_normalize() else {
					return false;
				};
				let values = axis.abs().to_array();
				let largest = (0..3)
					.max_by(|a, b| values[*a].total_cmp(&values[*b]))
					.unwrap();
				let valid = !used[largest]
					&& values[largest] >= 1.0 - 1e-5
					&& (0..3).all(|other| other == largest || values[other] <= 1e-5);
				used[largest] = true;
				valid
			});
		Ok(BodyCast {
			applied: delta * travel,
			contact: Some(MovementContact {
				entity: obstacle.key.clone(),
				normal: normal.to_array(),
				fraction,
			}),
			flat_support: matches!(obstacle.shape, Shape::Box) && aligned && normal.y < -0.999,
			obstacle_bounds: Some(obstacle.bounds),
		})
	}
	pub(crate) fn slide_body(
		&self,
		world: &crate::world::RuntimeWorld,
		key: &EntityKey,
		delta: Vec3,
		remaining: &mut usize,
	) -> Result<(Vec3, Vec<MovementContact>), String> {
		let mut applied = Vec3::ZERO;
		let mut rest = delta;
		let mut contacts = Vec::new();
		for _ in 0..3 {
			if rest.length_squared() <= EPSILON * EPSILON {
				break;
			}
			let cast = self.cast_body(world, key, applied, rest, remaining)?;
			applied += cast.applied;
			let Some(contact) = cast.contact else {
				break;
			};
			rest -= cast.applied;
			let normal = Vec3::from_array(contact.normal);
			let approach = rest.dot(normal);
			contacts.push(contact);
			if approach <= 0.0 {
				break;
			}
			rest -= normal * approach;
		}
		Ok((applied, contacts))
	}

	pub fn move_entity(
		&mut self,
		world: &mut crate::world::RuntimeWorld,
		key: &EntityKey,
		world_delta: [f32; 3],
	) -> Result<MovementResult, String> {
		let mut remaining = MAX_CANDIDATES;
		self.move_entity_with_budget(world, key, world_delta, &mut remaining)
	}
	/// Uses a caller-owned budget so several moves cannot each spend a fresh
	/// broad-phase allowance. Subtree lookups and skipped moving colliders also
	/// consume work: blocked movement must not bypass the aggregate limit.
	pub fn move_entity_with_budget(
		&mut self,
		world: &mut crate::world::RuntimeWorld,
		key: &EntityKey,
		world_delta: [f32; 3],
		remaining: &mut usize,
	) -> Result<MovementResult, String> {
		let mut result = MovementResult {
			requested_delta: world_delta,
			applied_delta: [0.0; 3],
			blocked_by: None,
			contacts: Vec::new(),
			changes: Default::default(),
		};
		let delta = Vec3::from_array(world_delta);
		if !delta.is_finite() {
			return Err("Movement must contain finite values".into());
		}
		let index = world
			.entity_index(key)
			.ok_or("Movement target does not exist")?;
		if delta == Vec3::ZERO {
			return Ok(result);
		}
		let parent = world.parent_matrix(index)?;
		if parent.determinant() == 0.0 {
			return Err("Cannot move beneath a singular parent transform".into());
		}
		let original = world.data().entities[index].transform.clone();
		let inverse = parent.inverse();
		if !(Vec3::from_array(original.position) + inverse.transform_vector3(delta)).is_finite() {
			return Err("Movement produces a nonfinite local position".into());
		}
		let (applied, contacts) = self.slide_delta(world, index, delta, remaining)?;
		result.blocked_by = contacts.first().map(|contact| contact.entity.clone());
		result.contacts = contacts;
		if applied == Vec3::ZERO {
			return Ok(result);
		}
		let mut transform = original.clone();
		transform.position =
			(Vec3::from_array(original.position) + inverse.transform_vector3(applied)).to_array();
		let before_world = world.world_matrices()[index].w_axis.truncate();
		result.changes = world.set_transform(key, transform)?;
		if let Err(error) = self.synchronize(world, &result.changes) {
			let _ = world.set_transform(key, original);
			return Err(error);
		}
		result.applied_delta =
			(world.world_matrices()[index].w_axis.truncate() - before_world).to_array();
		Ok(result)
	}

	fn slide_delta(
		&self,
		world: &crate::world::RuntimeWorld,
		root: usize,
		delta: Vec3,
		budget: &mut usize,
	) -> Result<(Vec3, Vec<MovementContact>), String> {
		if self.records.is_empty() {
			return Ok((delta, Vec::new()));
		}
		let indices = world.subtree_indices(root)?;
		consume_candidates(budget, indices.len())?;
		let moving = indices
			.iter()
			.copied()
			.collect::<std::collections::HashSet<_>>();
		let colliders = indices
			.iter()
			.filter_map(|&index| self.indices.get(&world.data().entities[index].key).copied())
			.collect::<Vec<_>>();
		let mut applied = Vec3::ZERO;
		let mut remainder = delta;
		let mut contacts = Vec::new();
		for _ in 0..3 {
			if remainder.length_squared() <= EPSILON * EPSILON {
				break;
			}
			let mut earliest: Option<(f64, Vec3, &EntityKey)> = None;
			for &index in &colliders {
				let initial = self.records[index].borrowed();
				let start = translated(&initial, applied)?;
				let end = translated(&start, remainder)?;
				let swept = start.bounds.union(end.bounds);
				for entry in &self.ordered {
					let obstacle = &self.records[entry.index];
					if obstacle.bounds.min()[0] > swept.max()[0] {
						break;
					}
					consume_candidates(budget, 1)?;
					if moving.contains(&obstacle.entity_index)
						|| !bounds_overlap(swept, obstacle.bounds)
					{
						continue;
					}
					if let Some((fraction, normal)) =
						sweep_hit(&start, &obstacle.borrowed(), remainder)?
						&& earliest.as_ref().is_none_or(|(time, _, key)| {
							fraction < *time
								|| fraction == *time && obstacle.key.as_str() < key.as_str()
						}) {
						earliest = Some((fraction, normal, &obstacle.key));
					}
				}
			}
			let Some((fraction, normal, entity)) = earliest else {
				applied += remainder;
				break;
			};
			contacts.push(MovementContact {
				entity: entity.clone(),
				normal: normal.to_array(),
				fraction,
			});
			let length = remainder.as_dvec3().length();
			let travel = (fraction - 0.0001 / length).max(0.0) as f32;
			applied += remainder * travel;
			remainder *= 1.0 - travel;
			let approach = remainder.dot(normal);
			if approach <= 0.0 {
				break;
			}
			remainder -= normal * approach;
		}
		Ok((applied, contacts))
	}
}

/// Translate an entity subtree in world space, preserving its local rotation
/// and scale. Motion advances to contact and slides along contact planes.
/// Invalid data or an exhausted work budget fails before any transform is modified.
pub fn move_entity(
	data: &mut SceneData,
	key: &EntityKey,
	world_delta: [f32; 3],
) -> Result<(), String> {
	let mut world = crate::world::RuntimeWorld::new(std::sync::Arc::new(data.clone()))?;
	let mut collisions = CollisionWorld::new(&world)?;
	let result = collisions.move_entity(&mut world, key, world_delta)?;
	if !result.changes.is_empty() {
		let index = world
			.entity_index(key)
			.ok_or("Movement target disappeared")?;
		data.entities[index].transform = world.data().entities[index].transform.clone();
	}
	Ok(())
}
