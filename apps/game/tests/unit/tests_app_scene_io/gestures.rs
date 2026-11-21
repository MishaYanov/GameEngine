use super::*;

#[test]
fn panel_editing_prefills_degrees_and_blocks_motion_without_committing_a_change() {
	let (mut app, [left, _]) = fixture();
	app.scene.get_mut(left).unwrap().transform_mut().rotation[1] = std::f32::consts::FRAC_PI_2;
	app.tools.panel.begin_edit(
		&app.scene,
		&app.editor,
		crate::editor_panel::Property::Rotation(1),
	);
	let edit = app.tools.panel.edit.as_ref().unwrap();
	assert_eq!(edit.entity, left);
	assert!((edit.text.parse::<f32>().unwrap() - 90.0).abs() < 0.001);
	let before = snapshot(&app);
	app.update(1.0);
	assert_eq!(snapshot(&app), before);
	app.tools.panel.edit = None;
	app.update(0.1);
	assert_ne!(snapshot(&app), before);
}

#[test]
fn drag_finish_is_one_undo_record_cancel_and_play_restore_the_preview() {
	use runtime::gizmo::{GizmoDrag, GizmoMode};
	let (mut app, [left, _]) = fixture();
	app.input.reset();
	let before = *app.scene.get(left).unwrap().transform();
	let make_drag = |camera: &Camera| {
		GizmoDrag::begin(
			left,
			before,
			camera,
			[800, 600],
			[400.0, 300.0],
			GizmoMode::Translate,
			0,
		)
		.unwrap()
	};
	app.tools.drag = Some(make_drag(&app.camera));
	let after = app
		.tools
		.drag
		.as_ref()
		.unwrap()
		.preview([450.0, 300.0])
		.unwrap();
	*app.scene.get_mut(left).unwrap().transform_mut() = after;
	app.finish_drag().unwrap();
	press(&mut app, &[Key::ControlLeft, Key::Z], 0.0);
	assert_eq!(*app.scene.get(left).unwrap().transform(), before);
	app.tools.drag = Some(make_drag(&app.camera));
	*app.scene.get_mut(left).unwrap().transform_mut() = after;
	app.cancel_drag();
	assert_eq!(*app.scene.get(left).unwrap().transform(), before);
	app.tools.drag = Some(make_drag(&app.camera));
	*app.scene.get_mut(left).unwrap().transform_mut() = after;
	app.start_play().unwrap();
	assert!(app.tools.drag.is_none());
	assert_eq!(
		*app.authored
			.as_ref()
			.unwrap()
			.scene
			.get(left)
			.unwrap()
			.transform(),
		before
	);
}

#[test]
fn panel_property_commits_convert_degrees_and_undo_each_field_as_one_edit() {
	use crate::editor_panel::{PanelAction, Property};
	for (property, text) in [
		(Property::Name, "Robot 🤖"),
		(Property::Position(2), "12.5"),
		(Property::Rotation(1), "90"),
		(Property::Scale(0), "-2"),
	] {
		let (mut app, [left, _]) = fixture();
		app.input.reset();
		app.tools.saved = Some(app.scene.to_data(&app.camera).unwrap());
		let before = snapshot(&app);
		let mut expected = *app.scene.get(left).unwrap().transform();
		app.panel_action(PanelAction::Edit(property)).unwrap();
		assert!(app.tools.panel.edit.as_ref().unwrap().replace);
		app.tools.panel.edit.as_mut().unwrap().text = text.into();
		app.commit_property().unwrap();
		assert!(app.tools.panel.edit.is_none());
		match property {
			Property::Name => assert_eq!(app.scene.name(left), Some(text)),
			Property::Position(axis) => expected.position[axis] = 12.5,
			Property::Rotation(axis) => expected.rotation[axis] = std::f32::consts::FRAC_PI_2,
			Property::Scale(axis) => expected.scale[axis] = -2.0,
		}
		assert_eq!(*app.scene.get(left).unwrap().transform(), expected);
		assert!(app.is_dirty());
		let after = snapshot(&app);
		press(&mut app, &[Key::ControlLeft, Key::Z], 0.0);
		assert_eq!(snapshot(&app), before);
		assert!(!app.is_dirty());
		press(&mut app, &[Key::ControlRight, Key::Y], 0.0);
		assert_eq!(snapshot(&app), after);
	}
}

#[test]
fn rejected_property_text_preserves_scene_and_existing_redo() {
	use crate::editor_panel::{PanelAction, Property};
	let (mut app, _) = fixture();
	app.input.reset();
	app.editor
		.rename_selected(&mut app.scene, "Renamed")
		.unwrap();
	let renamed = snapshot(&app);
	press(&mut app, &[Key::ControlLeft, Key::Z], 0.0);
	let before = snapshot(&app);
	for (property, text) in [
		(Property::Position(0), ""),
		(Property::Position(0), "one"),
		(Property::Position(0), "NaN"),
		(Property::Rotation(1), "inf"),
		(Property::Scale(2), "-inf"),
		(Property::Name, " \t"),
		(Property::Name, "line\nbreak"),
	] {
		app.panel_action(PanelAction::Edit(property)).unwrap();
		app.tools.panel.edit.as_mut().unwrap().text = text.into();
		assert!(app.commit_property().is_err(), "accepted {text:?}");
		assert!(app.tools.panel.edit.is_none());
		assert_eq!(snapshot(&app), before);
	}
	app.commit_property().unwrap(); // Enter with no active edit is a no-op.
	press(&mut app, &[Key::ControlLeft, Key::Y], 0.0);
	assert_eq!(snapshot(&app), renamed);
}
