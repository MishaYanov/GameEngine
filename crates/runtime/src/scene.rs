use std::{
	cell::{Ref, RefCell},
	collections::{BTreeMap, HashMap, HashSet},
};

use crate::entity_key::EntityKey;
use crate::model_instance::ModelInstance;
use crate::scene_data::{
	CURRENT_SCHEMA_VERSION, CameraData, ColliderData, EntityData, LightData, MaterialOverrideData,
	TransformData, resolve_world_matrices,
};
use crate::{Camera, SceneData};

mod limits;
mod render;
mod synchronization;

pub use limits::MAX_SCENE_RENDER_OBJECTS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EntityId(u64);

#[derive(Clone)]
struct SceneEntry {
	id: EntityId,
	key: Option<EntityKey>,
	name: String,
	model_path: Option<String>,
	asset_id: Option<engine_core::asset_catalog::AssetId>,
	collision_enabled: bool,
	instance: ModelInstance,
	parent: Option<EntityKey>,
	material: Option<MaterialOverrideData>,
	collider: Option<ColliderData>,
	mesh_index: Option<usize>,
	renderable: bool,
	component_ids: BTreeMap<String, engine_core::components::ComponentId>,
	native_behaviors: Vec<engine_core::components::NativeBehaviorComponent>,
	linked_prefab: Option<engine_core::linked_prefab::LinkedPrefabInstance>,
	animation: Option<engine_core::animation::AnimationComponent>,
	character_controller: Option<engine_core::character_controller::CharacterControllerComponent>,
}

impl SceneEntry {
	fn matches_asset_source(&self, entity: &EntityData) -> bool {
		self.model_path.as_deref() == Some(entity.model_path.as_str())
			&& self.asset_id == entity.asset_id
			&& self.mesh_index == entity.mesh_index
			&& self.animation == entity.animation
	}

	fn can_reuse_model(&self, entity: &EntityData) -> bool {
		self.matches_asset_source(entity)
			&& (!entity.renderable || self.renderable || !self.instance.model().parts().is_empty())
	}

	fn apply_document_values(&mut self, entity: &EntityData) {
		let transform = self.instance.transform_mut();
		transform.position = entity.transform.position;
		transform.rotation = entity.transform.rotation;
		transform.scale = entity.transform.scale;
		self.name.clone_from(&entity.name);
		self.apply_document_properties(entity);
	}

	fn apply_document_properties(&mut self, entity: &EntityData) {
		self.material.clone_from(&entity.material);
		self.collider.clone_from(&entity.collider);
		self.collision_enabled = entity.collision_enabled;
		self.renderable = entity.renderable;
		self.component_ids.clone_from(&entity.component_ids);
		self.native_behaviors.clone_from(&entity.native_behaviors);
		self.linked_prefab.clone_from(&entity.linked_prefab);
		self.animation.clone_from(&entity.animation);
		self.character_controller
			.clone_from(&entity.character_controller);
	}
}

#[derive(Clone)]
pub struct EntitySnapshot {
	entry: SceneEntry,
	index: usize,
	bindings: BTreeMap<String, EntityKey>,
	orbit_target_settings: Option<Box<engine_core::game_camera::OrbitCameraSettings>>,
}

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

/// Resource retention does not need the binding/history payload of a snapshot.
#[derive(Clone)]
pub(crate) struct RetainedEntity(SceneEntry);

#[derive(Clone, Default)]
pub struct Scene {
	entries: Vec<SceneEntry>,
	bindings: BTreeMap<String, EntityKey>,
	next_id: u64,
	lights: Vec<LightData>,
	camera_follow: Option<engine_core::game_camera::FollowCameraSettings>,
	camera_orbit: Option<engine_core::game_camera::OrbitCameraSettings>,
	id_indices: HashMap<EntityId, usize>,
	key_ids: HashMap<EntityKey, EntityId>,
	world_cache: RefCell<Option<Vec<glam::Mat4>>>,
	authored_children: Option<Vec<Vec<usize>>>,
	animation_root: Option<std::path::PathBuf>,
}

impl Scene {
	pub fn set_animation_root(&mut self, root: std::path::PathBuf) {
		self.animation_root = Some(root);
	}
	pub fn animation_root(&self) -> Option<&std::path::Path> {
		self.animation_root.as_deref()
	}
	pub fn restore_authored_animation(&mut self) {
		for entry in &mut self.entries {
			entry.instance.restore_authored_animation();
		}
	}
	pub fn reset_animation_overrides(&mut self) {
		for entry in &mut self.entries {
			entry.instance.reset_animation_override();
		}
	}
	pub fn set_animation_pose(
		&mut self,
		key: &EntityKey,
		pose: Option<&engine_core::animation::AnimationPose>,
	) -> Result<(), String> {
		let id = self
			.entity_by_key(key)
			.ok_or("Animation preview entity no longer exists")?;
		let index = self.id_indices[&id];
		match pose {
			Some(pose) => self.entries[index].instance.set_animation_pose(pose, false),
			None => {
				self.entries[index].instance.reset_animation_override();
				Ok(())
			}
		}
	}
	pub fn synchronize_animation(
		&mut self,
		bank: &engine_core::animation::AnimationBank,
	) -> Result<(), String> {
		for (key, pose) in bank.poses() {
			let id = self
				.entity_by_key(key)
				.ok_or("Animation entity is missing from native scene")?;
			let index = self.id_indices[&id];
			self.entries[index]
				.instance
				.set_animation_pose(pose, true)?;
		}
		Ok(())
	}

	pub fn camera_follow(&self) -> Option<&engine_core::game_camera::FollowCameraSettings> {
		self.camera_follow.as_ref()
	}
	pub fn set_camera_follow(
		&mut self,
		follow: Option<engine_core::game_camera::FollowCameraSettings>,
	) {
		if follow.is_some() {
			self.camera_orbit = None;
		}
		self.camera_follow = follow;
	}
	pub fn camera_orbit(&self) -> Option<&engine_core::game_camera::OrbitCameraSettings> {
		self.camera_orbit.as_ref()
	}
	pub fn set_camera_orbit(
		&mut self,
		orbit: Option<engine_core::game_camera::OrbitCameraSettings>,
	) {
		if orbit.is_some() {
			self.camera_follow = None;
		}
		self.camera_orbit = orbit;
	}
	pub(crate) fn synchronize_camera_settings(&mut self, camera: &CameraData) {
		self.camera_follow.clone_from(&camera.follow);
		self.camera_orbit.clone_from(&camera.orbit);
	}
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
			&& self.camera_follow == data.camera.follow
			&& self.camera_orbit == data.camera.orbit
			&& self.entries.len() == data.entities.len()
			&& self
				.entries
				.iter()
				.zip(&data.entities)
				.all(|(entry, saved)| {
					let transform = entry.instance.transform();
					entry.key.as_ref() == Some(&saved.key)
						&& entry.name == saved.name
						&& entry.asset_id == saved.asset_id
						&& entry.collision_enabled == saved.collision_enabled
						&& entry.model_path.as_deref() == Some(saved.model_path.as_str())
						&& transform.position == saved.transform.position
						&& transform.rotation == saved.transform.rotation
						&& transform.scale == saved.transform.scale
						&& entry.parent == saved.parent
						&& entry.material == saved.material
						&& entry.collider == saved.collider
						&& entry.mesh_index == saved.mesh_index
						&& entry.renderable == saved.renderable
						&& entry.component_ids == saved.component_ids
						&& entry.native_behaviors == saved.native_behaviors
						&& entry.linked_prefab == saved.linked_prefab
						&& entry.animation == saved.animation
						&& entry.character_controller == saved.character_controller
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
			orbit_target_settings: self
				.camera_orbit
				.as_ref()
				.filter(|orbit| Some(&orbit.target) == entry.key.as_ref())
				.map(|orbit| Box::new(orbit.clone())),
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
		if snapshot.orbit_target_settings.is_some()
			&& (self.camera_orbit.is_some() || self.camera_follow.is_some())
		{
			return Err("Cannot restore orbit target: another camera rig is configured".into());
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
		if let Some(orbit) = snapshot.orbit_target_settings.as_deref() {
			self.camera_orbit = Some(orbit.clone());
		}
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
			asset_id: None,
			collision_enabled: true,
			instance,
			parent: None,
			material: None,
			collider: None,
			mesh_index: None,
			renderable: true,
			component_ids: BTreeMap::new(),
			native_behaviors: Vec::new(),
			linked_prefab: None,
			animation: None,
			character_controller: None,
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
			if self
				.camera_orbit
				.as_ref()
				.is_some_and(|orbit| &orbit.target == key)
			{
				self.camera_orbit = None;
			}
		}

		Some(entry.instance)
	}

	pub fn instances(&self) -> impl Iterator<Item = &ModelInstance> {
		self.entries.iter().map(|entry| &entry.instance)
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
				asset_id: entry.asset_id.clone(),
				collision_enabled: entry.collision_enabled,
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
				component_ids: entry.component_ids.clone(),
				native_behaviors: entry.native_behaviors.clone(),
				linked_prefab: entry.linked_prefab.clone(),
				animation: entry.animation.clone(),
				character_controller: entry.character_controller.clone(),
			});
		}

		let data = SceneData {
			schema_version: CURRENT_SCHEMA_VERSION,
			bindings: self.bindings.clone(),
			entities,
			camera: CameraData {
				follow: self.camera_follow.clone(),
				orbit: self.camera_orbit.clone(),
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

	/// Editor picking must use the same authored visibility as scene submission.
	pub fn is_renderable(&self, id: EntityId) -> bool {
		self.id_indices
			.get(&id)
			.is_some_and(|&index| self.entries[index].renderable)
	}

	/// Prepared native model resources available to runtime visibility actions.
	/// Missing invisible authoring markers are deliberately excluded.
	pub fn visibility_capabilities(&self) -> HashSet<EntityKey> {
		self.entries
			.iter()
			.filter(|entry| !entry.instance.model().parts().is_empty())
			.filter_map(|entry| entry.key.clone())
			.collect()
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
		if source_metadata.linked_prefab.is_some()
			|| source_metadata.key.as_ref().is_some_and(|key| {
				self.entries
					.iter()
					.filter_map(|entry| entry.linked_prefab.as_ref())
					.any(|link| link.entity_map.values().any(|member| member == key))
			}) {
			return Err("Duplicate linked assemblies through EditorSession or Unpack first".into());
		}
		let id = self.spawn_persistent_model(name, model_path, instance)?;
		let entry = self
			.entries
			.get_mut(*self.id_indices.get(&id).ok_or("New entity is missing")?)
			.ok_or("New entity is missing")?;
		entry.asset_id = source_metadata.asset_id;
		entry.collision_enabled = source_metadata.collision_enabled;
		entry.parent = source_metadata.parent;
		entry.material = source_metadata.material;
		entry.collider = source_metadata.collider;
		entry.mesh_index = source_metadata.mesh_index;
		entry.renderable = source_metadata.renderable;
		entry.component_ids = source_metadata.component_ids;
		entry.native_behaviors = source_metadata.native_behaviors;
		if let (Some(old), Some(new)) = (source_metadata.key, entry.key.clone()) {
			let remap = HashMap::from([(old, new)]);
			for component in &mut entry.native_behaviors {
				component.behavior.remap_entity_references(&remap);
			}
		}
		entry.linked_prefab = source_metadata.linked_prefab;
		entry.animation = source_metadata.animation;
		entry.character_controller = source_metadata.character_controller;
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
		entry.asset_id.clone_from(&data.asset_id);
		entry.parent.clone_from(&data.parent);
		entry.mesh_index = data.mesh_index;
		entry.apply_document_properties(data);
		self.authored_children = None;
		*self.world_cache.get_mut() = None;
		Ok(())
	}

	pub fn asset_ref(&self, id: EntityId) -> Option<engine_core::asset_registry::AssetRef> {
		let entry = self.entries.get(*self.id_indices.get(&id)?)?;
		Some(engine_core::asset_registry::AssetRef {
			path: entry.model_path.clone()?,
			asset_id: entry.asset_id.clone(),
			mesh_index: entry.mesh_index,
		})
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
			.animation_bounds()
			.or_else(|| instance.model().bounds())
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
}

pub(crate) fn validate_animation_placements(
	data: &SceneData,
	matrices: &[glam::Mat4],
) -> Result<(), String> {
	for (entity, matrix) in data.entities.iter().zip(matrices) {
		if entity.animation.is_some() {
			validate_animation_placement(&entity.name, *matrix)?;
		}
	}
	Ok(())
}
fn validate_animation_placement(name: &str, matrix: glam::Mat4) -> Result<(), String> {
	engine_core::animation::checked_similarity_inverse(matrix)
		.map(|_| ())
		.map_err(|error| {
			format!(
				"Animated entity {name:?} needs positive uniform world scale and no reflection/shear: {error}"
			)
		})
}

#[cfg(test)]
#[path = "../tests/unit/scene.rs"]
mod tests;
