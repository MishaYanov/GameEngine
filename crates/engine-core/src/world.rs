//! CPU world owned by a running authored project. GPU resources never live here.
use crate::{
	EntityKey,
	scene_data::{SceneData, TransformData},
};
use glam::Mat4;
use serde::{Deserialize, Serialize};
use std::{
	collections::{HashMap, HashSet},
	sync::Arc,
};

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

pub struct RuntimeWorld {
	document: Arc<SceneData>,
	indices: HashMap<EntityKey, usize>,
	parents: Vec<Option<usize>>,
	children: Vec<Vec<usize>>,
	matrices: Vec<Mat4>,
	matrix_updates: u64,
}
impl RuntimeWorld {
	pub fn new(document: Arc<SceneData>) -> Result<Self, String> {
		document.validate()?;
		let matrices = document.world_matrices()?;
		let indices: HashMap<_, _> = document
			.entities
			.iter()
			.enumerate()
			.map(|(i, e)| (e.key.clone(), i))
			.collect();
		let mut parents = vec![None; document.entities.len()];
		let mut children = vec![Vec::new(); document.entities.len()];
		for (index, entity) in document.entities.iter().enumerate() {
			if let Some(parent) = &entity.parent {
				let parent = *indices.get(parent).ok_or("Missing world parent")?;
				parents[index] = Some(parent);
				children[parent].push(index);
			}
		}
		let matrix_updates = matrices.len() as u64;
		Ok(Self {
			document,
			indices,
			parents,
			children,
			matrices,
			matrix_updates,
		})
	}
	pub fn data(&self) -> &SceneData {
		&self.document
	}
	pub fn document(&self) -> Arc<SceneData> {
		Arc::clone(&self.document)
	}
	pub fn entity_index(&self, key: &EntityKey) -> Option<usize> {
		self.indices.get(key).copied()
	}
	pub fn world_matrices(&self) -> &[Mat4] {
		&self.matrices
	}
	pub fn matrix_updates(&self) -> u64 {
		self.matrix_updates
	}
	pub fn parent_matrix(&self, index: usize) -> Result<Mat4, String> {
		let parent = self
			.parents
			.get(index)
			.ok_or("World entity index is out of bounds")?;
		Ok(parent.map_or(Mat4::IDENTITY, |index| self.matrices[index]))
	}
	pub fn subtree_indices(&self, index: usize) -> Result<Vec<usize>, String> {
		if index >= self.children.len() {
			return Err("World entity index is out of bounds".into());
		}
		let mut stack = vec![index];
		let mut result = Vec::new();
		while let Some(index) = stack.pop() {
			result.push(index);
			stack.extend(self.children[index].iter().rev().copied());
		}
		Ok(result)
	}
	/// Stage the affected subtree before committing, preserving atomic failure.
	pub fn set_transform(
		&mut self,
		key: &EntityKey,
		transform: TransformData,
	) -> Result<WorldChange, String> {
		let index = self
			.entity_index(key)
			.ok_or("World entity does not exist")?;
		if self.document.entities[index].transform == transform {
			return Ok(WorldChange::default());
		}
		let local = transform.matrix();
		if !transform
			.position
			.iter()
			.chain(transform.rotation.iter())
			.chain(transform.scale.iter())
			.all(|v| v.is_finite())
			|| !local.is_finite()
		{
			return Err("World transform must remain finite".into());
		}
		let root = self.parent_matrix(index)? * local;
		let mut pending = vec![(index, root)];
		let mut staged = Vec::new();
		while let Some((current, matrix)) = pending.pop() {
			if !matrix.is_finite() {
				return Err("World transform overflows the supported finite range".into());
			}
			staged.push((current, matrix));
			for &child in self.children[current].iter().rev() {
				pending.push((
					child,
					matrix * self.document.entities[child].transform.matrix(),
				));
			}
		}
		let mut change = WorldChange::default();
		if staged.len() > MAX_CHANGE_KEYS {
			change.full_resync = true;
		} else {
			change.changed_entities = staged
				.iter()
				.map(|(i, _)| self.document.entities[*i].key.clone())
				.collect();
		}
		Arc::make_mut(&mut self.document).entities[index].transform = transform;
		self.matrix_updates = self.matrix_updates.saturating_add(staged.len() as u64);
		for (index, matrix) in staged {
			self.matrices[index] = matrix;
		}
		Ok(change)
	}
}
