//! Exercise the runtime through the same public API used by applications.
use std::{path::Path, sync::Arc};

use runtime::{
    Transform,
    assets::{MaterialData, Model, gltf_loader::load_first_mesh},
    model_instance::ModelInstance,
    scene::Scene,
};

#[test]
fn empty_scenes_and_models_produce_no_render_objects() {
    let mut scene = Scene::default();
    assert!(scene.render_objects().is_empty());
    let id = scene.spawn(ModelInstance::new(
        Arc::new(Model::new(Vec::new())),
        Transform::default(),
    ));
    assert_eq!(scene.instances().count(), 1);
    assert!(scene.render_objects().is_empty());
    assert!(scene.remove(id).is_some());
    assert!(scene.render_objects().is_empty());
}

#[test]
fn scene_removal_releases_only_its_own_shared_model_reference() {
    let model = Arc::new(Model::new(Vec::new()));
    let mut scene = Scene::default();
    let first = scene.spawn(ModelInstance::new(Arc::clone(&model), Transform::default()));
    let second = scene.spawn(ModelInstance::new(Arc::clone(&model), Transform::default()));
    assert_eq!(Arc::strong_count(&model), 3);
    let removed = scene.remove(first).unwrap();
    assert_eq!(Arc::strong_count(&model), 3);
    drop(removed);
    assert_eq!(Arc::strong_count(&model), 2);
    assert!(scene.get_mut(first).is_none());
    assert!(scene.get_mut(second).is_some());
    drop(scene);
    assert_eq!(Arc::strong_count(&model), 1);
}

#[test]
fn imported_material_data_is_available_to_runtime_consumers() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/game/assets/props/robot.glb");
    let parts = load_first_mesh(path).expect("bundled model remains importable after the move");
    assert!(!parts.is_empty());
    for part in parts {
        let MaterialData {
            base_color_image,
            base_color_factor,
            double_sided,
        } = part.material;
        assert!(base_color_factor.iter().all(|value| value.is_finite()));
        if let Some(image) = base_color_image {
            assert!(image.width() > 0 && image.height() > 0);
            assert_eq!(
                image.pixels().len(),
                image.width() as usize * image.height() as usize * 4
            );
        }
        // External applications can also construct materials with the imported values.
        let _material = MaterialData {
            base_color_image: None,
            base_color_factor,
            double_sided,
        };
        assert!(!part.mesh.vertices().is_empty());
        assert!(!part.mesh.indices().is_empty());
    }
}
