use super::*;
use crate::Transform;
use crate::assets::Model;
use std::sync::Arc;

fn instance(model: &Arc<Model>) -> ModelInstance {
    ModelInstance::new(Arc::clone(model), Transform::default())
}

#[test]
fn removal_does_not_reuse_ids_or_change_other_entities() {
    let model = Arc::new(Model::new(Vec::new()));
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
    let model = Arc::new(Model::new(Vec::new()));
    let mut first = instance(&model);
    let second = instance(&model);
    first.transform_mut().scale = [2.0; 3];
    assert_eq!(second.transform().scale, [1.0; 3]);
    assert!(std::ptr::eq(first.model(), second.model()));
}

#[test]
fn snapshot_preserves_survivor_order_and_new_model_metadata_after_removal() {
    let model = Arc::new(Model::new(Vec::new()));
    let mut scene = Scene::default();
    let removed = scene.spawn_model("Removed", "props/removed.glb", instance(&model));
    let survivor = scene.spawn_model("Robot label", "props/robot.glb", instance(&model));
    scene.remove(removed).unwrap();
    let added = scene.spawn_model("Robot label", "props/alternate.glb", instance(&model));
    scene.get_mut(survivor).unwrap().transform_mut().position = [2.0, 3.0, 4.0];
    scene.get_mut(added).unwrap().transform_mut().scale = [-1.0, 0.0, 2.0];

    let data = scene.to_data(&Camera::default()).unwrap();
    assert_eq!(data.entities.len(), 2);
    assert_eq!(data.entities[0].model_path, "props/robot.glb");
    assert_eq!(data.entities[0].transform.position, [2.0, 3.0, 4.0]);
    assert_eq!(data.entities[1].model_path, "props/alternate.glb");
    assert_eq!(data.entities[1].transform.scale, [-1.0, 0.0, 2.0]);
    assert_eq!(scene.name(removed), None);
    assert_ne!(removed, added);
}
