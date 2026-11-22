use std::collections::{HashMap, HashSet};

use super::{
	EntityId, MAX_SCENE_RENDER_OBJECTS, RetainedEntity, Scene, SceneEntry,
	validate_animation_placement, validate_animation_placements,
};
use crate::{EntityKey, SceneData, model_instance::ModelInstance};

impl Scene {
	/// Update CPU draw state without retiring GPU models when topology and model
	/// sources are unchanged. Returns false when the GPU factory must rebuild.
	pub fn synchronize(&mut self, data: &SceneData) -> Result<bool, String> {
		let world = data.validated_world_matrices()?;
		validate_animation_placements(data, &world)?;
		if !self.synchronize_entries(data) {
			return Ok(false);
		}
		*self.world_cache.get_mut() = Some(world);
		Ok(true)
	}

	fn synchronize_entries(&mut self, data: &SceneData) -> bool {
		if self.entries.len() != data.entities.len()
			|| !self
				.entries
				.iter()
				.zip(&data.entities)
				.all(|(entry, entity)| {
					entry.key.as_ref() == Some(&entity.key) && entry.can_reuse_model(entity)
				}) {
			return false;
		}
		for (entry, entity) in self.entries.iter_mut().zip(&data.entities) {
			entry.parent.clone_from(&entity.parent);
			entry.apply_document_values(entity);
		}
		self.authored_children = None;
		self.bindings.clone_from(&data.bindings);
		self.lights.clone_from(&data.lights);
		self.set_organization(&data.organization);
		self.synchronize_camera_settings(&data.camera);
		true
	}

	/// Apply a complete dirty set from the same validated authored revision.
	/// The caller must request a full resync after lost events or document replacement.
	/// Parent/asset/topology changes return false before changing scene contents.
	pub fn synchronize_authored(
		&mut self,
		document: engine_core::scene_data::ValidatedScene<'_>,
		changes: &engine_core::world::WorldChange,
	) -> Result<bool, String> {
		let data = document.data();
		if changes.full_resync
			|| changes.topology_changed
			|| !changes.removed_entities.is_empty()
			|| self.entries.len() != data.entities.len()
		{
			return Ok(false);
		}
		let mut changed = Vec::with_capacity(changes.changed_entities.len());
		for key in &changes.changed_entities {
			let Some(index) = self
				.entity_by_key(key)
				.and_then(|id| self.id_indices.get(&id))
				.copied()
			else {
				return Ok(false);
			};
			let entity = &data.entities[index];
			let entry = &self.entries[index];
			if entry.key.as_ref() != Some(&entity.key)
				|| entry.parent != entity.parent
				|| !entry.can_reuse_model(entity)
			{
				return Ok(false);
			}
			changed.push(index);
		}
		// A hierarchy index is rebuilt only after structural/full synchronization.
		if self.authored_children.is_none() {
			let mut children = vec![Vec::new(); self.entries.len()];
			for (index, entry) in self.entries.iter().enumerate() {
				if let Some(parent) = entry.parent.as_ref() {
					let parent = self
						.entity_by_key(parent)
						.and_then(|id| self.id_indices.get(&id))
						.ok_or("Native scene parent is missing")?;
					children[*parent].push(index);
				}
			}
			self.authored_children = Some(children);
		}
		let children = self
			.authored_children
			.as_ref()
			.expect("hierarchy initialized");
		let mut affected = HashSet::new();
		let mut pending = Vec::new();
		for &index in &changed {
			let old = self.entries[index].instance.transform();
			let new = &data.entities[index].transform;
			if old.position != new.position
				|| old.rotation != new.rotation
				|| old.scale != new.scale
			{
				pending.push(index);
			}
		}
		while let Some(index) = pending.pop() {
			if affected.insert(index) {
				pending.extend_from_slice(&children[index]);
			}
		}
		let mut matrices = HashMap::with_capacity(affected.len());
		if !affected.is_empty() {
			let cached = self.cached_world_matrices()?;
			for &index in &affected {
				let mut chain = Vec::new();
				let mut node = index;
				while affected.contains(&node) && !matrices.contains_key(&node) {
					if self.entries[node].key.as_ref() != Some(&data.entities[node].key)
						|| self.entries[node].parent != data.entities[node].parent
					{
						return Ok(false);
					}
					chain.push(node);
					let Some(parent) = self.entries[node].parent.as_ref() else {
						break;
					};
					node = self.id_indices[&self.key_ids[parent]];
				}
				while let Some(node) = chain.pop() {
					let local = data.entities[node].transform.matrix();
					let matrix = self.entries[node].parent.as_ref().map_or(local, |parent| {
						let parent = self.id_indices[&self.key_ids[parent]];
						matrices.get(&parent).copied().unwrap_or(cached[parent]) * local
					});
					matrices.insert(node, matrix);
				}
			}
		}
		for (&index, &matrix) in &matrices {
			if self.entries[index].animation.is_some() {
				validate_animation_placement(&self.entries[index].name, matrix)?;
			}
		}
		for index in changed {
			let entity = &data.entities[index];
			let entry = &mut self.entries[index];
			entry.apply_document_values(entity);
		}
		if let Some(cache) = self.world_cache.get_mut() {
			for (index, matrix) in matrices {
				cache[index] = matrix;
			}
		}
		// Binding changes currently have no dedicated WorldChange flag.
		if self.bindings != data.bindings {
			self.bindings.clone_from(&data.bindings);
		}
		if self.organization != data.organization {
			self.set_organization(&data.organization);
		}
		if changes.lights_changed {
			self.lights.clone_from(&data.lights);
		}
		if changes.camera_changed {
			self.synchronize_camera_settings(&data.camera);
		}
		Ok(true)
	}

	/// Consume a RuntimeWorld dirty set without rebuilding hierarchy matrices
	/// or scanning every entity on each simulation tick. RuntimeWorld changes
	/// include affected descendants; structural/full changes use the factory path.
	pub fn synchronize_world(
		&mut self,
		world: &engine_core::world::RuntimeWorld,
		changes: &engine_core::world::WorldChange,
	) -> Result<bool, String> {
		if changes.full_resync
			|| changes.topology_changed
			|| !changes.removed_entities.is_empty()
			|| self.entries.len() != world.data().entities.len()
		{
			if !self.synchronize_entries(world.data()) {
				let all_prepared = world.data().entities.iter().all(|entity| {
					self.entity_by_key(&entity.key).is_some_and(|id| {
						let entry = &self.entries[self.id_indices[&id]];
						entry.matches_asset_source(entity)
					})
				});
				if !all_prepared {
					return Ok(false);
				}
				self.synchronize_retained(world.data(), &HashMap::new(), None)?;
			}
			let source = world.world_matrices();
			match self.world_cache.get_mut() {
				Some(cache) if cache.len() == source.len() => cache.copy_from_slice(source),
				cache => *cache = Some(source.to_vec()),
			}
			return Ok(true);
		}
		let mut updates = Vec::with_capacity(changes.changed_entities.len());
		for key in &changes.changed_entities {
			let Some(id) = self.entity_by_key(key) else {
				return Ok(false);
			};
			let Some(cpu_index) = world.entity_index(key) else {
				return Ok(false);
			};
			let gpu_index = self.id_indices[&id];
			let entity = &world.data().entities[cpu_index];
			let entry = &self.entries[gpu_index];
			if !entry.can_reuse_model(entity) {
				return Ok(false);
			}
			updates.push((gpu_index, cpu_index));
		}
		if self
			.world_cache
			.get_mut()
			.as_ref()
			.is_none_or(|matrices| matrices.len() != world.world_matrices().len())
		{
			*self.world_cache.get_mut() = Some(world.world_matrices().to_vec());
		}
		for (gpu_index, cpu_index) in updates {
			let entity = &world.data().entities[cpu_index];
			let entry = &mut self.entries[gpu_index];
			entry.parent.clone_from(&entity.parent);
			entry.apply_document_values(entity);
			self.world_cache
				.get_mut()
				.as_mut()
				.expect("world cache initialized")[gpu_index] = world.world_matrices()[cpu_index];
		}
		if changes.lights_changed {
			self.lights.clone_from(&world.data().lights);
		}
		if changes.camera_changed {
			self.synchronize_camera_settings(&world.data().camera);
		}
		Ok(true)
	}

	/// Apply a core authoring document while retaining GPU resources and stable
	/// native IDs. The core owns history; this adapter owns only resource Arcs.
	pub(crate) fn synchronize_retained(
		&mut self,
		data: &SceneData,
		retained: &HashMap<EntityKey, RetainedEntity>,
		supplied_model: Option<&ModelInstance>,
	) -> Result<(), String> {
		let world = data.validated_world_matrices()?;
		validate_animation_placements(data, &world)?;
		if self.synchronize_entries(data) {
			*self.world_cache.get_mut() = Some(world);
			return Ok(());
		}
		let mut next = Scene {
			animation_root: self.animation_root.clone(),
			next_id: self.next_id,
			bindings: data.bindings.clone(),
			lights: data.lights.clone(),
			organization: data.organization.clone(),
			..Scene::default()
		};
		next.synchronize_camera_settings(&data.camera);
		let mut object_count = 0usize;
		for entity in &data.entities {
			let existing = self
				.entity_by_key(&entity.key)
				.and_then(|id| self.id_indices.get(&id))
				.map(|&index| &self.entries[index])
				.or_else(|| retained.get(&entity.key).map(|retained| &retained.0));
			let mut entry = if let Some(entry) = existing {
				if !entry.can_reuse_model(entity) {
					return Err("Native resource adapter needs a model factory to change an existing asset source".into());
				}
				entry.clone()
			} else {
				let template = retained
					.values()
					.map(|retained| &retained.0)
					.find(|entry| entry.can_reuse_model(entity));
				let instance = if let Some(instance) = supplied_model {
					instance.clone()
				} else if let Some(template) = template {
					template.instance.clone()
				} else if !entity.renderable {
					ModelInstance::new(
						std::sync::Arc::new(crate::assets::Model::new(Vec::new())),
						crate::Transform::default(),
					)
				} else {
					return Err(format!(
						"GPU model {:?} mesh {} is not available to restore; load it through AssetManager first",
						entity.model_path,
						entity.mesh_index.unwrap_or(0)
					));
				};
				let id = EntityId(next.next_id);
				next.next_id = next
					.next_id
					.checked_add(1)
					.ok_or("Scene entity IDs exhausted")?;
				SceneEntry {
					id,
					key: Some(entity.key.clone()),
					name: entity.name.clone(),
					model_path: Some(entity.model_path.clone()),
					asset_id: entity.asset_id.clone(),
					collision_enabled: entity.collision_enabled,
					instance,
					parent: entity.parent.clone(),
					material: entity.material.clone(),
					collider: entity.collider.clone(),
					mesh_index: entity.mesh_index,
					renderable: entity.renderable,
					component_ids: entity.component_ids.clone(),
					native_behaviors: entity.native_behaviors.clone(),
					linked_prefab: entity.linked_prefab.clone(),
					animation: entity.animation.clone(),
					interactable: entity.interactable.clone(),
					character_controller: entity.character_controller.clone(),
				}
			};
			entry.parent.clone_from(&entity.parent);
			entry.apply_document_values(entity);
			object_count = object_count
				.checked_add(entry.instance.model().parts().len())
				.ok_or("Scene render-object count overflow")?;
			if object_count > MAX_SCENE_RENDER_OBJECTS {
				return Err(format!(
					"Scene exceeds {MAX_SCENE_RENDER_OBJECTS} render objects"
				));
			}
			next.id_indices.insert(entry.id, next.entries.len());
			next.key_ids.insert(entity.key.clone(), entry.id);
			next.entries.push(entry);
		}
		*next.world_cache.get_mut() = Some(world);
		*self = next;
		Ok(())
	}
}
