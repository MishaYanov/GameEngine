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
	let removed = scene
		.spawn_keyed_model(
			EntityKey::new("removed").unwrap(),
			"Removed",
			"props/removed.glb",
			instance(&model),
		)
		.unwrap();
	let survivor = scene
		.spawn_keyed_model(
			EntityKey::new("survivor").unwrap(),
			"Robot label",
			"props/robot.glb",
			instance(&model),
		)
		.unwrap();
    scene.remove(removed).unwrap();
	let added = scene
		.spawn_keyed_model(
			EntityKey::new("added").unwrap(),
			"Robot label",
			"props/alternate.glb",
			instance(&model),
		)
		.unwrap();
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

#[test]
fn duplicate_keys_are_rejected_and_removal_clears_all_bindings_to_that_key() {
	let model = Arc::new(Model::new(Vec::new()));
	let key = EntityKey::new("robot").unwrap();
	let mut scene = Scene::default();
	let id = scene
		.spawn_keyed_model(key.clone(), "Robot", "props/robot.glb", instance(&model))
		.unwrap();
	scene.set_binding("robot_a", id).unwrap();
	scene.set_binding("alias", id).unwrap();
	assert!(
		scene
			.spawn_keyed_model(
				key.clone(),
				"Duplicate",
				"props/robot.glb",
				instance(&model)
			)
			.is_err()
	);
	assert_eq!(scene.instances().count(), 1);
	assert_eq!(scene.entity_by_key(&key), Some(id));
	assert!(scene.set_binding(" \t", id).is_err());
	let pathless = scene.spawn(instance(&model));
	assert!(scene.set_binding("invalid", pathless).is_err());
	scene.remove(pathless).unwrap();
	scene.remove(id).unwrap();
	assert_eq!(scene.entity_by_key(&key), None);
	assert_eq!(scene.binding("robot_a"), None);
	assert_eq!(scene.binding("alias"), None);
	assert!(scene.set_binding("deleted", id).is_err());
	let replacement = scene
		.spawn_keyed_model(
			key.clone(),
			"Replacement",
			"props/robot.glb",
			instance(&model),
		)
		.unwrap();
	assert_ne!(replacement, id);
	assert_eq!(scene.entity_by_key(&key), Some(replacement));
	assert_eq!(scene.binding("alias"), None);
	let data = scene.to_data(&Camera::default()).unwrap();
	assert!(data.bindings.is_empty());
}

#[test]
fn duplicate_shares_model_but_has_independent_transform_and_a_new_persistent_key() {
	let model = Arc::new(Model::new(Vec::new()));
	let mut scene = Scene::default();
	let original = scene
		.spawn_persistent_model("Robot", "props/robot.glb", instance(&model))
		.unwrap();
	scene.get_mut(original).unwrap().transform_mut().rotation = [0.1, 0.2, 0.3];
	scene.set_binding("robot_a", original).unwrap();
	let duplicate = scene.duplicate(original, [0.4, -0.2, 0.0]).unwrap();
	assert_ne!(scene.key(original), scene.key(duplicate));
	assert_eq!(scene.name(duplicate), Some("Robot Copy"));
	assert_eq!(scene.model_path(duplicate), Some("props/robot.glb"));
	assert!(std::ptr::eq(
		scene.get(original).unwrap().model(),
		scene.get(duplicate).unwrap().model()
	));
	assert_eq!(
		scene.get(duplicate).unwrap().transform().position,
		[0.4, -0.2, 0.0]
	);
	assert_eq!(
		scene.get(duplicate).unwrap().transform().rotation,
		[0.1, 0.2, 0.3]
	);
	scene.get_mut(duplicate).unwrap().transform_mut().scale = [2.0; 3];
	assert_eq!(scene.get(original).unwrap().transform().scale, [1.0; 3]);
	assert_eq!(scene.binding("robot_a"), Some(original));
	let saved = scene.to_data(&Camera::default()).unwrap();
	assert_eq!(saved.entities.len(), 2);
	assert_eq!(saved.entities[1].key, *scene.key(duplicate).unwrap());
}

#[test]
fn failed_spawn_or_duplicate_does_not_insert_a_partial_entity() {
	let model = Arc::new(Model::new(Vec::new()));
	let mut scene = Scene::default();
	let original = scene.spawn_model("Robot", "props/robot.glb", instance(&model));
	let pathless = scene.spawn(instance(&model));
	for offset in [[f32::NAN, 0.0, 0.0], [0.0, f32::INFINITY, 0.0]] {
		assert!(scene.duplicate(original, offset).is_err());
	}
	assert!(scene.duplicate(pathless, [0.0; 3]).is_err());
	scene.remove(pathless).unwrap();
	assert!(scene.duplicate(pathless, [0.0; 3]).is_err());
	let mut invalid = instance(&model);
	invalid.transform_mut().scale[0] = f32::NAN;
	assert!(
		scene
			.spawn_persistent_model("Invalid", "props/robot.glb", invalid)
			.is_err()
	);
	assert_eq!(scene.entity_ids().collect::<Vec<_>>(), vec![original]);
	assert_eq!(scene.get(original).unwrap().transform().position, [0.0; 3]);
}
