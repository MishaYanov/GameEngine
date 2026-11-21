//! Bounded CPU collision queries for authoring and simple kinematic movement.
//! Rotated boxes use enclosing world AABBs. Swept movement is conservative: it
//! advances to contact and projects remaining motion along at most three contact planes;
//! it is not a rigid-body solver or triangle-accurate collision system.

mod geometry;
mod movement;

use geometry::{
	Shape, WorldCollider, bounds_overlap, collider_geometry, contact_geometry, world_colliders,
};
pub(crate) use movement::BodyCast;
pub use movement::{MovementContact, MovementResult, move_entity};

use glam::{DVec3, Mat4, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::runtime_lifecycle::{RuntimeEntityHandle, RuntimeLifecycle};
use crate::world::RuntimeWorld;
use crate::{
	Bounds3, EntityKey,
	collider::ColliderInteraction,
	scene_data::{ColliderData, MAX_SCENE_ENTITIES, SceneData},
};

const MAX_CANDIDATES: usize = 1_000_000;
const MAX_CONTACTS: usize = 4096;
const CAMERA_MAX_VISITS: usize = 8192;
const INTERACTION_MAX_VISITS: usize = 512;
const CAMERA_CLEARANCE: f64 = 0.001;

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
	interaction: ColliderInteraction,
}

#[derive(Clone)]
pub(crate) struct CameraExclusions {
	target: RuntimeEntityHandle,
	topology_revision: u64,
	by_entity_index: Arc<[bool]>,
}

impl CameraExclusions {
	pub(crate) fn build(
		world: &RuntimeWorld,
		lifecycle: &RuntimeLifecycle,
		target: &RuntimeEntityHandle,
		remaining: &mut usize,
	) -> Result<Self, String> {
		lifecycle.validate_handle(target)?;
		let count = world.data().entities.len();
		if count > crate::scene_data::MAX_SCENE_ENTITIES {
			return Err("Camera exclusion world exceeds the scene entity limit".into());
		}

		consume_camera_budget(remaining, count)?;
		let root = world
			.entity_index(&target.key)
			.ok_or_else(|| "Camera target is missing from the candidate world".to_owned())?;
		let mut excluded = vec![false; count];
		for index in world.subtree_indices(root)? {
			let slot = excluded
				.get_mut(index)
				.ok_or_else(|| "Camera exclusion index is invalid".to_owned())?;
			*slot = true;
		}
		Ok(Self {
			target: target.clone(),
			topology_revision: lifecycle.topology_revision,
			by_entity_index: excluded.into(),
		})
	}

	pub(crate) fn matches(
		&self,
		world: &RuntimeWorld,
		target: &RuntimeEntityHandle,
		topology_revision: u64,
	) -> bool {
		self.target == *target
			&& self.topology_revision == topology_revision
			&& self.by_entity_index.len() == world.data().entities.len()
	}
}

#[derive(Clone, Debug, PartialEq)]
pub struct CameraProbeHit {
	pub blocker: crate::EntityKey,
	pub fraction: f64,
}

pub(crate) struct CameraProbeLimit {
	pub distance: f32,
	pub hit: Option<CameraProbeHit>,
}

pub(crate) struct CameraProbe<'a> {
	pub pivot: [f32; 3],
	pub forward: [f32; 3],
	pub distance: f32,
	pub radius: f32,
	pub obstruction_mask: u32,
	pub target: &'a RuntimeEntityHandle,
	pub topology_revision: u64,
}

impl CachedCollider {
	fn borrowed(&self) -> WorldCollider<'_> {
		WorldCollider {
			entity: &self.key,
			entity_index: self.entity_index,
			bounds: self.bounds,
			shape: self.shape,
			interaction: self.interaction,
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
impl CollisionWorld {
	pub fn new(world: &crate::world::RuntimeWorld) -> Result<Self, String> {
		let records = world_colliders(world.data(), world.world_matrices())?
			.into_iter()
			.map(|c| CachedCollider {
				key: c.entity.clone(),
				entity_index: c.entity_index,
				bounds: c.bounds,
				shape: c.shape,
				interaction: c.interaction,
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

	/// Conservatively tests a short interaction segment against synchronized
	/// solid collider bounds. An exhausted query grants no interaction.
	pub fn interaction_segment_clear(
		&self,
		world: &RuntimeWorld,
		actor: &EntityKey,
		target: &EntityKey,
	) -> bool {
		let Some(actor_index) = self.indices.get(actor).copied() else {
			return false;
		};
		let Some(actor_entity) = world.entity_index(actor) else {
			return false;
		};
		let Some(target_index) = world.entity_index(target) else {
			return false;
		};
		let actor_collider = &self.records[actor_index];
		let from = world.world_matrices()[actor_entity].w_axis.truncate();
		let to = world.world_matrices()[target_index].w_axis.truncate();
		let delta = to - from;
		if !from.is_finite() || !to.is_finite() || !delta.is_finite() {
			return false;
		}
		if delta == Vec3::ZERO {
			return true;
		}
		for (visit, entry) in self.ordered.iter().enumerate() {
			let obstacle = &self.records[entry.index];
			if obstacle.bounds.min()[0] > from.x.max(to.x) {
				break;
			}
			if visit >= INTERACTION_MAX_VISITS {
				return false;
			}
			let interaction = obstacle.interaction;
			if &obstacle.key != actor
				&& &obstacle.key != target
				&& !interaction.sensor
				&& interaction.detects(actor_collider.interaction)
				&& obstacle.bounds.ray_distance(from, delta, 1.0).is_some()
			{
				return false;
			}
		}
		true
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
				let present = world.entity_index(key).is_some_and(|index| {
					world.data().entities[index].collision_enabled
						&& world.data().entities[index].collider.is_some()
				});
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
					interaction: collider.interaction(),
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
		let mut remaining = MAX_CANDIDATES;
		self.query_with_budget(&mut remaining, false)
	}

	/// Sensor contacts share the caller's work allowance with movement and other steps.
	pub fn query_sensor_overlaps_with_budget(
		&mut self,
		remaining: &mut usize,
	) -> Result<Vec<Contact>, String> {
		self.query_with_budget(remaining, true)
	}

	fn query_with_budget(
		&mut self,
		remaining: &mut usize,
		sensors_only: bool,
	) -> Result<Vec<Contact>, String> {
		self.stats.queries = self.stats.queries.saturating_add(1);
		let mut contacts = Vec::new();
		use std::ops::Bound::{Excluded, Unbounded};
		for entry in &self.ordered {
			let a = &self.records[entry.index];
			for next in self.ordered.range((Excluded(entry), Unbounded)) {
				let b = &self.records[next.index];
				if b.bounds.min()[0] > a.bounds.max()[0] {
					break;
				}
				consume_candidates(remaining, 1).map_err(|_| {
					format!("Collision query exceeds {MAX_CANDIDATES} broadphase candidates")
				})?;
				if sensors_only && !a.interaction.sensor && !b.interaction.sensor {
					continue;
				}
				if let Some((normal, penetration)) = contact_geometry(&a.borrowed(), &b.borrowed())
				{
					if !normal.is_finite() || !penetration.is_finite() {
						return Err("Collision contact exceeds finite limits".into());
					}
					if contacts.len() >= MAX_CONTACTS {
						return Err("Collision contact budget exhausted".into());
					}
					contacts.push(Contact::ordered(
						&a.key,
						&b.key,
						normal,
						penetration,
						a.interaction.sensor || b.interaction.sensor,
					));
				}
			}
		}
		sort_contacts(&mut contacts);
		Ok(contacts)
	}
	pub(crate) fn cast_camera_probe(
		&self,
		world: &RuntimeWorld,
		probe: CameraProbe<'_>,
		exclusions: &CameraExclusions,
		remaining: &mut usize,
	) -> Result<CameraProbeLimit, String> {
		if !exclusions.matches(world, probe.target, probe.topology_revision) {
			return Err("Camera exclusions do not match the candidate world topology".into());
		}
		let ray = prepare_camera_probe(&probe)?;
		consume_camera_budget(remaining, 1)?; // Account for the query itself.
		let mut visits = 0;
		let mut earliest: Option<(f64, &EntityKey)> = None;
		let mut endpoint_boxes: Vec<(&EntityKey, DVec3, DVec3)> = Vec::new();
		for entry in &self.ordered {
			// Charge before every filter, including the broad-phase stop test.
			charge_camera_visit(&mut visits, remaining)?;
			let obstacle = &self.records[entry.index];
			let min = DVec3::from_array(obstacle.bounds.min().map(f64::from));
			let max = DVec3::from_array(obstacle.bounds.max().map(f64::from));
			if min.x > ray.swept_max.x {
				break;
			}
			let excluded = *exclusions
				.by_entity_index
				.get(obstacle.entity_index)
				.ok_or_else(|| "Camera collider has a stale entity index".to_owned())?;
			if excluded
				|| obstacle.interaction.sensor
				|| obstacle.interaction.layers & probe.obstruction_mask == 0
				|| (0..3)
					.any(|axis| max[axis] < ray.swept_min[axis] || min[axis] > ray.swept_max[axis])
			{
				continue;
			}
			// Expand in the pivot's frame. At huge world coordinates adding a small
			// physical radius to an absolute face can round it away even in f64.
			let relative_min = min - ray.start;
			let relative_max = max - ray.start;
			let actual_radius = DVec3::splat(f64::from(probe.radius));
			let actual_min = relative_min - actual_radius;
			let actual_max = relative_max + actual_radius;
			if strictly_inside(DVec3::ZERO, actual_min, actual_max) {
				return Err(format!(
					"Camera pivot starts penetrating collider {}. Adjust the orbit pivot offset or follow lag, or move the collider.",
					obstacle.key.as_str()
				));
			}
			// This separating axis stays outside for the entire segment.
			// f32 rounding is monotonic away from the already-representable pivot.
			let moving_away = (0..3).any(|axis| {
				0.0 <= actual_min[axis] && ray.delta[axis] <= 0.0
					|| 0.0 >= actual_max[axis] && ray.delta[axis] >= 0.0
			});
			if moving_away {
				continue;
			}
			let padded_min = relative_min - DVec3::splat(ray.padding);
			let padded_max = relative_max + DVec3::splat(ray.padding);
			let fraction = if strictly_inside(DVec3::ZERO, padded_min, padded_max) {
				Some(0.0) // Safe physical origin, but no guard clearance to move inward.
			} else {
				segment_box_entry(DVec3::ZERO, ray.delta, padded_min, padded_max)?
			};
			if let Some(fraction) = fraction {
				let replace = earliest.as_ref().is_none_or(|(time, old_key)| {
					fraction < *time
						|| (fraction == *time && obstacle.key.as_str() < old_key.as_str())
				});
				if replace {
					earliest = Some((fraction, &obstacle.key));
				}
			}
			endpoint_boxes.push((&obstacle.key, actual_min, actual_max));
		}
		let requested = f64::from(probe.distance);
		let allowed = match earliest {
			Some((fraction, _)) => {
				let physical_length = ray.delta.length();
				let clearance_fraction = if physical_length > 0.0 {
					CAMERA_CLEARANCE / physical_length
				} else {
					0.0
				};
				requested * (fraction - clearance_fraction).max(0.0)
			}
			None => requested,
		};
		let distance = distance_round_down(allowed)?;
		let forward = DVec3::from_array(probe.forward.map(f64::from));
		let ideal_eye = ray.start - forward * f64::from(distance);
		let rounded_eye = ideal_eye.as_vec3();
		if !rounded_eye.is_finite() {
			return Err("Accepted camera endpoint is not finite".into());
		}
		let checked_eye = rounded_eye.as_dvec3() - ray.start;
		for (key, min, max) in endpoint_boxes {
			charge_camera_visit(&mut visits, remaining)?;
			if strictly_inside(checked_eye, min, max) {
				return Err(format!(
					"Rounded camera endpoint penetrates collider {}",
					key.as_str()
				));
			}
		}
		let hit = earliest.map(|(fraction, key)| CameraProbeHit {
			blocker: key.clone(),
			fraction,
		});
		Ok(CameraProbeLimit { distance, hit })
	}
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Contact {
	pub a: EntityKey,
	pub b: EntityKey,
	/// Unit normal from a toward b. Move a along the negative normal to separate.
	pub normal: [f32; 3],
	pub penetration: f32,
	/// True when either participant is a sensor. Sensors report overlaps and
	/// never block kinematic movement.
	#[serde(default)]
	pub sensor: bool,
}

impl Contact {
	fn ordered(
		first: &EntityKey,
		second: &EntityKey,
		normal: Vec3,
		penetration: f32,
		sensor: bool,
	) -> Self {
		let (a, b, normal) = if first.as_str() <= second.as_str() {
			(first, second, normal)
		} else {
			(second, first, -normal)
		};
		Self {
			a: a.clone(),
			b: b.clone(),
			normal: normal.to_array(),
			penetration,
			sensor,
		}
	}
}

fn sort_contacts(contacts: &mut [Contact]) {
	contacts.sort_by(|a, b| {
		a.a.as_str()
			.cmp(b.a.as_str())
			.then_with(|| a.b.as_str().cmp(b.b.as_str()))
	});
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
			if let Some((normal, penetration)) = contact_geometry(a, b) {
				if !normal.is_finite() || !penetration.is_finite() {
					return Err("Collision contact exceeds finite numeric limits".into());
				}
				if contacts.len() >= MAX_CONTACTS {
					return Err(format!("Collision query exceeds {MAX_CONTACTS} contacts"));
				}
				contacts.push(Contact::ordered(
					a.entity,
					b.entity,
					normal,
					penetration,
					a.interaction.sensor || b.interaction.sensor,
				));
			}
		}
	}
	sort_contacts(&mut contacts);
	Ok(contacts)
}

/// World-space debug geometry independent of renderer or frontend controls.
pub fn debug_shapes(data: &SceneData) -> Result<Vec<DebugShape>, String> {
	let matrices = checked_matrices(data)?;
	data.entities
		.iter()
		.zip(matrices)
		.filter_map(|(entity, matrix)| {
			entity
				.collider
				.as_ref()
				.map(|collider| debug_shape(&entity.key, collider, matrix))
		})
		.collect()
}

/// Effective collision geometry for one collider under its world transform.
pub fn debug_shape(
	entity: &EntityKey,
	collider: &ColliderData,
	world_matrix: Mat4,
) -> Result<DebugShape, String> {
	let (bounds, shape) = collider_geometry(collider, world_matrix)?;
	let min = bounds.min();
	let max = bounds.max();
	Ok(match shape {
		Shape::Box => DebugShape::Box {
			entity: entity.clone(),
			min,
			max,
		},
		Shape::Sphere { center, radius } => DebugShape::Sphere {
			entity: entity.clone(),
			center: center.to_array(),
			radius,
			min,
			max,
		},
	})
}

fn checked_matrices(data: &SceneData) -> Result<Vec<Mat4>, String> {
	if data.entities.len() > MAX_SCENE_ENTITIES {
		return Err(format!(
			"Collision scene exceeds {MAX_SCENE_ENTITIES} entities"
		));
	}
	data.world_matrices()
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

fn strictly_inside(point: DVec3, min: DVec3, max: DVec3) -> bool {
	(0..3).all(|axis| point[axis] > min[axis] && point[axis] < max[axis])
}

fn segment_box_entry(
	start: DVec3,
	delta: DVec3,
	min: DVec3,
	max: DVec3,
) -> Result<Option<f64>, String> {
	if !start.is_finite()
		|| !delta.is_finite()
		|| !min.is_finite()
		|| !max.is_finite()
		|| (0..3).any(|axis| min[axis] > max[axis])
	{
		return Err("Camera segment and box must be finite and ordered".into());
	}
	if strictly_inside(start, min, max) {
		return Err("Camera pivot starts penetrating a blocking collider".into());
	}
	if delta == DVec3::ZERO {
		return Ok(None);
	}
	let mut enter = f64::NEG_INFINITY;
	let mut exit = f64::INFINITY;
	for axis in 0..3 {
		let origin = start[axis];
		let direction = delta[axis];
		if direction == 0.0 {
			if origin < min[axis] || origin > max[axis] {
				return Ok(None);
			}
			continue;
		}
		let first = (min[axis] - origin) / direction;
		let second = (max[axis] - origin) / direction;
		enter = enter.max(first.min(second));
		exit = exit.min(first.max(second));
		if enter > exit {
			return Ok(None);
		}
	}
	// A boundary point moving away has exit == 0: allow it to leave.
	if exit <= 0.0 || enter > 1.0 || exit < enter.max(0.0) {
		return Ok(None);
	}
	Ok(Some(enter.max(0.0)))
}

struct PreparedCameraProbe {
	start: DVec3,
	delta: DVec3,
	padding: f64,
	swept_min: DVec3,
	swept_max: DVec3,
}

fn prepare_camera_probe(probe: &CameraProbe<'_>) -> Result<PreparedCameraProbe, String> {
	let start = DVec3::from_array(probe.pivot.map(f64::from));
	let forward = DVec3::from_array(probe.forward.map(f64::from));
	if !start.is_finite()
		|| !forward.is_finite()
		|| !probe.distance.is_finite()
		|| probe.distance < 0.0
		|| !probe.radius.is_finite()
		|| probe.radius <= 0.0
		|| (forward.length() - 1.0).abs() > 1.0e-5
	{
		return Err("Camera probe requires finite input and a unit viewing direction".into());
	}
	// Same multiply/add order as 10.3 orbit_pose, avoiding a different f32 ray.
	let delta = -forward * f64::from(probe.distance);
	let end = start + delta;
	if !end.is_finite() || end.abs().max_element() > f64::from(f32::MAX) {
		return Err("Requested camera endpoint is not representable as f32".into());
	}
	let coordinate_scale = start.abs().max(end.abs()).max_element().max(1.0);
	let rounding_guard = 4.0 * f64::from(f32::EPSILON) * coordinate_scale;
	let padding = f64::from(probe.radius) + rounding_guard;
	Ok(PreparedCameraProbe {
		start,
		delta,
		padding,
		swept_min: start.min(end) - DVec3::splat(padding),
		swept_max: start.max(end) + DVec3::splat(padding),
	})
}

fn distance_round_down(distance: f64) -> Result<f32, String> {
	if !distance.is_finite() || distance < 0.0 || distance > f64::from(f32::MAX) {
		return Err("Allowed camera distance is not representable".into());
	}
	let rounded = distance as f32;
	if f64::from(rounded) <= distance {
		return Ok(rounded);
	}
	Ok(f32::from_bits(rounded.to_bits() - 1))
}

fn charge_camera_visit(local: &mut usize, remaining: &mut usize) -> Result<(), String> {
	if *local == CAMERA_MAX_VISITS {
		return Err("Camera obstruction query exceeded 8192 work visits".into());
	}
	consume_camera_budget(remaining, 1)?;
	*local += 1;
	Ok(())
}

fn consume_camera_budget(remaining: &mut usize, count: usize) -> Result<(), String> {
	consume_candidates(remaining, count)
		.map_err(|_| "Camera obstruction exhausted the shared collision work budget".to_owned())
}

#[cfg(test)]
#[path = "../tests/unit/collision/camera_probe_math_tests.rs"]
mod camera_probe_math_tests;

#[cfg(test)]
#[path = "../tests/unit/collision/character_query_tests.rs"]
mod character_query_tests;
