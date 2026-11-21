use super::*;
use crate::{Transform, assets::Model, model_instance::ModelInstance};
use std::sync::Arc;

fn add(scene: &mut Scene) -> EntityId {
	scene.spawn(ModelInstance::new(
		Arc::new(Model::new(Vec::new())),
		Transform::default(),
	))
}

#[test]
fn cycling_visits_all_entities_in_scene_order_then_wraps() {
	let mut scene = Scene::default();
	let ids = [add(&mut scene), add(&mut scene), add(&mut scene)];
	let mut selection = EntitySelection::default();
	assert_eq!(selection.selected(), None);
	for id in ids.into_iter().cycle().take(7) {
		selection.select_next(&scene);
		assert_eq!(selection.selected(), Some(id));
		assert!(selection.is_selected(id));
	}
}

#[test]
fn failed_selection_preserves_current_and_deleted_selection_recovers() {
	let mut scene = Scene::default();
	let first = add(&mut scene);
	let removed = add(&mut scene);
	let mut selection = EntitySelection::default();
	selection.select(&scene, first).unwrap();
	scene.remove(removed).unwrap();
	assert!(selection.select(&scene, removed).is_err());
	assert_eq!(selection.selected(), Some(first));
	selection.clear_if_missing(&scene);
	assert_eq!(selection.selected(), Some(first));
	scene.remove(first).unwrap();
	let replacement = add(&mut scene);
	selection.select_next(&scene);
	assert_eq!(selection.selected(), Some(replacement));
	scene.remove(replacement).unwrap();
	selection.clear_if_missing(&scene);
	assert_eq!(selection.selected(), None);
}

#[test]
fn empty_single_entity_and_explicit_clear_are_stable() {
	let mut scene = Scene::default();
	let mut selection = EntitySelection::default();
	selection.select_next(&scene);
	assert_eq!(selection.selected(), None);
	let id = add(&mut scene);
	for _ in 0..3 {
		selection.select_next(&scene);
		assert_eq!(selection.selected(), Some(id));
	}
	selection.clear();
	selection.clear();
	assert!(!selection.is_selected(id));
	selection.select_next(&scene);
	scene.remove(id).unwrap();
	selection.select_next(&scene);
	assert_eq!(selection.selected(), None);
}
