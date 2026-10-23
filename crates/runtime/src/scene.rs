use std::collections::BTreeMap;

use crate::entity_key::EntityKey;
use crate::model_instance::ModelInstance;
use crate::scene_data::{CURRENT_SCHEMA_VERSION, CameraData, EntityData, TransformData};
use crate::{Camera, SceneData};
use renderer::vulkan::{ModelPushConstants, RenderObject};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntityId(u64);

#[derive(Clone)]
struct SceneEntry {
    id: EntityId,
    key: Option<EntityKey>,
    name: String,
    model_path: Option<String>,
    instance: ModelInstance,
}

#[derive(Clone, Default)]
pub struct Scene {
    entries: Vec<SceneEntry>,
    bindings: BTreeMap<String, EntityKey>,
    next_id: u64,
}

#[derive(Clone)]
pub struct EntitySnapshot {
    entry: SceneEntry,
    index: usize,
    bindings: BTreeMap<String, EntityKey>,
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

impl Scene {
    pub fn rename(&mut self, id: EntityId, name: impl Into<String>) -> Result<(), String> {
        let name = name.into();
        if name.trim().is_empty() || name.chars().any(char::is_control) {
            return Err("Entity name must contain visible text and no control characters".into());
        }
        self.entries
            .iter_mut()
            .find(|entry| entry.id == id)
            .ok_or("Cannot rename a missing entity")?
            .name = name;
        Ok(())
    }

    /// Compare authored content without allocating a second serialization each frame.
    pub fn matches_data(&self, data: &SceneData, camera: &Camera) -> bool {
        let saved_camera = &data.camera;
        self.bindings == data.bindings
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
            })
            && camera.position == saved_camera.position
            && camera.yaw == saved_camera.yaw
            && camera.pitch == saved_camera.pitch
            && camera.vertical_fov == saved_camera.vertical_fov
            && camera.near == saved_camera.near_plane
            && camera.far == saved_camera.far_plane
    }
    pub fn snapshot(&self, id: EntityId) -> Result<EntitySnapshot, String> {
        let index = self
            .entries
            .iter()
            .position(|entry| entry.id == id)
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
        self.entries
            .insert(snapshot.index.min(self.entries.len()), entry.clone());
        self.bindings.extend(snapshot.bindings.clone());
        self.next_id = self.next_id.max(next_id);
        Ok(entry.id)
    }

    pub fn spawn(&mut self, instance: ModelInstance) -> EntityId {
        self.spawn_named("entity", instance)
    }

    pub fn name(&self, id: EntityId) -> Option<&str> {
        let name = self
            .entries
            .iter()
            .find(|entry| entry.id == id)?
            .name
            .as_str();
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
        self.entries
            .iter()
            .find(|entry| entry.id == id)?
            .key
            .as_ref()
    }

    pub fn entity_by_key(&self, key: &EntityKey) -> Option<EntityId> {
        self.entries
            .iter()
            .find(|entry| entry.key.as_ref() == Some(key))
            .map(|entry| entry.id)
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

        self.entries.push(SceneEntry {
            id,
            key,
            name,
            model_path,
            instance,
        });

        id
    }

    pub fn get_mut(&mut self, id: EntityId) -> Option<&mut ModelInstance> {
        self.entries
            .iter_mut()
            .find(|entry| entry.id == id)
            .map(|entry| &mut entry.instance)
    }

    pub fn remove(&mut self, id: EntityId) -> Option<ModelInstance> {
        let index = self.entries.iter().position(|entry| entry.id == id)?;
        let entry = self.entries.remove(index);
        if let Some(key) = entry.key.as_ref() {
            self.bindings.retain(|_, target| target != key);
        }

        Some(entry.instance)
    }

    pub fn instances(&self) -> impl Iterator<Item = &ModelInstance> {
        self.entries.iter().map(|entry| &entry.instance)
    }

    pub fn render_objects(&self) -> Vec<RenderObject<'_>> {
        self.render_visible(None).0
    }

    pub fn render_visible(
        &self,
        frustum: Option<&crate::visibility::Frustum>,
    ) -> (Vec<RenderObject<'_>>, crate::visibility::SceneRenderStats) {
        let mut objects = Vec::new();
        let mut stats = crate::visibility::SceneRenderStats::default();

        for instance in self.instances() {
            stats.total_entities += 1;
            let outside = frustum.is_some_and(|frustum| {
                instance
                    .model()
                    .bounds()
                    .and_then(|bounds| bounds.transformed(instance.transform().matrix()).ok())
                    .is_some_and(|bounds| !frustum.intersects(bounds))
            });
            if outside {
                stats.culled_entities += 1;
                continue;
            }
            stats.visible_entities += 1;
            let transform = instance.transform();
            let constants = ModelPushConstants::from_3d(
                transform.position,
                transform.rotation,
                transform.scale,
            );
            for part in instance.model().parts() {
                stats.draw_calls += 1;
                stats.triangles += part.mesh().index_count() as usize / 3;
                objects.push(RenderObject::new(part.mesh(), part.material(), constants));
            }
        }

        (objects, stats)
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
        };
        data.validate()?;
        Ok(data)
    }

    pub fn get(&self, id: EntityId) -> Option<&ModelInstance> {
        self.entries
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| &entry.instance)
    }

    pub fn contains(&self, id: EntityId) -> bool {
        self.entries.iter().any(|entry| entry.id == id)
    }

    pub fn entity_ids(&self) -> impl Iterator<Item=EntityId> + '_ {
        self.entries.iter().map(|entry| entry.id)
    }

    pub fn model_path(&self, id: EntityId) -> Option<&str> {
        self.entries
            .iter()
            .find(|entry| entry.id == id)?
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

        self.spawn_persistent_model(name, model_path, instance)
    }
}

#[cfg(test)]
#[path = "tests_scene.rs"]
mod tests;
