use runtime::{
	Bounds3, Camera, InputState, Key, SceneEditor, Transform,
	assets::Model,
	model_instance::ModelInstance,
	scene::{EntityId, Scene},
};
use std::sync::Arc;

fn instance() -> ModelInstance {
	ModelInstance::new(
		Arc::new(Model::with_bounds(
			vec![],
			Some(Bounds3::new([-0.5; 3], [0.5; 3]).unwrap()),
		)),
		Transform::default(),
	)
}
fn keys(keys: &[Key]) -> InputState {
	let mut input = InputState::default();
	for key in keys {
		input.press(*key);
	}
	input
}
fn fixture() -> (Scene, SceneEditor, EntityId) {
	let mut scene = Scene::default();
	let id = scene
		.spawn_persistent_model("Free entity", "props/robot.glb", instance())
		.unwrap();
	let editor = SceneEditor::from_scene(&scene);
	(scene, editor, id)
}

#[test]
fn editor_operates_without_game_bindings_and_structural_undo_keeps_selection_valid() {
	let (mut scene, mut editor, first) = fixture();
	assert_eq!(editor.selected_entity(), Some(first));
	let spawned = editor
		.spawn(&mut scene, "New", "props/robot.glb", instance())
		.unwrap();
	let duplicate = editor.duplicate_selected(&mut scene).unwrap().unwrap();
	assert_eq!(editor.selected_entity(), Some(duplicate));
	editor.delete_selected(&mut scene).unwrap();
	assert_eq!(editor.selected_entity(), Some(first));
	for expected in [Some(duplicate), Some(spawned), Some(first)] {
		editor
			.update(&mut scene, &keys(&[Key::ControlLeft, Key::Z]), 0.0)
			.unwrap();
		assert_eq!(editor.selected_entity(), expected);
	}
	for expected in [Some(spawned), Some(duplicate), None] {
		editor
			.update(&mut scene, &keys(&[Key::ControlRight, Key::Y]), 0.0)
			.unwrap();
		assert_eq!(editor.selected_entity(), expected);
	}
	assert!(
		scene
			.to_data(&Camera::default())
			.unwrap()
			.bindings
			.is_empty()
	);
}

#[test]
fn continuous_movement_merges_and_end_gesture_creates_a_separate_undo_step() {
	let (mut scene, mut editor, id) = fixture();
	let mut input = keys(&[Key::W]);
	for _ in 0..3 {
		editor.update(&mut scene, &input, 0.1).unwrap();
		input.end_frame();
	}
	editor.end_gesture();
	editor.update(&mut scene, &input, 0.1).unwrap();
	editor
		.update(&mut scene, &keys(&[Key::ControlLeft, Key::Z]), 1.0)
		.unwrap();
	assert!((scene.get(id).unwrap().transform().position[1] - 0.3).abs() < 1e-5);
	editor
		.update(&mut scene, &keys(&[Key::ControlLeft, Key::Z]), 1.0)
		.unwrap();
	assert_eq!(scene.get(id).unwrap().transform().position, [0.0; 3]);
}

#[test]
fn selecting_another_entity_prevents_merging_its_edits_with_the_previous_one() {
	let (mut scene, mut editor, first) = fixture();
	let second = editor
		.spawn(&mut scene, "Other", "props/robot.glb", instance())
		.unwrap();
	editor.select(&scene, first).unwrap();
	let mut input = keys(&[Key::W]);
	editor.update(&mut scene, &input, 0.2).unwrap();
	input.end_frame();
	editor.select(&scene, second).unwrap();
	editor.update(&mut scene, &input, 0.4).unwrap();
	editor
		.update(&mut scene, &keys(&[Key::ControlLeft, Key::Z]), 0.0)
		.unwrap();
	assert_eq!(scene.get(second).unwrap().transform().position, [0.0; 3]);
	assert_eq!(
		scene.get(first).unwrap().transform().position,
		[0.0, 0.2, 0.0]
	);
}

#[test]
fn picking_a_hit_selects_it_a_miss_clears_selection_and_invalid_pixels_preserve_it() {
	let (mut scene, mut editor, first) = fixture();
	let far = editor
		.spawn(&mut scene, "Far", "props/robot.glb", instance())
		.unwrap();
	scene.get_mut(far).unwrap().transform_mut().position = [0.0, 0.0, -4.0];
	assert_eq!(
		editor
			.pick(&scene, &Camera::default(), [400.0, 300.0], [800, 600])
			.unwrap(),
		Some(first)
	);
	assert!(
		editor
			.pick(&scene, &Camera::default(), [-1.0, 300.0], [800, 600])
			.is_err()
	);
	assert_eq!(editor.selected_entity(), Some(first));
	assert_eq!(
		editor
			.pick(&scene, &Camera::default(), [0.0, 0.0], [800, 600])
			.unwrap(),
		None
	);
	assert_eq!(editor.selected_entity(), None);
	assert_eq!(editor.duplicate_selected(&mut scene).unwrap(), None);
	editor.delete_selected(&mut scene).unwrap();
}

#[test]
fn invalid_spawn_preserves_scene_selection_and_existing_redo() {
	let (mut scene, mut editor, first) = fixture();
	editor.update(&mut scene, &keys(&[Key::W]), 0.5).unwrap();
	editor
		.update(&mut scene, &keys(&[Key::ControlLeft, Key::Z]), 0.0)
		.unwrap();
	for (name, path) in [
		("", "props/robot.glb"),
		("New", "../outside.glb"),
		("New", "C:/robot.glb"),
		("New", "props\\robot.glb"),
	] {
		assert!(editor.spawn(&mut scene, name, path, instance()).is_err());
	}
	assert_eq!(scene.instances().count(), 1);
	assert_eq!(editor.selected_entity(), Some(first));
	editor
		.update(&mut scene, &keys(&[Key::ControlLeft, Key::Y]), 0.0)
		.unwrap();
	assert_eq!(
		scene.get(first).unwrap().transform().position,
		[0.0, 0.5, 0.0]
	);
}

#[test]
fn deleting_the_last_entity_is_undoable_and_error_edits_preserve_redo() {
	let (mut scene, mut editor, first) = fixture();
	editor.delete_selected(&mut scene).unwrap();
	assert_eq!(editor.selected_entity(), None);
	assert_eq!(scene.instances().count(), 0);
	editor.delete_selected(&mut scene).unwrap(); // A no-op must not erase deletion history.
	editor
		.update(&mut scene, &keys(&[Key::ControlLeft, Key::Z]), 0.0)
		.unwrap();
	assert_eq!(editor.selected_entity(), Some(first));
	editor
		.update(&mut scene, &keys(&[Key::ControlLeft, Key::Y]), 0.0)
		.unwrap();
	assert_eq!(editor.selected_entity(), None);
	editor
		.update(&mut scene, &keys(&[Key::ControlLeft, Key::Z]), 0.0)
		.unwrap();
	editor.update(&mut scene, &keys(&[Key::W]), 0.2).unwrap();
	editor
		.update(&mut scene, &keys(&[Key::ControlLeft, Key::Z]), 0.0)
		.unwrap();
	assert!(
		editor
			.update(&mut scene, &keys(&[Key::W]), f32::INFINITY)
			.is_err()
	);
	assert_eq!(scene.get(first).unwrap().transform().position, [0.0; 3]);
	editor
		.update(&mut scene, &keys(&[Key::ControlLeft, Key::Y]), 0.0)
		.unwrap();
	assert_eq!(
		scene.get(first).unwrap().transform().position,
		[0.0, 0.2, 0.0]
	);
}

#[test]
fn rename_validation_and_undo_restore_saved_content_without_changing_identity() {
	let (mut scene, mut editor, id) = fixture();
	let camera = Camera::default();
	let saved = scene.to_data(&camera).unwrap();
	let key = scene.key(id).unwrap().clone();
	editor.rename_selected(&mut scene, "New label 🤖").unwrap();
	assert!(!scene.matches_data(&saved, &camera));
	assert_eq!(scene.key(id), Some(&key));
	editor
		.update(&mut scene, &keys(&[Key::ControlLeft, Key::Z]), 0.0)
		.unwrap();
	assert!(scene.matches_data(&saved, &camera));
	for invalid in ["", " \t", "line\nbreak", "nul\0name"] {
		assert!(editor.rename_selected(&mut scene, invalid).is_err());
	}
	editor.rename_selected(&mut scene, "Free entity").unwrap(); // no-op must preserve redo
	editor
		.update(&mut scene, &keys(&[Key::ControlLeft, Key::Y]), 0.0)
		.unwrap();
	assert_eq!(scene.name(id), Some("New label 🤖"));
}

#[test]
fn a_preview_commits_as_one_undo_record_and_invalid_after_does_not_mutate_it() {
	let (mut scene, mut editor, id) = fixture();
	let before = *scene.get(id).unwrap().transform();
	let mut after = before;
	after.position = [1.0, 2.0, 3.0];
	after.rotation = [0.1, 0.2, 0.3];
	after.scale = [-1.0, 2.0, 0.0];
	*scene.get_mut(id).unwrap().transform_mut() = after;
	editor
		.commit_transform(&mut scene, id, before, after)
		.unwrap();
	assert_eq!(*scene.get(id).unwrap().transform(), after);
	editor
		.update(&mut scene, &keys(&[Key::ControlLeft, Key::Z]), 0.0)
		.unwrap();
	assert_eq!(*scene.get(id).unwrap().transform(), before);
	let mut invalid = after;
	invalid.position[0] = f32::NAN;
	assert!(
		editor
			.commit_transform(&mut scene, id, before, invalid)
			.is_err()
	);
	editor
		.update(&mut scene, &keys(&[Key::ControlLeft, Key::Y]), 0.0)
		.unwrap();
	assert_eq!(*scene.get(id).unwrap().transform(), after);
}

#[test]
fn numeric_transform_edit_is_one_discrete_undo_record_and_noop_preserves_redo() {
	let (mut scene, mut editor, id) = fixture();
	let transform = Transform {
		position: [3.0, 4.0, 5.0],
		rotation: [0.0, std::f32::consts::PI, 0.0],
		scale: [2.0; 3],
	};
	editor
		.set_selected_transform(&mut scene, transform)
		.unwrap();
	editor
		.update(&mut scene, &keys(&[Key::ControlLeft, Key::Z]), 0.0)
		.unwrap();
	editor
		.set_selected_transform(&mut scene, Transform::default())
		.unwrap();
	editor
		.update(&mut scene, &keys(&[Key::ControlLeft, Key::Y]), 0.0)
		.unwrap();
	assert_eq!(*scene.get(id).unwrap().transform(), transform);
}

#[test]
fn invalid_preview_baseline_preserves_live_transform_and_redo() {
	let (mut scene, mut editor, id) = fixture();
	let before = *scene.get(id).unwrap().transform();
	let mut after = before;
	after.position = [1.0, 2.0, 3.0];
	editor.set_selected_transform(&mut scene, after).unwrap();
	editor
		.update(&mut scene, &keys(&[Key::ControlLeft, Key::Z]), 0.0)
		.unwrap();

	for component in 0..3 {
		let mut invalid = before;
		match component {
			0 => invalid.position[0] = f32::NAN,
			1 => invalid.rotation[1] = f32::INFINITY,
			_ => invalid.scale[2] = f32::NEG_INFINITY,
		}
		assert!(
			editor
				.commit_transform(&mut scene, id, invalid, after)
				.is_err()
		);
		assert_eq!(*scene.get(id).unwrap().transform(), before);
	}
	editor
		.update(&mut scene, &keys(&[Key::ControlLeft, Key::Y]), 0.0)
		.unwrap();
	assert_eq!(*scene.get(id).unwrap().transform(), after);
	editor
		.update(&mut scene, &keys(&[Key::ControlLeft, Key::Z]), 0.0)
		.unwrap();
	assert_eq!(*scene.get(id).unwrap().transform(), before);
}
