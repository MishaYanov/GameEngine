//! Bounded CPU collision queries for authoring and simple kinematic movement.
//! Rotated boxes use enclosing world AABBs. Swept movement is conservative: it
//! stops the requested translation when its swept bounds may hit an obstacle;
//! it is not a rigid-body solver or triangle-accurate collision system.

use std::collections::HashMap;

use glam::{Mat4, Vec3};
use serde::{Deserialize, Serialize};

use crate::{
	Bounds3, EntityKey,
	scene_data::{ColliderData, MAX_SCENE_ENTITIES, SceneData},
};

const MAX_CANDIDATES: usize = 1_000_000;
const MAX_CONTACTS: usize = 4096;
const EPSILON: f32 = 1.0e-6;

/// Persistent broad-phase ordering and cached world geometry. The stateless
/// helpers below remain available for authored-data queries.
pub struct CollisionWorld {
	records: Vec<CachedCollider>,
	indices: HashMap<EntityKey, usize>,
	ordered: std::collections::BTreeSet<SortEntry>,
	stats: CollisionStats,
}
struct CachedCollider {
	key: EntityKey,
	entity_index: usize,
	bounds: Bounds3,
	shape: Shape,
}
impl CachedCollider {
	fn borrowed(&self) -> WorldCollider<'_> {
		WorldCollider {
			entity: &self.key,
			entity_index: self.entity_index,
			bounds: self.bounds,
			shape: self.shape,
		}
	}
}
#[derive(Clone)]
struct SortEntry {
	x: f32,
	key: String,
	index: usize,
}
impl PartialEq for SortEntry {
	fn eq(&self, other: &Self) -> bool {
		self.cmp(other) == std::cmp::Ordering::Equal
	}
}
impl Eq for SortEntry {}
impl PartialOrd for SortEntry {
	fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
		Some(self.cmp(other))
	}
}
impl Ord for SortEntry {
	fn cmp(&self, other: &Self) -> std::cmp::Ordering {
		self.x
			.total_cmp(&other.x)
			.then_with(|| self.key.cmp(&other.key))
			.then_with(|| self.index.cmp(&other.index))
	}
}
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct CollisionStats {
	pub rebuilds: u64,
	pub updated_colliders: u64,
	pub queries: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MovementResult {
	pub requested_delta: [f32; 3],
	pub applied_delta: [f32; 3],
	pub blocked_by: Option<EntityKey>,
	pub changes: crate::world::WorldChange,
}
impl CollisionWorld {
	pub fn new(world: &crate::world::RuntimeWorld) -> Result<Self, String> {
		let records = world_colliders(world.data(), world.world_matrices())?
			.into_iter()
			.map(|c| CachedCollider {
				key: c.entity.clone(),
				entity_index: c.entity_index,
				bounds: c.bounds,
				shape: c.shape,
			})
			.collect::<Vec<_>>();
		let indices = records
			.iter()
			.enumerate()
			.map(|(i, c)| (c.key.clone(), i))
			.collect();
		let ordered = records
			.iter()
			.enumerate()
			.map(|(i, c)| SortEntry {
				x: c.bounds.min()[0],
				key: c.key.as_str().into(),
				index: i,
			})
			.collect();
		let updated_colliders = records.len() as u64;
		Ok(Self {
			records,
			indices,
			ordered,
			stats: CollisionStats {
				rebuilds: 1,
				updated_colliders,
				queries: 0,
			},
		})
	}
	pub fn stats(&self) -> CollisionStats {
		self.stats
	}
	pub fn collider_count(&self) -> usize {
		self.records.len()
	}
	pub fn synchronize(
		&mut self,
		world: &crate::world::RuntimeWorld,
		changes: &crate::world::WorldChange,
	) -> Result<(), String> {
		let membership_changed = changes
			.changed_entities
			.iter()
			.chain(changes.removed_entities.iter())
			.any(|key| {
				let present = world
					.entity_index(key)
					.is_some_and(|index| world.data().entities[index].collider.is_some());
				self.indices.contains_key(key) != present
			});
		if changes.topology_changed || changes.full_resync || membership_changed {
			let mut candidate = Self::new(world)?;
			candidate.stats.rebuilds = self.stats.rebuilds.saturating_add(1);
			candidate.stats.updated_colliders = self
				.stats
				.updated_colliders
				.saturating_add(candidate.records.len() as u64);
			candidate.stats.queries = self.stats.queries;
			*self = candidate;
			return Ok(());
		}
		let mut replacements = Vec::new();
		for key in &changes.changed_entities {
			let Some(&record) = self.indices.get(key) else {
				continue;
			};
			let index = world
				.entity_index(key)
				.ok_or("Cached collider target is missing")?;
			let entity = &world.data().entities[index];
			let Some(collider) = entity.collider.as_ref() else {
				return Err("Collider topology changed without a topology notification".into());
			};
			let matrix = world.world_matrices()[index];
			let (bounds, shape) = collider_geometry(collider, matrix)?;
			replacements.push((
				record,
				CachedCollider {
					key: key.clone(),
					entity_index: index,
					bounds,
					shape,
				},
			));
		}
		for (index, replacement) in replacements {
			let old = &self.records[index];
			self.ordered.remove(&SortEntry {
				x: old.bounds.min()[0],
				key: old.key.as_str().into(),
				index,
			});
			self.ordered.insert(SortEntry {
				x: replacement.bounds.min()[0],
				key: replacement.key.as_str().into(),
				index,
			});
			self.records[index] = replacement;
			self.stats.updated_colliders = self.stats.updated_colliders.saturating_add(1);
		}
		Ok(())
	}
	pub fn query_overlaps(&mut self) -> Result<Vec<Contact>, String> {
		self.stats.queries = self.stats.queries.saturating_add(1);
		let mut contacts = Vec::new();
		let mut candidates = 0;
		use std::ops::Bound::{Excluded, Unbounded};
		for entry in &self.ordered {
			let a = &self.records[entry.index];
			for next in self.ordered.range((Excluded(entry), Unbounded)) {
				let b = &self.records[next.index];
				if b.bounds.min()[0] > a.bounds.max()[0] {
					break;
				}
				charge_candidate(&mut candidates)?;
				if let Some((mut normal, penetration)) =
					contact_geometry(&a.borrowed(), &b.borrowed())
				{
					if !normal.is_finite() || !penetration.is_finite() {
						return Err("Collision contact exceeds finite limits".into());
					}
					if contacts.len() >= MAX_CONTACTS {
						return Err("Collision contact budget exhausted".into());
					}
					let (first, second) = if a.key.as_str() <= b.key.as_str() {
						(&a.key, &b.key)
					} else {
						normal = -normal;
						(&b.key, &a.key)
					};
					contacts.push(Contact {
						a: first.clone(),
						b: second.clone(),
						normal: normal.to_array(),
						penetration,
					});
				}
			}
		}
		contacts.sort_by(|a, b| {
			a.a.as_str()
				.cmp(b.a.as_str())
				.then_with(|| a.b.as_str().cmp(b.b.as_str()))
		});
		Ok(contacts)
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
		let mut transform = world.data().entities[index].transform.clone();
		let position =
			Vec3::from_array(transform.position) + parent.inverse().transform_vector3(delta);
		if !position.is_finite() {
			return Err("Movement produces a nonfinite local position".into());
		}
		if !self.records.is_empty() {
			let moving_indices = world.subtree_indices(index)?;
			consume_candidates(remaining, moving_indices.len())?;
			let moving = moving_indices
				.iter()
				.copied()
				.collect::<std::collections::HashSet<_>>();
			// Look up only this subtree's colliders, rather than scanning the
			// entire world's collider cache for every movement action.
			let mut moving_colliders = moving_indices
				.iter()
				.filter_map(|&index| self.indices.get(&world.data().entities[index].key).copied())
				.collect::<Vec<_>>();
			moving_colliders.sort_unstable();
			for collider_index in moving_colliders {
				let collider = &self.records[collider_index];
				let borrowed = collider.borrowed();
				let end = translated(&borrowed, delta)?;
				let swept = collider.bounds.union(end.bounds);
				for entry in &self.ordered {
					let obstacle = &self.records[entry.index];
					if obstacle.bounds.min()[0] > swept.max()[0] {
						break;
					}
					consume_candidates(remaining, 1)?;
					if moving.contains(&obstacle.entity_index) {
						continue;
					}
					if bounds_overlap(swept, obstacle.bounds)
						&& blocks_motion(&borrowed, &end, &obstacle.borrowed(), delta)?
					{
						result.blocked_by = Some(obstacle.key.clone());
						return Ok(result);
					}
				}
			}
		}
		let before = transform.clone();
		let before_world = world.world_matrices()[index].w_axis.truncate();
		transform.position = position.to_array();
		result.changes = world.set_transform(key, transform)?;
		if let Err(error) = self.synchronize(world, &result.changes) {
			let _ = world.set_transform(key, before);
			return Err(error);
		}
		result.applied_delta =
			(world.world_matrices()[index].w_axis.truncate() - before_world).to_array();
		Ok(result)
	}
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Contact {
	pub a: EntityKey,
	pub b: EntityKey,
	/// Unit normal from a toward b. Move a along the negative normal to separate.
	pub normal: [f32; 3],
	pub penetration: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DebugShape {
	Box {
		entity: EntityKey,
		min: [f32; 3],
		max: [f32; 3],
	},
	Sphere {
		entity: EntityKey,
		center: [f32; 3],
		radius: f32,
		min: [f32; 3],
		max: [f32; 3],
	},
}

#[derive(Clone, Copy)]
enum Shape {
	Box,
	Sphere { center: Vec3, radius: f32 },
}

struct WorldCollider<'a> {
	entity: &'a EntityKey,
	entity_index: usize,
	bounds: Bounds3,
	shape: Shape,
}

/// Deterministic sweep-and-prune on minimum X, followed by box/sphere tests.
/// Dense scenes return an explicit budget error instead of unbounded pair work.
pub fn query_overlaps(data: &SceneData) -> Result<Vec<Contact>, String> {
	let matrices = checked_matrices(data)?;
	let mut colliders = world_colliders(data, &matrices)?;
	colliders.sort_by(|a, b| {
		a.bounds.min()[0]
			.total_cmp(&b.bounds.min()[0])
			.then_with(|| a.entity.as_str().cmp(b.entity.as_str()))
	});
	let mut contacts = Vec::new();
	let mut candidates = 0;
	for (index, a) in colliders.iter().enumerate() {
		for b in &colliders[index + 1..] {
			if b.bounds.min()[0] > a.bounds.max()[0] {
				break;
			}
			charge_candidate(&mut candidates)?;
			if !bounds_overlap(a.bounds, b.bounds) {
				continue;
			}
			if let Some((mut normal, penetration)) = contact_geometry(a, b) {
				if !normal.is_finite() || !penetration.is_finite() {
					return Err("Collision contact exceeds finite numeric limits".into());
				}
				if contacts.len() >= MAX_CONTACTS {
					return Err(format!("Collision query exceeds {MAX_CONTACTS} contacts"));
				}
				let (first, second) = if a.entity.as_str() <= b.entity.as_str() {
					(a.entity, b.entity)
				} else {
					normal = -normal;
					(b.entity, a.entity)
				};
				contacts.push(Contact {
					a: first.clone(),
					b: second.clone(),
					normal: normal.to_array(),
					penetration,
				});
			}
		}
	}
	contacts.sort_by(|a, b| {
		a.a.as_str()
			.cmp(b.a.as_str())
			.then_with(|| a.b.as_str().cmp(b.b.as_str()))
	});
	Ok(contacts)
}

/// World-space debug geometry independent of renderer or frontend controls.
pub fn debug_shapes(data: &SceneData) -> Result<Vec<DebugShape>, String> {
	let matrices = checked_matrices(data)?;
	Ok(world_colliders(data, &matrices)?
		.into_iter()
		.map(|collider| {
			let min = collider.bounds.min();
			let max = collider.bounds.max();
			match collider.shape {
				Shape::Box => DebugShape::Box {
					entity: collider.entity.clone(),
					min,
					max,
				},
				Shape::Sphere { center, radius } => DebugShape::Sphere {
					entity: collider.entity.clone(),
					center: center.to_array(),
					radius,
					min,
					max,
				},
			}
		})
		.collect())
}

/// Translate an entity subtree in world space, preserving its local rotation
/// and scale. Blocked motion succeeds without changing the scene. Invalid data
/// or an exhausted work budget fails before any transform is modified.
pub fn move_entity(
	data: &mut SceneData,
	key: &EntityKey,
	world_delta: [f32; 3],
) -> Result<(), String> {
	let delta = Vec3::from_array(world_delta);
	if !delta.is_finite() {
		return Err("Collision movement must contain finite values".into());
	}
	let index = data
		.entities
		.iter()
		.position(|entity| &entity.key == key)
		.ok_or_else(|| format!("Entity {:?} does not exist", key.as_str()))?;
	if delta == Vec3::ZERO {
		return Ok(());
	}
	let matrices = checked_matrices(data)?;
	let indices: HashMap<_, _> = data
		.entities
		.iter()
		.enumerate()
		.map(|(index, entity)| (entity.key.as_str(), index))
		.collect();
	let parent_matrix = match &data.entities[index].parent {
		Some(parent) => {
			matrices[*indices
				.get(parent.as_str())
				.ok_or("Missing movement parent")?]
		}
		None => Mat4::IDENTITY,
	};
	if parent_matrix.determinant() == 0.0 {
		return Err("Cannot move in world space beneath a singular parent transform".into());
	}
	let local_delta = parent_matrix.inverse().transform_vector3(delta);
	let position = Vec3::from_array(data.entities[index].transform.position) + local_delta;
	if !position.is_finite() {
		return Err("Movement produces a nonfinite local position".into());
	}
	let moving = subtree_members(data, &indices, index)?;
	let colliders = world_colliders(data, &matrices)?;
	let mut stationary: Vec<_> = colliders
		.iter()
		.filter(|collider| !moving[collider.entity_index])
		.collect();
	stationary.sort_by(|a, b| {
		a.bounds.min()[0]
			.total_cmp(&b.bounds.min()[0])
			.then_with(|| a.entity.as_str().cmp(b.entity.as_str()))
	});
	let mut candidates = 0;
	for collider in colliders
		.iter()
		.filter(|collider| moving[collider.entity_index])
	{
		let end = translated(collider, delta)?;
		let swept = collider.bounds.union(end.bounds);
		for obstacle in &stationary {
			if obstacle.bounds.min()[0] > swept.max()[0] {
				break;
			}
			charge_candidate(&mut candidates)?;
			if !bounds_overlap(swept, obstacle.bounds) {
				continue;
			}
			if blocks_motion(collider, &end, obstacle, delta)? {
				return Ok(());
			}
		}
	}
	data.entities[index].transform.position = position.to_array();
	Ok(())
}

fn checked_matrices(data: &SceneData) -> Result<Vec<Mat4>, String> {
	if data.entities.len() > MAX_SCENE_ENTITIES {
		return Err(format!(
			"Collision scene exceeds {MAX_SCENE_ENTITIES} entities"
		));
	}
	data.world_matrices()
}

fn world_colliders<'a>(
	data: &'a SceneData,
	matrices: &[Mat4],
) -> Result<Vec<WorldCollider<'a>>, String> {
	let mut colliders = Vec::new();
	for (index, (entity, matrix)) in data.entities.iter().zip(matrices).enumerate() {
		let Some(collider) = &entity.collider else {
			continue;
		};
		let (bounds, shape) = collider_geometry(collider, *matrix)?;
		colliders.push(WorldCollider {
			entity: &entity.key,
			entity_index: index,
			bounds,
			shape,
		});
	}
	Ok(colliders)
}

fn collider_geometry(collider: &ColliderData, matrix: Mat4) -> Result<(Bounds3, Shape), String> {
	collider.validate()?;
	let geometry = match collider {
		ColliderData::Box {
			half_extents,
			offset,
		} => {
			let offset = Vec3::from_array(*offset);
			let half = Vec3::from_array(*half_extents);
			let bounds = Bounds3::new((offset - half).to_array(), (offset + half).to_array())?
				.transformed(matrix)?;
			(bounds, Shape::Box)
		}
		ColliderData::Sphere { radius, offset } => {
			let center = matrix.transform_point3(Vec3::from_array(*offset));
			let radius = (*radius as f64 * conservative_scale(matrix)) as f32;
			if !radius.is_finite() || !center.is_finite() {
				return Err("Sphere collider produces nonfinite world geometry".into());
			}
			let half = Vec3::splat(radius);
			(
				Bounds3::new((center - half).to_array(), (center + half).to_array())?,
				Shape::Sphere { center, radius },
			)
		}
	};
	Ok(geometry)
}

fn conservative_scale(matrix: Mat4) -> f64 {
	let axes = [
		matrix.x_axis.truncate(),
		matrix.y_axis.truncate(),
		matrix.z_axis.truncate(),
	];
	// The largest absolute row sum of A^T A bounds its largest eigenvalue.
	// Orthogonal TRS columns reduce to the exact maximum axis scale; hierarchy
	// shear remains safely enclosed instead of underestimating the sphere.
	(0..3)
		.map(|row| {
			(0..3)
				.map(|column| {
					(0..3)
						.map(|axis| axes[row][axis] as f64 * axes[column][axis] as f64)
						.sum::<f64>()
						.abs()
				})
				.sum::<f64>()
		})
		.fold(0.0, f64::max)
		.sqrt()
}

fn bounds_overlap(a: Bounds3, b: Bounds3) -> bool {
	(0..3).all(|axis| a.min()[axis] <= b.max()[axis] && a.max()[axis] >= b.min()[axis])
}

fn contact_geometry(a: &WorldCollider<'_>, b: &WorldCollider<'_>) -> Option<(Vec3, f32)> {
	match (a.shape, b.shape) {
		(Shape::Box, Shape::Box) => box_box(a.bounds, b.bounds),
		(
			Shape::Sphere {
				center: a,
				radius: ar,
			},
			Shape::Sphere {
				center: b,
				radius: br,
			},
		) => {
			let (normal, distance) = direction_and_distance(a, b);
			let penetration = ar as f64 + br as f64 - distance;
			(penetration >= 0.0).then_some((normal, penetration as f32))
		}
		(Shape::Sphere { center, radius }, Shape::Box) => sphere_box(center, radius, b.bounds),
		(Shape::Box, Shape::Sphere { center, radius }) => {
			sphere_box(center, radius, a.bounds).map(|(normal, penetration)| (-normal, penetration))
		}
	}
}

fn box_box(a: Bounds3, b: Bounds3) -> Option<(Vec3, f32)> {
	if !bounds_overlap(a, b) {
		return None;
	}
	let mut penetration = f32::INFINITY;
	let mut normal = Vec3::X;
	for axis in 0..3 {
		let negative = a.max()[axis] - b.min()[axis];
		let positive = b.max()[axis] - a.min()[axis];
		let depth = negative.min(positive);
		if depth < penetration {
			penetration = depth;
			normal = Vec3::ZERO;
			normal[axis] = if negative <= positive { 1.0 } else { -1.0 };
		}
	}
	Some((normal, penetration))
}

fn sphere_box(center: Vec3, radius: f32, bounds: Bounds3) -> Option<(Vec3, f32)> {
	let closest = center.clamp(
		Vec3::from_array(bounds.min()),
		Vec3::from_array(bounds.max()),
	);
	let (normal, distance) = direction_and_distance(center, closest);
	if distance > 0.0 {
		let penetration = radius as f64 - distance;
		return (penetration >= 0.0).then_some((normal, penetration as f32));
	}
	// Containment needs distance to an escape face, not zero penetration.
	let mut depth = f32::INFINITY;
	let mut normal = Vec3::X;
	for axis in 0..3 {
		let negative = center[axis] - bounds.min()[axis];
		let positive = bounds.max()[axis] - center[axis];
		if negative.min(positive) < depth {
			depth = negative.min(positive);
			normal = Vec3::ZERO;
			normal[axis] = if negative <= positive { 1.0 } else { -1.0 };
		}
	}
	Some((normal, radius + depth))
}

fn direction_and_distance(from: Vec3, to: Vec3) -> (Vec3, f64) {
	let delta = [
		to.x as f64 - from.x as f64,
		to.y as f64 - from.y as f64,
		to.z as f64 - from.z as f64,
	];
	let distance = delta
		.into_iter()
		.map(|value| value * value)
		.sum::<f64>()
		.sqrt();
	let normal = if distance > 0.0 {
		Vec3::from_array(delta.map(|value| (value / distance) as f32))
	} else {
		Vec3::X
	};
	(normal, distance)
}

fn translated<'a>(collider: &WorldCollider<'a>, delta: Vec3) -> Result<WorldCollider<'a>, String> {
	let bounds = Bounds3::new(
		(Vec3::from_array(collider.bounds.min()) + delta).to_array(),
		(Vec3::from_array(collider.bounds.max()) + delta).to_array(),
	)?;
	let shape = match collider.shape {
		Shape::Box => Shape::Box,
		Shape::Sphere { center, radius } => Shape::Sphere {
			center: center + delta,
			radius,
		},
	};
	Ok(WorldCollider {
		entity: collider.entity,
		entity_index: collider.entity_index,
		bounds,
		shape,
	})
}

fn blocks_motion(
	start: &WorldCollider<'_>,
	end: &WorldCollider<'_>,
	obstacle: &WorldCollider<'_>,
	delta: Vec3,
) -> Result<bool, String> {
	if let Some((normal, penetration)) = contact_geometry(start, obstacle) {
		if !normal.is_finite() || !penetration.is_finite() {
			return Err("Collision contact exceeds finite numeric limits".into());
		}
		let after = contact_geometry(end, obstacle).map_or(0.0, |(_, depth)| depth);
		if !after.is_finite() {
			return Err("Collision contact exceeds finite numeric limits".into());
		}
		let approach = motion_dot(delta, normal);
		if approach < 0.0 && after <= penetration || penetration <= EPSILON && approach <= 0.0 {
			return Ok(false);
		}
		return Ok(true);
	}
	if bounds_overlap(start.bounds, obstacle.bounds) {
		// If their bounding boxes overlap but the curved surfaces do not,
		// permit travel away from a separating plane; otherwise block the
		// uncertain sweep rather than stepping through a potential contact.
		let separating = match (start.shape, obstacle.shape) {
			(Shape::Sphere { center: a, .. }, Shape::Sphere { center: b, .. }) => {
				direction_and_distance(a, b).0
			}
			(Shape::Sphere { center, .. }, Shape::Box) => {
				direction_and_distance(
					center,
					center.clamp(
						Vec3::from_array(obstacle.bounds.min()),
						Vec3::from_array(obstacle.bounds.max()),
					),
				)
				.0
			}
			(Shape::Box, Shape::Sphere { center, .. }) => {
				direction_and_distance(
					center.clamp(
						Vec3::from_array(start.bounds.min()),
						Vec3::from_array(start.bounds.max()),
					),
					center,
				)
				.0
			}
			_ => Vec3::ZERO,
		};
		return Ok(motion_dot(delta, separating) > 0.0);
	}
	let min = Vec3::from_array(start.bounds.min());
	let max = Vec3::from_array(start.bounds.max());
	let center = min * 0.5 + max * 0.5;
	let half = max * 0.5 - min * 0.5;
	let expanded = Bounds3::new(
		(Vec3::from_array(obstacle.bounds.min()) - half).to_array(),
		(Vec3::from_array(obstacle.bounds.max()) + half).to_array(),
	)?;
	Ok(expanded.ray_distance(center, delta, 1.0).is_some())
}

fn motion_dot(delta: Vec3, direction: Vec3) -> f64 {
	(0..3)
		.map(|axis| delta[axis] as f64 * direction[axis] as f64)
		.sum()
}

fn subtree_members(
	data: &SceneData,
	indices: &HashMap<&str, usize>,
	root: usize,
) -> Result<Vec<bool>, String> {
	let mut children = vec![Vec::new(); data.entities.len()];
	for (index, entity) in data.entities.iter().enumerate() {
		if let Some(parent) = &entity.parent {
			let parent = *indices
				.get(parent.as_str())
				.ok_or("Missing collider parent")?;
			children[parent].push(index);
		}
	}
	let mut moving = vec![false; data.entities.len()];
	let mut pending = vec![root];
	while let Some(index) = pending.pop() {
		moving[index] = true;
		pending.extend_from_slice(&children[index]);
	}
	Ok(moving)
}

fn charge_candidate(candidates: &mut usize) -> Result<(), String> {
	*candidates += 1;
	if *candidates > MAX_CANDIDATES {
		return Err(format!(
			"Collision query exceeds {MAX_CANDIDATES} broadphase candidates"
		));
	}
	Ok(())
}

fn consume_candidates(remaining: &mut usize, count: usize) -> Result<(), String> {
	if count > *remaining {
		*remaining = 0;
		return Err("Collision movement work budget exhausted".into());
	}
	*remaining -= count;
	Ok(())
}
