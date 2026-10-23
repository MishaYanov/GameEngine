use runtime::{
	Camera, EntityKey, Transform,
	assets::Model,
	commands::{CommandHistory, SetTransform},
	model_instance::ModelInstance,
	scene::{EntityId, Scene},
};
use std::sync::Arc;

fn model() -> Arc<Model> {
	Arc::new(Model::new(vec![]))
}
fn instance(model: &Arc<Model>) -> ModelInstance {
	ModelInstance::new(Arc::clone(model), Transform::default())
}
fn add(scene: &mut Scene, key: &str, model: &Arc<Model>) -> EntityId {
	scene
		.spawn_keyed_model(
			EntityKey::new(key).unwrap(),
			key,
			"props/robot.glb",
			instance(model),
		)
		.unwrap()
}
fn saved(scene: &Scene) -> String {
	scene
		.to_data(&Camera::default())
		.unwrap()
		.to_json()
		.unwrap()
}

#[test]
fn snapshot_restore_preserves_identity_order_bindings_transform_and_shared_resources() {
	let model = model();
	let mut scene = Scene::default();
	let first = add(&mut scene, "first", &model);
	let middle = add(&mut scene, "middle", &model);
	let last = add(&mut scene, "last", &model);
	scene.set_binding("robot_a", middle).unwrap();
	scene.set_binding("alias", middle).unwrap();
	scene.get_mut(middle).unwrap().transform_mut().scale = [-2.0, 0.0, 3.0];
	let before = saved(&scene);
	let snapshot = scene.snapshot(middle).unwrap();
	assert_eq!(snapshot.id(), middle);
	assert_eq!(snapshot.key().unwrap().as_str(), "middle");
	assert_eq!(snapshot.name(), "middle");
	scene.remove(middle).unwrap();
	assert_eq!(scene.binding("robot_a"), None);
	assert_eq!(scene.restore(&snapshot).unwrap(), middle);
	assert_eq!(
		scene.entity_ids().collect::<Vec<_>>(),
		vec![first, middle, last]
	);
	assert_eq!(saved(&scene), before);
	assert!(std::ptr::eq(
		scene.get(middle).unwrap().model(),
		snapshot.instance().model()
	));
	assert!(std::ptr::eq(
		scene.get(middle).unwrap().model(),
		model.as_ref()
	));
}

#[test]
fn restore_conflicts_leave_scene_unchanged_and_snapshot_can_be_retried() {
	let model = model();
	let mut scene = Scene::default();
	let original = add(&mut scene, "original", &model);
	let other = add(&mut scene, "other", &model);
	scene.set_binding("role", original).unwrap();
	let snapshot = scene.snapshot(original).unwrap();
	let before = saved(&scene);
	assert!(scene.restore(&snapshot).is_err());
	assert_eq!(saved(&scene), before);
	scene.remove(original).unwrap();
	let collision = add(&mut scene, "original", &model);
	let before = saved(&scene);
	assert!(scene.restore(&snapshot).is_err());
	assert_eq!(saved(&scene), before);
	scene.remove(collision).unwrap();
	scene.set_binding("role", other).unwrap();
	let before = saved(&scene);
	assert!(scene.restore(&snapshot).is_err());
	assert_eq!(saved(&scene), before);
	scene.remove(other).unwrap();
	scene.restore(&snapshot).unwrap();
	assert_eq!(scene.binding("role"), Some(original));
	let added = add(&mut scene, "after-restore", &model);
	assert_ne!(added, original);
	assert_ne!(added, other);
	assert_ne!(added, collision);
}

#[test]
fn cloning_scene_isolates_transforms_but_reuses_model_resources_and_bindings() {
	let model = model();
	let mut source = Scene::default();
	let id = add(&mut source, "robot", &model);
	source.set_binding("role", id).unwrap();
	let mut clone = source.clone();
	assert_eq!(saved(&source), saved(&clone));
	clone.get_mut(id).unwrap().transform_mut().position[0] = 2.0;
	assert_eq!(source.get(id).unwrap().transform().position[0], 0.0);
	assert!(std::ptr::eq(
		source.get(id).unwrap().model(),
		clone.get(id).unwrap().model()
	));
	clone.remove(id).unwrap();
	assert_eq!(source.binding("role"), Some(id));
	assert_eq!(clone.binding("role"), None);
}

#[test]
fn mixed_structural_and_transform_history_restores_exact_order_keys_and_bindings() {
	let model = model();
	let mut scene = Scene::default();
	let original = add(&mut scene, "original", &model);
	add(&mut scene, "last", &model);
	scene.set_binding("role", original).unwrap();
	let before = saved(&scene);
	let mut history = CommandHistory::default();
	let spawned = history
		.spawn(&mut scene, "spawned", "props/robot.glb", instance(&model))
		.unwrap();
	let spawn_key = scene.key(spawned).unwrap().clone();
	history
		.execute(
			&mut scene,
			SetTransform {
				entity: spawned,
				transform: Transform {
					position: [1.0, 2.0, 3.0],
					..Transform::default()
				},
			},
			true,
		)
		.unwrap();
	let duplicated = history
		.duplicate(&mut scene, spawned, [0.4, 0.0, 0.0])
		.unwrap();
	let duplicate_key = scene.key(duplicated).unwrap().clone();
	history.delete(&mut scene, original).unwrap();
	let after = saved(&scene);
	for (id, present) in [
		(original, true),
		(duplicated, false),
		(spawned, true),
		(spawned, false),
	] {
		let effect = history.undo_scene(&mut scene).unwrap().unwrap();
		assert_eq!((effect.entity, effect.present), (id, present));
	}
	assert_eq!(saved(&scene), before);
	assert!(history.undo_scene(&mut scene).unwrap().is_none());
	for (id, present) in [
		(spawned, true),
		(spawned, true),
		(duplicated, true),
		(original, false),
	] {
		let effect = history.redo_scene(&mut scene).unwrap().unwrap();
		assert_eq!((effect.entity, effect.present), (id, present));
	}
	assert_eq!(saved(&scene), after);
	assert_eq!(scene.key(spawned), Some(&spawn_key));
	assert_eq!(scene.key(duplicated), Some(&duplicate_key));
	assert!(std::ptr::eq(
		scene.get(duplicated).unwrap().model(),
		model.as_ref()
	));
	assert!(history.redo_scene(&mut scene).unwrap().is_none());
}

#[test]
fn structural_errors_and_no_op_transform_preserve_the_redo_branch() {
	let model = model();
	let mut scene = Scene::default();
	let source = add(&mut scene, "source", &model);
	let mut history = CommandHistory::default();
	let spawned = history
		.spawn(&mut scene, "new", "props/robot.glb", instance(&model))
		.unwrap();
	assert!(history.undo(&mut scene).is_err());
	history.undo_scene(&mut scene).unwrap();
	assert!(history.redo(&mut scene).is_err());
	assert!(history.delete(&mut scene, spawned).is_err());
	assert!(history.duplicate(&mut scene, spawned, [0.0; 3]).is_err());
	let mut invalid = instance(&model);
	invalid.transform_mut().position[0] = f32::NAN;
	assert!(
		history
			.spawn(&mut scene, "bad", "props/robot.glb", invalid)
			.is_err()
	);
	assert!(
		history
			.execute(
				&mut scene,
				SetTransform {
					entity: source,
					transform: Transform::default()
				},
				true
			)
			.unwrap()
			.is_none()
	);
	assert_eq!(
		history.redo_scene(&mut scene).unwrap().unwrap().entity,
		spawned
	);
}

#[test]
fn failed_structural_undo_preserves_record_for_retry_without_partial_changes() {
	let model = model();
	let mut scene = Scene::default();
	let first = add(&mut scene, "first", &model);
	let other = add(&mut scene, "other", &model);
	scene.set_binding("role", first).unwrap();
	let mut history = CommandHistory::default();
	history.delete(&mut scene, first).unwrap();
	scene.set_binding("role", other).unwrap();
	let before = saved(&scene);
	for _ in 0..2 {
		assert!(history.undo_scene(&mut scene).is_err());
		assert_eq!(saved(&scene), before);
	}
	scene.remove(other).unwrap();
	assert_eq!(
		history.undo_scene(&mut scene).unwrap().unwrap().entity,
		first
	);
	assert_eq!(scene.binding("role"), Some(first));
}

#[test]
fn failed_structural_redo_preserves_record_and_a_successful_new_edit_discards_it() {
	let model = model();
	let mut scene = Scene::default();
	let mut history = CommandHistory::default();
	let spawned = history
		.spawn(&mut scene, "spawned", "props/robot.glb", instance(&model))
		.unwrap();
	let key = scene.key(spawned).unwrap().clone();
	history.undo_scene(&mut scene).unwrap();
	let collision = scene
		.spawn_keyed_model(
			key.clone(),
			"collision",
			"props/robot.glb",
			instance(&model),
		)
		.unwrap();
	let before = saved(&scene);
	for _ in 0..2 {
		assert!(history.redo_scene(&mut scene).is_err());
		assert_eq!(saved(&scene), before);
	}
	scene.remove(collision).unwrap();
	assert_eq!(
		history.redo_scene(&mut scene).unwrap().unwrap().entity,
		spawned
	);
	history.undo_scene(&mut scene).unwrap();
	let branch = history
		.spawn(&mut scene, "branch", "props/robot.glb", instance(&model))
		.unwrap();
	assert!(history.redo_scene(&mut scene).unwrap().is_none());
	assert!(!scene.contains(spawned));
	assert!(scene.contains(branch));
}
