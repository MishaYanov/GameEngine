use crate::{EntityKey, SceneData};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

pub const MAX_CHANGE_KEYS: usize = 4096;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WorldChange {
	pub changed_entities: Vec<EntityKey>,
	pub removed_entities: Vec<EntityKey>,
	pub topology_changed: bool,
	pub camera_changed: bool,
	pub lights_changed: bool,
	pub full_resync: bool,
}
impl WorldChange {
	pub fn is_empty(&self) -> bool {
		self.changed_entities.is_empty()
			&& self.removed_entities.is_empty()
			&& !self.topology_changed
			&& !self.camera_changed
			&& !self.lights_changed
			&& !self.full_resync
	}
	pub fn merge(&mut self, other: Self) {
		self.topology_changed |= other.topology_changed;
		self.camera_changed |= other.camera_changed;
		self.lights_changed |= other.lights_changed;
		self.full_resync |= other.full_resync;
		if self.full_resync {
			self.changed_entities.clear();
			self.removed_entities.clear();
			return;
		}
		let mut count = self.changed_entities.len() + self.removed_entities.len();
		if count > MAX_CHANGE_KEYS {
			self.full_resync = true;
			self.changed_entities.clear();
			self.removed_entities.clear();
			return;
		}
		for (target, source) in [
			(&mut self.changed_entities, other.changed_entities),
			(&mut self.removed_entities, other.removed_entities),
		] {
			if source.is_empty() {
				continue;
			}
			let mut keys: HashSet<_> = target.iter().cloned().collect();
			for key in source {
				if keys.insert(key.clone()) {
					if count == MAX_CHANGE_KEYS {
						self.full_resync = true;
						break;
					}
					target.push(key);
					count += 1;
				}
			}
			if self.full_resync {
				break;
			}
		}
		if self.full_resync {
			self.changed_entities.clear();
			self.removed_entities.clear();
		}
	}
	/// Used at authoring boundaries, not on each simulation tick.
	pub fn between(before: &SceneData, after: &SceneData) -> Self {
		let old: HashMap<_, _> = before.entities.iter().map(|e| (&e.key, e)).collect();
		let new: HashMap<_, _> = after.entities.iter().map(|e| (&e.key, e)).collect();
		let mut change = Self {
			camera_changed: before.camera != after.camera,
			lights_changed: before.lights != after.lights,
			..Self::default()
		};
		for entity in &after.entities {
			if old.get(&entity.key).is_none_or(|old| **old != *entity) {
				change.push_key(&entity.key, false);
			}
			change.topology_changed |= old.get(&entity.key).is_none_or(|old| {
				old.parent != entity.parent
					|| old.model_path != entity.model_path
					|| old.mesh_index != entity.mesh_index
					|| old.renderable != entity.renderable
					|| old.collider.is_some() != entity.collider.is_some()
			});
		}
		for entity in &before.entities {
			if !new.contains_key(&entity.key) {
				change.push_key(&entity.key, true);
				change.topology_changed = true;
			}
		}
		change
	}
	fn push_key(&mut self, key: &EntityKey, removed: bool) {
		if self.full_resync {
			return;
		}
		if self.changed_entities.len() + self.removed_entities.len() == MAX_CHANGE_KEYS {
			self.full_resync = true;
			self.changed_entities.clear();
			self.removed_entities.clear();
		} else if removed {
			self.removed_entities.push(key.clone());
		} else {
			self.changed_entities.push(key.clone());
		}
	}
}

/// Retains membership indexes across the actions and catch-up steps of one tick.
/// Ordering and the shared key budget match sequential `WorldChange::merge` calls.
#[derive(Default)]
pub(crate) struct ChangeAccumulator {
	change: WorldChange,
	changed: HashSet<EntityKey>,
	removed: HashSet<EntityKey>,
}
impl ChangeAccumulator {
	pub(crate) fn merge(&mut self, other: WorldChange) {
		self.change.topology_changed |= other.topology_changed;
		self.change.camera_changed |= other.camera_changed;
		self.change.lights_changed |= other.lights_changed;
		if self.change.full_resync {
			return;
		}
		if other.full_resync {
			self.require_resync();
			return;
		}
		let mut count = self.change.changed_entities.len() + self.change.removed_entities.len();
		for (target, keys, source) in [
			(
				&mut self.change.changed_entities,
				&mut self.changed,
				other.changed_entities,
			),
			(
				&mut self.change.removed_entities,
				&mut self.removed,
				other.removed_entities,
			),
		] {
			for key in source {
				if keys.contains(&key) {
					continue;
				}
				if count == MAX_CHANGE_KEYS {
					self.change.full_resync = true;
					break;
				}
				keys.insert(key.clone());
				target.push(key);
				count += 1;
			}
			if self.change.full_resync {
				break;
			}
		}
		if self.change.full_resync {
			self.require_resync();
		}
	}
	fn require_resync(&mut self) {
		self.change.full_resync = true;
		self.change.changed_entities.clear();
		self.change.removed_entities.clear();
		self.changed.clear();
		self.removed.clear();
	}
	pub(crate) fn finish(self) -> WorldChange {
		self.change
	}
}
