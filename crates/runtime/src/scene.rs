use std::{
	cell::{Ref, RefCell},
	collections::{BTreeMap, HashMap, HashSet},
};

use crate::entity_key::EntityKey;
use crate::model_instance::ModelInstance;
use crate::scene_data::{
	CURRENT_SCHEMA_VERSION, CameraData, ColliderData, EntityData, LightData, LightKind,
	MaterialOverrideData, TransformData, resolve_world_matrices,
};
use crate::{Camera, SceneData};
use renderer::vulkan::{ModelPushConstants, RenderObject};

/// Bounds CPU submission storage independently of the renderer's GPU buffers.
pub const MAX_SCENE_RENDER_OBJECTS: usize = 65_536;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EntityId(u64);

#[derive(Clone)]
struct SceneEntry {
	id: EntityId,
	key: Option<EntityKey>,
	name: String,
	model_path: Option<String>,
	instance: ModelInstance,
	parent: Option<EntityKey>,
	material: Option<MaterialOverrideData>,
	collider: Option<ColliderData>,
	mesh_index: Option<usize>,
	renderable: bool,
}

#[derive(Clone, Default)]
pub struct Scene {
	entries: Vec<SceneEntry>,
	bindings: BTreeMap<String, EntityKey>,
	next_id: u64,
	lights: Vec<LightData>,
	id_indices: HashMap<EntityId, usize>,
	key_ids: HashMap<EntityKey, EntityId>,
	world_cache: RefCell<Option<Vec<glam::Mat4>>>,
	authored_children: Option<Vec<Vec<usize>>>,
}

#[derive(Clone)]
pub struct EntitySnapshot {
	entry: SceneEntry,
	index: usize,
	bindings: BTreeMap<String, EntityKey>,
}

/// Resource retention does not need the binding/history payload of a snapshot.
#[derive(Clone)]
pub(crate) struct RetainedEntity(SceneEntry);

impl EntitySnapshot {
	pub fn id(&self) -> EntityId {
		self.entry.id
	}
	pub fn key(&self) -> Option<&EntityKey> {
		self.entry.key.as_ref()
	}
	pub fn name(&self) -> &str {
		&self.entry.name
	}
	pub fn instance(&self) -> &ModelInstance {
		&self.entry.instance
	}
}

impl Scene {
	pub(crate) fn retain_entity(&self, id: EntityId) -> Result<RetainedEntity, String> {
		let index = self
			.id_indices
			.get(&id)
			.ok_or("Cannot retain a missing entity")?;
		Ok(RetainedEntity(self.entries[*index].clone()))
	}
	pub fn rename(&mut self, id: EntityId, name: impl Into<String>) -> Result<(), String> {
		let name = name.into();
		if name.trim().is_empty() || name.chars().any(char::is_control) {
			return Err("Entity name must contain visible text and no control characters".into());
		}
		let index = *self
			.id_indices
			.get(&id)
			.ok_or("Cannot rename a missing entity")?;
		self.entries[index].name = name;
		Ok(())
	}

	/// Compare authored content without allocating a second serialization each frame.
	pub fn matches_data(&self, data: &SceneData, camera: &Camera) -> bool {
		let saved_camera = &data.camera;
		self.bindings == data.bindings
			&& self.lights == data.lights
			&& self.entries.len() == data.entities.len()
			&& self
				.entries
				.iter()
				.zip(&data.entities)
				.all(|(entry, saved)| {
					let transform = entry.instance.transform();
					entry.key.as_ref() == Some(&saved.key)
						&& entry.name == saved.name
						&& entry.model_path.as_deref() == Some(saved.model_path.as_str())
						&& transform.position == saved.transform.position
						&& transform.rotation == saved.transform.rotation
						&& transform.scale == saved.transform.scale
						&& entry.parent == saved.parent
						&& entry.material == saved.material
						&& entry.collider == saved.collider
						&& entry.mesh_index == saved.mesh_index
						&& entry.renderable == saved.renderable
				}) && camera.position == saved_camera.position
			&& camera.yaw == saved_camera.yaw
			&& camera.pitch == saved_camera.pitch
			&& camera.vertical_fov == saved_camera.vertical_fov
			&& camera.near == saved_camera.near_plane
			&& camera.far == saved_camera.far_plane
	}
	pub fn snapshot(&self, id: EntityId) -> Result<EntitySnapshot, String> {
		let index = *self
			.id_indices
			.get(&id)
			.ok_or_else(|| format!("Cannot snapshot missing entity {id:?}"))?;
		let entry = self.entries[index].clone();
		let bindings = self
			.bindings
			.iter()
			.filter(|(_, key)| entry.key.as_ref() == Some(*key))
			.map(|(name, key)| (name.clone(), key.clone()))
			.collect();
		Ok(EntitySnapshot {
			entry,
			index,
			bindings,
		})
	}

	pub fn restore(&mut self, snapshot: &EntitySnapshot) -> Result<EntityId, String> {
		let entry = &snapshot.entry;
		if entry
			.parent
			.as_ref()
			.is_some_and(|parent| self.entity_by_key(parent).is_none())
		{
			return Err("Cannot restore an entity whose parent is missing".into());
		}
		if self.contains(entry.id)
			|| entry
				.key
				.as_ref()
				.is_some_and(|key| self.entity_by_key(key).is_some())
		{
			return Err("Cannot restore an entity whose ID or key is already in use".to_string());
		}
		for (name, key) in &snapshot.bindings {
			if self
				.bindings
				.get(name)
				.is_some_and(|existing| existing != key)
			{
				return Err(format!(
					"Cannot restore entity: binding {name:?} has another target"
				));
			}
		}
		let next_id = entry
			.id
			.0
			.checked_add(1)
			.ok_or("Scene entity IDs exhausted")?;
		let index = snapshot.index.min(self.entries.len());
		self.entries.insert(index, entry.clone());
		for (position, entry) in self.entries.iter().enumerate().skip(index) {
			self.id_indices.insert(entry.id, position);
		}
		if let Some(key) = &entry.key {
			self.key_ids.insert(key.clone(), entry.id);
		}
		*self.world_cache.get_mut() = None;
		self.bindings.extend(snapshot.bindings.clone());
		self.authored_children = None;
		self.next_id = self.next_id.max(next_id);
		Ok(entry.id)
	}

	pub fn spawn(&mut self, instance: ModelInstance) -> EntityId {
		self.spawn_named("entity", instance)
	}

	pub fn name(&self, id: EntityId) -> Option<&str> {
		let name = self.entries.get(*self.id_indices.get(&id)?)?.name.as_str();
		Some(name)
	}

	pub fn spawn_named(&mut self, name: impl Into<String>, instance: ModelInstance) -> EntityId {
		self.spawn_entry(None, name.into(), None, instance)
	}

	pub fn spawn_model(
		&mut self,
		name: impl Into<String>,
		model_path: impl Into<String>,
		instance: ModelInstance,
	) -> EntityId {
		self.spawn_entry(None, name.into(), Some(model_path.into()), instance)
	}

	pub fn spawn_keyed_model(
		&mut self,
		key: EntityKey,
		name: impl Into<String>,
		model_path: impl Into<String>,
		instance: ModelInstance,
	) -> Result<EntityId, String> {
		key.validate()?;
		if self.entity_by_key(&key).is_some() {
			return Err(format!("Duplicate entity key {:?}", key.as_str()));
		}
		Ok(self.spawn_entry(Some(key), name.into(), Some(model_path.into()), instance))
	}

	pub fn key(&self, id: EntityId) -> Option<&EntityKey> {
		self.entries.get(*self.id_indices.get(&id)?)?.key.as_ref()
	}

	pub fn entity_by_key(&self, key: &EntityKey) -> Option<EntityId> {
		self.key_ids.get(key).copied()
	}

	pub fn set_binding(&mut self, name: impl Into<String>, id: EntityId) -> Result<(), String> {
		let name = name.into();
		if name.trim().is_empty() {
			return Err("Binding name cannot be empty or whitespace".to_string());
		}
		let key = self
			.key(id)
			.ok_or_else(|| {
				format!("Cannot bind {name:?}: entity {id:?} is missing or has no persistent key")
			})?
			.clone();
		self.bindings.insert(name, key);
		Ok(())
	}

	pub fn binding(&self, name: &str) -> Option<EntityId> {
		self.entity_by_key(self.bindings.get(name)?)
	}

	fn spawn_entry(
		&mut self,
		key: Option<EntityKey>,
		name: String,
		model_path: Option<String>,
		instance: ModelInstance,
	) -> EntityId {
		let id = EntityId(self.next_id);

		self.next_id = self
			.next_id
			.checked_add(1)
			.expect("scene entity IDs exhausted");

		self.id_indices.insert(id, self.entries.len());
		if let Some(key) = &key {
			self.key_ids.insert(key.clone(), id);
		}
		*self.world_cache.get_mut() = None;
		self.entries.push(SceneEntry {
			id,
			key,
			name,
			model_path,
			instance,
			parent: None,
			material: None,
			collider: None,
			mesh_index: None,
			renderable: true,
		});
		self.authored_children = None;

		id
	}

	pub fn get_mut(&mut self, id: EntityId) -> Option<&mut ModelInstance> {
		let index = *self.id_indices.get(&id)?;
		*self.world_cache.get_mut() = None;
		self.entries.get_mut(index).map(|entry| &mut entry.instance)
	}

	pub fn remove(&mut self, id: EntityId) -> Option<ModelInstance> {
		let index = *self.id_indices.get(&id)?;
		// The legacy single-entity operation cannot orphan children. The document
		// command facade owns atomic subtree deletion and its undo record.
		if let Some(key) = &self.entries[index].key
			&& self
				.entries
				.iter()
				.any(|entry| entry.parent.as_ref() == Some(key))
		{
			return None;
		}
		let entry = self.entries.remove(index);
		self.authored_children = None;
		self.id_indices.remove(&id);
		for (position, entry) in self.entries.iter().enumerate().skip(index) {
			self.id_indices.insert(entry.id, position);
		}
		*self.world_cache.get_mut() = None;
		if let Some(key) = entry.key.as_ref() {
			self.key_ids.remove(key);
			self.bindings.retain(|_, target| target != key);
		}

		Some(entry.instance)
	}

	pub fn instances(&self) -> impl Iterator<Item = &ModelInstance> {
		self.entries.iter().map(|entry| &entry.instance)
	}

	pub fn render_objects(&self) -> Vec<RenderObject<'_>> {
		self.try_render_objects()
			.expect("Scene exceeds its rendering budget or has an invalid hierarchy")
	}

	pub fn try_render_objects(&self) -> Result<Vec<RenderObject<'_>>, String> {
		self.try_render_visible(None).map(|(objects, _)| objects)
	}

	pub fn render_visible(
		&self,
		frustum: Option<&crate::visibility::Frustum>,
	) -> (Vec<RenderObject<'_>>, crate::visibility::SceneRenderStats) {
		self.try_render_visible(frustum)
			.expect("Scene exceeds its rendering budget or has an invalid hierarchy")
	}

	/// Fallible rendering entry point for production callers. Check the entire
	/// scene's part count before growing a submission vector, even with culling.
	pub fn try_render_visible(
		&self,
		frustum: Option<&crate::visibility::Frustum>,
	) -> Result<(Vec<RenderObject<'_>>, crate::visibility::SceneRenderStats), String> {
		let mut part_count = 0usize;
		for entry in self.entries.iter().filter(|entry| entry.renderable) {
			part_count = part_count
				.checked_add(entry.instance.model().parts().len())
				.ok_or("Scene render-object count overflow")?;
			if part_count > MAX_SCENE_RENDER_OBJECTS {
				return Err(format!(
					"Scene requires more than {MAX_SCENE_RENDER_OBJECTS} render objects"
				));
			}
		}
		let mut objects = Vec::new();
		let mut stats = crate::visibility::SceneRenderStats::default();

		let world_matrices = self.cached_world_matrices()?;
		for (entry, matrix) in self.entries.iter().zip(world_matrices.iter().copied()) {
			let instance = &entry.instance;
			stats.total_entities += 1;
			if !entry.renderable {
				continue;
			}
			let outside = frustum.is_some_and(|frustum| {
				instance
					.model()
					.bounds()
					.and_then(|bounds| bounds.transformed(matrix).ok())
					.is_some_and(|bounds| !frustum.intersects(bounds))
			});
			if outside {
				stats.culled_entities += 1;
				continue;
			}
			stats.visible_entities += 1;
			let mut constants = ModelPushConstants::identity();
			constants.model = matrix.to_cols_array();
			if let Some(material) = &entry.material {
				constants.base_color_factor = material.base_color_factor;
			}
			objects
				.try_reserve(instance.model().parts().len())
				.map_err(|error| format!("Cannot reserve scene render objects: {error}"))?;
			for part in instance.model().parts() {
				stats.draw_calls += 1;
				stats.triangles += part.mesh().index_count() as usize / 3;
				objects.push(RenderObject::new(part.mesh(), part.material(), constants));
			}
		}

		Ok((objects, stats))
	}

	pub fn to_data(&self, camera: &Camera) -> Result<SceneData, String> {
		let mut entities = Vec::with_capacity(self.entries.len());

		for (index, entry) in self.entries.iter().enumerate() {
			let model_path = entry.model_path.as_ref().ok_or_else(|| {
				format!(
					"entity {index} ({:?}): cannot save without a source model path",
					entry.name
				)
			})?;
			let transform = entry.instance.transform();

			let key = entry.key.as_ref().ok_or_else(|| {
				format!(
					"entity {index} ({:?}): cannot save without a persistent key",
					entry.name
				)
			})?;

			entities.push(EntityData {
				key: key.clone(),
				name: entry.name.clone(),
				model_path: model_path.clone(),
				transform: TransformData {
					position: transform.position,
					rotation: transform.rotation,
					scale: transform.scale,
				},
				parent: entry.parent.clone(),
				material: entry.material.clone(),
				collider: entry.collider.clone(),
				mesh_index: entry.mesh_index,
				renderable: entry.renderable,
			});
		}

		let data = SceneData {
			schema_version: CURRENT_SCHEMA_VERSION,
			bindings: self.bindings.clone(),
			entities,
			camera: CameraData {
				position: camera.position,
				yaw: camera.yaw,
				pitch: camera.pitch,
				vertical_fov: camera.vertical_fov,
				near_plane: camera.near,
				far_plane: camera.far,
			},
			lights: self.lights.clone(),
		};
		data.validate()?;
		Ok(data)
	}

	pub fn get(&self, id: EntityId) -> Option<&ModelInstance> {
		self.entries
			.get(*self.id_indices.get(&id)?)
			.map(|entry| &entry.instance)
	}

	pub fn contains(&self, id: EntityId) -> bool {
		self.id_indices.contains_key(&id)
	}

	pub fn entity_ids(&self) -> impl ExactSizeIterator<Item = EntityId> + DoubleEndedIterator + '_ {
		self.entries.iter().map(|entry| entry.id)
	}

	pub fn model_path(&self, id: EntityId) -> Option<&str> {
		self.entries
			.get(*self.id_indices.get(&id)?)?
			.model_path
			.as_deref()
	}

	pub fn spawn_persistent_model(
		&mut self,
		name: impl Into<String>,
		model_path: impl Into<String>,
		instance: ModelInstance,
	) -> Result<EntityId, String> {
		instance.transform().validate()?;
		let key = loop {
			let key = EntityKey::generate();
			if self.entity_by_key(&key).is_none() {
				break key;
			}
		};
		self.spawn_keyed_model(key, name, model_path, instance)
	}

	pub fn duplicate(&mut self, source: EntityId, offset: [f32; 3]) -> Result<EntityId, String> {
		let mut instance = self
			.get(source)
			.cloned()
			.ok_or_else(|| format!("Cannot duplicate {source:?}: entity is missing"))?;

		let source_name = self
			.name(source)
			.ok_or_else(|| format!("Cannot duplicate {source:?}: entity is missing"))?;
		let name = format!("{source_name} Copy");

		let model_path = self
			.model_path(source)
			.ok_or_else(|| format!("Cannot duplicate {source:?}: source has no model path"))?
			.to_owned();

		{
			let transform = instance.transform_mut();

			for (position, amount) in transform.position.iter_mut().zip(offset) {
				*position += amount;
			}

			transform.validate()?;
		}

		let source_metadata = self
			.entries
			.get(
				*self
					.id_indices
					.get(&source)
					.ok_or("Source entity is missing")?,
			)
			.cloned()
			.ok_or("Source entity is missing")?;
		let id = self.spawn_persistent_model(name, model_path, instance)?;
		let entry = self
			.entries
			.get_mut(*self.id_indices.get(&id).ok_or("New entity is missing")?)
			.ok_or("New entity is missing")?;
		entry.parent = source_metadata.parent;
		entry.material = source_metadata.material;
		entry.collider = source_metadata.collider;
		entry.mesh_index = source_metadata.mesh_index;
		entry.renderable = source_metadata.renderable;
		Ok(id)
	}

	/// Install already-validated document metadata while assembling a scene.
	pub fn set_entity_metadata(&mut self, id: EntityId, data: &EntityData) -> Result<(), String> {
		let entry = self
			.entries
			.get_mut(*self.id_indices.get(&id).ok_or("Entity is missing")?)
			.ok_or("Entity is missing")?;
		if entry.key.as_ref() != Some(&data.key) {
			return Err("Entity metadata key does not match its persistent identity".into());
		}
		entry.parent = data.parent.clone();
		entry.material = data.material.clone();
		entry.collider = data.collider.clone();
		entry.mesh_index = data.mesh_index;
		entry.renderable = data.renderable;
		self.authored_children = None;
		*self.world_cache.get_mut() = None;
		Ok(())
	}

	/// Update CPU draw state without retiring GPU models when topology and model
	/// sources are unchanged. Returns false when the GPU factory must rebuild.
	pub fn synchronize(&mut self, data: &SceneData) -> Result<bool, String> {
		let world = data.validated_world_matrices()?;
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
					entry.key.as_ref() == Some(&entity.key)
						&& entry.model_path.as_deref() == Some(entity.model_path.as_str())
						&& entry.mesh_index == entity.mesh_index
						&& entry.renderable == entity.renderable
				}) {
			return false;
		}
		for (entry, entity) in self.entries.iter_mut().zip(&data.entities) {
			let transform = entry.instance.transform_mut();
			transform.position = entity.transform.position;
			transform.rotation = entity.transform.rotation;
			transform.scale = entity.transform.scale;
			entry.name.clone_from(&entity.name);
			entry.parent.clone_from(&entity.parent);
			entry.material.clone_from(&entity.material);
			entry.collider.clone_from(&entity.collider);
		}
		self.authored_children = None;
		self.bindings.clone_from(&data.bindings);
		self.lights.clone_from(&data.lights);
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
				|| entry.model_path.as_deref() != Some(entity.model_path.as_str())
				|| entry.mesh_index != entity.mesh_index
				|| entry.renderable != entity.renderable
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
		for index in changed {
			let entity = &data.entities[index];
			let entry = &mut self.entries[index];
			*entry.instance.transform_mut() = crate::Transform {
				position: entity.transform.position,
				rotation: entity.transform.rotation,
				scale: entity.transform.scale,
			};
			entry.name.clone_from(&entity.name);
			entry.material.clone_from(&entity.material);
			entry.collider.clone_from(&entity.collider);
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
		if changes.lights_changed {
			self.lights.clone_from(&data.lights);
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
				return Ok(false);
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
			if entry.model_path.as_deref() != Some(entity.model_path.as_str())
				|| entry.mesh_index != entity.mesh_index
				|| entry.renderable != entity.renderable
			{
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
			let transform = entry.instance.transform_mut();
			transform.position = entity.transform.position;
			transform.rotation = entity.transform.rotation;
			transform.scale = entity.transform.scale;
			entry.name.clone_from(&entity.name);
			entry.parent.clone_from(&entity.parent);
			entry.material.clone_from(&entity.material);
			entry.collider.clone_from(&entity.collider);
			self.world_cache
				.get_mut()
				.as_mut()
				.expect("world cache initialized")[gpu_index] = world.world_matrices()[cpu_index];
		}
		if changes.lights_changed {
			self.lights.clone_from(&world.data().lights);
		}
		Ok(true)
	}

	pub fn asset_ref(&self, id: EntityId) -> Option<engine_core::asset_registry::AssetRef> {
		let entry = self.entries.get(*self.id_indices.get(&id)?)?;
		Some(engine_core::asset_registry::AssetRef {
			path: entry.model_path.clone()?,
			mesh_index: entry.mesh_index,
		})
	}

	pub(crate) fn subtree_render_object_count(&self, id: EntityId) -> Result<usize, String> {
		let root = *self
			.id_indices
			.get(&id)
			.ok_or("Duplicate source entity is missing")?;
		let mut children = vec![Vec::new(); self.entries.len()];
		for (index, entry) in self.entries.iter().enumerate() {
			if let Some(parent) = entry.parent.as_ref() {
				let parent = self
					.key_ids
					.get(parent)
					.and_then(|id| self.id_indices.get(id))
					.copied()
					.ok_or("Duplicate source parent is missing")?;
				children[parent].push(index);
			}
		}
		let mut pending = vec![root];
		let mut visited = vec![false; self.entries.len()];
		let mut count = 0usize;
		while let Some(index) = pending.pop() {
			if visited[index] {
				return Err("Duplicate source hierarchy contains a cycle".into());
			}
			visited[index] = true;
			count = count
				.checked_add(self.entries[index].instance.model().parts().len())
				.ok_or("Render-object count overflow")?;
			pending.extend_from_slice(&children[index]);
		}
		Ok(count)
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
		if self.synchronize_entries(data) {
			*self.world_cache.get_mut() = Some(world);
			return Ok(());
		}
		let mut next = Scene {
			next_id: self.next_id,
			bindings: data.bindings.clone(),
			lights: data.lights.clone(),
			..Scene::default()
		};
		let mut object_count = 0usize;
		for entity in &data.entities {
			let existing = self
				.entity_by_key(&entity.key)
				.and_then(|id| self.id_indices.get(&id))
				.map(|&index| &self.entries[index])
				.or_else(|| retained.get(&entity.key).map(|retained| &retained.0));
			let mut entry = if let Some(entry) = existing {
				if entry.model_path.as_deref() != Some(entity.model_path.as_str())
					|| entry.mesh_index != entity.mesh_index
					|| entry.renderable != entity.renderable
				{
					return Err("Native resource adapter needs a model factory to change an existing asset source".into());
				}
				entry.clone()
			} else {
				let template = retained.values().map(|retained| &retained.0).find(|entry| {
					entry.model_path.as_deref() == Some(entity.model_path.as_str())
						&& entry.mesh_index == entity.mesh_index
						&& entry.renderable == entity.renderable
				});
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
					instance,
					parent: entity.parent.clone(),
					material: entity.material.clone(),
					collider: entity.collider.clone(),
					mesh_index: entity.mesh_index,
					renderable: entity.renderable,
				}
			};
			let transform = entry.instance.transform_mut();
			transform.position = entity.transform.position;
			transform.rotation = entity.transform.rotation;
			transform.scale = entity.transform.scale;
			entry.name.clone_from(&entity.name);
			entry.parent.clone_from(&entity.parent);
			entry.material.clone_from(&entity.material);
			entry.collider.clone_from(&entity.collider);
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

	pub fn lights(&self) -> &[LightData] {
		&self.lights
	}
	pub fn set_lights(&mut self, lights: Vec<LightData>) {
		self.lights = lights;
	}

	pub fn world_matrices(&self) -> Result<Vec<glam::Mat4>, String> {
		Ok(self.cached_world_matrices()?.clone())
	}

	fn cached_world_matrices(&self) -> Result<Ref<'_, Vec<glam::Mat4>>, String> {
		if self.world_cache.borrow().is_none() {
			let local: Vec<_> = self
				.entries
				.iter()
				.map(|entry| entry.instance.transform().matrix())
				.collect();
			let keys: Vec<_> = self
				.entries
				.iter()
				.map(|entry| entry.key.as_ref())
				.collect();
			let parents: Vec<_> = self
				.entries
				.iter()
				.map(|entry| entry.parent.as_ref())
				.collect();
			*self.world_cache.borrow_mut() = Some(resolve_world_matrices(&local, &keys, &parents)?);
		}
		Ok(Ref::map(self.world_cache.borrow(), |cache| {
			cache.as_ref().expect("World cache was initialized")
		}))
	}

	pub fn world_matrix(&self, id: EntityId) -> Result<glam::Mat4, String> {
		let index = *self.id_indices.get(&id).ok_or("Entity is missing")?;
		Ok(self.cached_world_matrices()?[index])
	}

	pub fn world_bounds(&self, id: EntityId) -> Result<Option<crate::Bounds3>, String> {
		let instance = self.get(id).ok_or("Entity is missing")?;
		instance
			.model()
			.bounds()
			.map(|bounds| bounds.transformed(self.world_matrix(id)?))
			.transpose()
	}

	pub fn parent(&self, id: EntityId) -> Option<&EntityKey> {
		self.entries
			.get(*self.id_indices.get(&id)?)?
			.parent
			.as_ref()
	}

	pub fn collider(&self, id: EntityId) -> Option<&ColliderData> {
		self.entries
			.get(*self.id_indices.get(&id)?)?
			.collider
			.as_ref()
	}

	pub fn renderer_lights(&self) -> Vec<renderer::vulkan::Light> {
		self.lights
			.iter()
			.map(|light| match light.kind {
				LightKind::Directional => renderer::vulkan::Light::Directional {
					direction: light.direction,
					color: light.color,
					intensity: light.intensity,
				},
				LightKind::Point => renderer::vulkan::Light::Point {
					position: light.position,
					color: light.color,
					intensity: light.intensity,
					range: light.range,
				},
			})
			.collect()
	}
}

#[cfg(test)]
#[path = "tests_scene.rs"]
mod tests;
