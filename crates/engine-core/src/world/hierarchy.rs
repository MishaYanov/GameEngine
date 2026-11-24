use super::RuntimeWorld;
use crate::{
	EntityKey,
	components::ComponentId,
	scene_data::{EntityData, SceneData, TransformData},
};
use glam::Mat4;
use std::sync::Arc;

/// Select which transform a live object keeps when its parent changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeReparentMode {
	/// Keep the existing local position, rotation, and scale.
	KeepLocal,
	/// Keep the world transform if it can be represented by local TRS.
	KeepWorld,
}

impl RuntimeWorld {
	pub(crate) fn reparent_candidate(
		&self,
		key: &EntityKey,
		parent: Option<&EntityKey>,
		mode: RuntimeReparentMode,
	) -> Result<Option<Self>, String> {
		let child = self.entity_index(key).ok_or("Runtime child is missing")?;
		let new_parent = parent
			.map(|key| self.entity_index(key).ok_or("Runtime parent is missing"))
			.transpose()?;
		if self.parents[child] == new_parent {
			return Ok(None);
		}
		if !self.document.entities[child].active
			|| new_parent.is_some_and(|index| !self.document.entities[index].active)
		{
			return Err("Runtime reparent requires active objects".into());
		}
		let mut ancestor = new_parent;
		while let Some(index) = ancestor {
			if child == index {
				return Err("Runtime reparent would create a hierarchy cycle".into());
			}
			ancestor = self.parents[index];
		}
		let local = match mode {
			RuntimeReparentMode::KeepLocal => self.document.entities[child].transform.clone(),
			RuntimeReparentMode::KeepWorld => {
				let parent = new_parent.map_or(Mat4::IDENTITY, |index| self.matrices[index]);
				if parent.determinant() == 0.0 {
					return Err("Runtime parent world transform is singular".into());
				}
				TransformData::from_matrix(parent.inverse() * self.matrices[child])?
			}
		};
		let mut document = self.data().clone();
		document.organization.root_membership.remove(key);
		document.entities[child].parent = parent.cloned();
		document.entities[child].transform = local;
		self.rebuild_runtime_document(Arc::new(document)).map(Some)
	}

	pub(crate) fn rebuild_runtime_document(
		&self,
		document: Arc<SceneData>,
	) -> Result<Self, String> {
		let mut next = Self::new(document)?;
		next.instance_token = self.instance_token.clone();
		next.matrix_updates = self.matrix_updates.saturating_add(next.matrix_updates);
		Ok(next)
	}

	pub(crate) fn restore_runtime_document(
		&mut self,
		document: Arc<SceneData>,
		matrix_updates: u64,
	) -> Result<(), String> {
		*self = self.rebuild_runtime_document(document)?;
		self.matrix_updates = matrix_updates;
		Ok(())
	}

	pub(crate) fn set_runtime_emitter_enabled(
		&mut self,
		index: usize,
		component_id: &ComponentId,
		enabled: bool,
	) -> Result<Option<&EntityData>, String> {
		let emitter = self.document.entities[index]
			.particle_emitter
			.as_ref()
			.filter(|emitter| &emitter.id == component_id)
			.ok_or("Runtime particle emitter component is missing")?;
		if emitter.enabled == enabled {
			return Ok(None);
		}
		if std::iter::successors(self.parents[index], |&index| self.parents[index])
			.any(|index| self.document.entities[index].linked_prefab.is_some())
		{
			return Err(
				"Runtime emitter toggle requires an independent or unpacked component".into(),
			);
		}
		let entity = &mut Arc::make_mut(&mut self.document).entities[index];
		entity
			.particle_emitter
			.as_mut()
			.expect("verified emitter")
			.enabled = enabled;
		Ok(Some(entity))
	}
}
