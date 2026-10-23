use super::*;
use crate::{assets::Model, model_instance::ModelInstance};
use std::sync::Arc;

fn fixture() -> (Scene, EntityId) {
	let mut scene = Scene::default();
	let id = scene.spawn(ModelInstance::new(
		Arc::new(Model::new(Vec::new())),
		Transform::default(),
	));
	(scene, id)
}

fn command(entity: EntityId, x: f32) -> SetTransform {
	SetTransform {
		entity,
		transform: Transform {
			position: [x, 0.0, 0.0],
			..Transform::default()
		},
	}
}

fn x(scene: &Scene, id: EntityId) -> f32 {
	scene.get(id).unwrap().transform().position[0]
}

#[test]
fn apply_reports_before_and_after_and_restores_every_transform_field() {
	let (mut scene, id) = fixture();
	let mut history = CommandHistory::default();
	let target = Transform {
		position: [1.0, 2.0, 3.0],
		rotation: [0.1, 0.2, 0.3],
		scale: [-1.0, 0.0, 2.0],
	};
	let change = history
		.execute(
			&mut scene,
			SetTransform {
				entity: id,
				transform: target,
			},
			false,
		)
		.unwrap()
		.unwrap();
	assert_eq!(change.entity, id);
	assert_eq!(change.before, Transform::default());
	assert_eq!(change.after, target);
	assert_eq!(*scene.get(id).unwrap().transform(), target);
	history.undo(&mut scene).unwrap().unwrap();
	assert_eq!(*scene.get(id).unwrap().transform(), Transform::default());
	history.redo(&mut scene).unwrap().unwrap();
	assert_eq!(*scene.get(id).unwrap().transform(), target);
}

#[test]
fn invalid_and_missing_entity_edits_leave_scene_and_redo_usable() {
	let (mut scene, id) = fixture();
	let mut history = CommandHistory::default();
	history
		.execute(&mut scene, command(id, 1.0), false)
		.unwrap();
	history.undo(&mut scene).unwrap();
	let mut invalid = command(id, 5.0);
	invalid.transform.rotation[1] = f32::NAN;
	assert!(history.execute(&mut scene, invalid, false).is_err());
	assert_eq!(x(&scene, id), 0.0);
	let missing = scene.spawn(ModelInstance::new(
		Arc::new(Model::new(Vec::new())),
		Transform::default(),
	));
	scene.remove(missing).unwrap();
	assert!(
		history
			.execute(&mut scene, command(missing, 4.0), false)
			.is_err()
	);
	history.redo(&mut scene).unwrap().unwrap();
	assert_eq!(x(&scene, id), 1.0);
}

#[test]
fn no_op_preserves_redo_but_a_new_edit_discards_the_redo_branch() {
	let (mut scene, id) = fixture();
	let mut history = CommandHistory::default();
	history
		.execute(&mut scene, command(id, 1.0), false)
		.unwrap();
	history.undo(&mut scene).unwrap();
	assert!(
		history
			.execute(&mut scene, command(id, 0.0), true)
			.unwrap()
			.is_none()
	);
	history.redo(&mut scene).unwrap().unwrap();
	history.undo(&mut scene).unwrap();
	history.execute(&mut scene, command(id, 2.0), true).unwrap();
	assert!(history.redo(&mut scene).unwrap().is_none());
	history.undo(&mut scene).unwrap();
	assert_eq!(x(&scene, id), 0.0);
}

#[test]
fn a_merged_edit_after_undo_starts_a_new_branch_without_merging_older_history() {
	let (mut scene, id) = fixture();
	let mut history = CommandHistory::default();
	history
		.execute(&mut scene, command(id, 1.0), false)
		.unwrap();
	history
		.execute(&mut scene, command(id, 2.0), false)
		.unwrap();
	history.undo(&mut scene).unwrap();
	history.execute(&mut scene, command(id, 3.0), true).unwrap();
	assert!(history.redo(&mut scene).unwrap().is_none());
	history.undo(&mut scene).unwrap();
	assert_eq!(x(&scene, id), 1.0);
	history.undo(&mut scene).unwrap();
	assert_eq!(x(&scene, id), 0.0);
}

#[test]
fn merging_keeps_the_original_before_value_and_latest_after_value() {
	let (mut scene, id) = fixture();
	let mut history = CommandHistory::default();
	for value in [1.0, 2.0, 3.0] {
		history
			.execute(&mut scene, command(id, value), true)
			.unwrap();
	}
	let change = history.undo(&mut scene).unwrap().unwrap();
	assert_eq!(change.before.position[0], 0.0);
	assert_eq!(change.after.position[0], 3.0);
	assert!(history.undo(&mut scene).unwrap().is_none());
	history.redo(&mut scene).unwrap();
	assert_eq!(x(&scene, id), 3.0);
}

#[test]
fn merging_never_combines_different_entities_or_separate_gestures() {
	let (mut scene, first) = fixture();
	let second = scene.spawn(scene.get(first).unwrap().clone());
	let mut history = CommandHistory::default();
	history
		.execute(&mut scene, command(first, 1.0), false)
		.unwrap();
	history
		.execute(&mut scene, command(second, 2.0), true)
		.unwrap();
	history
		.execute(&mut scene, command(second, 3.0), false)
		.unwrap();
	for (a, b) in [(1.0, 2.0), (1.0, 0.0), (0.0, 0.0)] {
		history.undo(&mut scene).unwrap().unwrap();
		assert_eq!((x(&scene, first), x(&scene, second)), (a, b));
	}
	assert!(history.undo(&mut scene).unwrap().is_none());
}

#[test]
fn failed_undo_and_redo_retain_their_records_until_explicit_clear() {
	for undo in [true, false] {
		let (mut scene, id) = fixture();
		let mut history = CommandHistory::default();
		history
			.execute(&mut scene, command(id, 1.0), false)
			.unwrap();
		if !undo {
			history.undo(&mut scene).unwrap();
		}
		scene.remove(id).unwrap();
		for _ in 0..2 {
			let result = if undo {
				history.undo(&mut scene)
			} else {
				history.redo(&mut scene)
			};
			assert!(result.is_err());
		}
		history.clear();
		assert!(history.undo(&mut scene).unwrap().is_none());
		assert!(history.redo(&mut scene).unwrap().is_none());
	}
}

#[test]
fn bounded_history_evicts_only_the_oldest_changes_and_redoes_in_order() {
	let (mut scene, id) = fixture();
	let mut history = CommandHistory::default();
	for value in 1..=MAX_UNDO_ENTRIES + 1 {
		history
			.execute(&mut scene, command(id, value as f32), false)
			.unwrap();
	}
	for expected in (1..=MAX_UNDO_ENTRIES).rev() {
		history.undo(&mut scene).unwrap().unwrap();
		assert_eq!(x(&scene, id), expected as f32);
	}
	assert!(history.undo(&mut scene).unwrap().is_none());
	for expected in 2..=MAX_UNDO_ENTRIES + 1 {
		history.redo(&mut scene).unwrap().unwrap();
		assert_eq!(x(&scene, id), expected as f32);
	}
	assert!(history.redo(&mut scene).unwrap().is_none());
}
