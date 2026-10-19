use crate::model_instance::ModelInstance;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntityId(u64);

struct SceneEntry {
    id: EntityId,
    instance: ModelInstance,
}

#[derive(Default)]
pub struct Scene {
    entries: Vec<SceneEntry>,
    next_id: u64,
}

impl Scene {
    pub fn spawn(&mut self, instance: ModelInstance) -> EntityId {
        let id = EntityId(self.next_id);

        self.next_id = self
            .next_id
            .checked_add(1)
            .expect("scene entity IDs exhausted");

        self.entries.push(SceneEntry { id, instance });

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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{assets::Model, transform::Transform};
    use std::sync::Arc;

    fn instance(model: &Arc<Model>) -> ModelInstance {
        ModelInstance::new(Arc::clone(model), Transform::default())
    }

    #[test]
    fn removal_does_not_reuse_ids_or_change_other_entities() {
        let model = Arc::new(Model::empty_for_test());
        let mut scene = Scene::default();
        let first = scene.spawn(instance(&model));
        let second = scene.spawn(instance(&model));
        scene.get_mut(second).unwrap().transform_mut().position = [1.0, 2.0, 3.0];
        assert!(scene.remove(first).is_some());
        assert!(scene.remove(first).is_none());
        assert!(scene.get_mut(first).is_none());
        let third = scene.spawn(instance(&model));
        assert_ne!(first, third);
        assert_ne!(second, third);
        let positions: Vec<_> = scene.instances().map(|i| i.transform().position).collect();
        assert_eq!(positions, vec![[1.0, 2.0, 3.0], [0.0; 3]]);
    }

    #[test]
    fn shared_model_instances_have_independent_transforms() {
        let model = Arc::new(Model::empty_for_test());
        let mut first = instance(&model);
        let second = instance(&model);
        first.transform_mut().scale = [2.0; 3];
        assert_eq!(second.transform().scale, [1.0; 3]);
        assert!(std::ptr::eq(first.model(), second.model()));
    }
}
