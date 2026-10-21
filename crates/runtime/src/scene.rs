use crate::model_instance::ModelInstance;
use crate::scene_data::{CameraData, EntityData, TransformData};
use crate::{Camera, SceneData};
use renderer::vulkan::{ModelPushConstants, RenderObject};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntityId(u64);

struct SceneEntry {
    id: EntityId,
    name: String,
    model_path: Option<String>,
    instance: ModelInstance,
}

#[derive(Default)]
pub struct Scene {
    entries: Vec<SceneEntry>,
    next_id: u64,
}

impl Scene {
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
        self.spawn_entry(name.into(), None, instance)
    }

    pub fn spawn_model(
        &mut self,
        name: impl Into<String>,
        model_path: impl Into<String>,
        instance: ModelInstance,
    ) -> EntityId {
        self.spawn_entry(name.into(), Some(model_path.into()), instance)
    }

    fn spawn_entry(
        &mut self,
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

        Some(entry.instance)
    }

    pub fn instances(&self) -> impl Iterator<Item = &ModelInstance> {
        self.entries.iter().map(|entry| &entry.instance)
    }

    pub fn render_objects(&self) -> Vec<RenderObject<'_>> {
        let mut objects = Vec::new();

        for instance in self.instances() {
            let transform = instance.transform();
            let constants = ModelPushConstants::from_3d(
                transform.position,
                transform.rotation,
                transform.scale,
            );
            for part in instance.model().parts() {
                objects.push(RenderObject::new(part.mesh(), part.material(), constants));
            }
        }

        objects
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

            entities.push(EntityData {
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
            schema_version: 1,
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
}

#[cfg(test)]
#[path = "tests_scene.rs"]
mod tests;
