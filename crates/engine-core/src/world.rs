//! CPU world owned by a running authored project. GPU resources never live here.
mod changes;
pub(crate) use changes::ChangeAccumulator;
pub use changes::{MAX_CHANGE_KEYS, WorldChange};

use crate::game_camera::CameraTargetSnapshot;
use crate::runtime_lifecycle::RuntimeEntityHandle;
use crate::{
	EntityKey,
	scene_data::{MaterialOverrideData, SceneData, TransformData},
};
use glam::Mat4;
use std::{
	collections::{HashMap, HashSet},
	sync::{Arc, OnceLock},
};

pub struct RuntimeWorld {
	// Standalone consumers can distinguish two independently constructed worlds
	// even when both contain the same authored keys. Ordinary edits retain this token.
	instance_token: OnceLock<Arc<()>>,
	document: Arc<SceneData>,
	indices: HashMap<EntityKey, usize>,
	parents: Vec<Option<usize>>,
	children: Vec<Vec<usize>>,
	matrices: Vec<Mat4>,
	matrix_updates: u64,
}
impl RuntimeWorld {
	pub fn new(document: Arc<SceneData>) -> Result<Self, String> {
		let matrices = document.validated_world_matrices()?;
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
			instance_token: OnceLock::new(),
			document,
			indices,
			parents,
			children,
			matrices,
			matrix_updates,
		})
	}
	pub(crate) fn instance_token(&self) -> &Arc<()> {
		self.instance_token.get_or_init(|| Arc::new(()))
	}

	pub fn data(&self) -> &SceneData {
		&self.document
	}
	pub fn document(&self) -> Arc<SceneData> {
		Arc::clone(&self.document)
	}
	pub(crate) fn remove_runtime_entities(
		&mut self,
		keys: &HashSet<EntityKey>,
	) -> Result<WorldChange, String> {
		let mut data = self.data().clone();
		data.entities.retain(|entity| !keys.contains(&entity.key));
		data.bindings.retain(|_, key| !keys.contains(key));
		data.camera.clear_removed_targets(keys);
		for entity in &mut data.entities {
			entity.native_behaviors.retain(|component| {
				component
					.behavior
					.entity_references()
					.iter()
					.all(|key| !keys.contains(*key))
			});
		}
		let next = Self::new(Arc::new(data))?;
		*self = next;
		let mut removed: Vec<_> = keys.iter().cloned().collect();
		removed.sort_by(|a, b| a.as_str().cmp(b.as_str()));
		Ok(WorldChange {
			removed_entities: removed,
			topology_changed: true,
			full_resync: true,
			..WorldChange::default()
		})
	}
	pub fn set_visible(&mut self, key: &EntityKey, visible: bool) -> Result<WorldChange, String> {
		let index = self
			.entity_index(key)
			.ok_or("Visibility target is missing")?;
		if self.document.entities[index].renderable == visible {
			return Ok(WorldChange::default());
		}
		Arc::make_mut(&mut self.document).entities[index].renderable = visible;
		Ok(WorldChange {
			changed_entities: vec![key.clone()],
			topology_changed: true,
			..WorldChange::default()
		})
	}
	pub fn set_collision_enabled(
		&mut self,
		key: &EntityKey,
		enabled: bool,
	) -> Result<WorldChange, String> {
		let index = self
			.entity_index(key)
			.ok_or("Collision target is missing")?;
		if self.document.entities[index].collision_enabled == enabled {
			return Ok(WorldChange::default());
		}
		Arc::make_mut(&mut self.document).entities[index].collision_enabled = enabled;
		Ok(WorldChange {
			changed_entities: vec![key.clone()],
			..WorldChange::default()
		})
	}
	pub fn set_material(
		&mut self,
		key: &EntityKey,
		material: Option<MaterialOverrideData>,
	) -> Result<WorldChange, String> {
		let index = self.entity_index(key).ok_or("Material target is missing")?;
		if material.as_ref().is_some_and(|value| {
			value
				.base_color_factor
				.iter()
				.any(|channel| !channel.is_finite() || !(0.0..=1.0).contains(channel))
		}) {
			return Err("Runtime material channels must be finite and within 0..=1".into());
		}
		if self.document.entities[index].material == material {
			return Ok(WorldChange::default());
		}
		Arc::make_mut(&mut self.document).entities[index].material = material;
		Ok(WorldChange {
			changed_entities: vec![key.clone()],
			..WorldChange::default()
		})
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
	pub(crate) fn entity_world_position(&self, key: &EntityKey) -> Option<[f32; 3]> {
		let index = self.entity_index(key)?;
		Some(self.matrices[index].w_axis.truncate().to_array())
	}

	/// Sample world-space translation after the caller has validated the lifecycle handle.
	pub fn camera_target_snapshot(
		&self,
		handle: &RuntimeEntityHandle,
	) -> Result<CameraTargetSnapshot, String> {
		let world_position = self
			.entity_world_position(&handle.key)
			.ok_or("Camera target is absent from the current world")?;
		if !world_position.iter().all(|v| v.is_finite()) {
			return Err("Camera target world position is not finite".into());
		}
		Ok(CameraTargetSnapshot {
			handle: handle.clone(),
			world_position,
		})
	}
}

#[cfg(test)]
#[path = "../tests/unit/world.rs"]
mod tests;
