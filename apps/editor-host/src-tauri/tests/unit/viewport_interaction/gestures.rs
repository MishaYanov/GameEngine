use super::*;

#[test]
fn clicking_shift_adds_control_toggles_and_empty_click_clears_selection() {
	let fixture = Fixture::new(2);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	let first = pixel(&controller, Vec3::new(-0.3, 0.0, 0.0));
	let second = pixel(&controller, Vec3::new(3.0, 0.0, 0.0));
	click(&mut controller, &mut workspace, first, false, false);
	assert_eq!(selection(&workspace), ["entity-0"]);
	click(&mut controller, &mut workspace, second, true, false);
	assert_eq!(selection(&workspace), ["entity-0", "entity-1"]);
	click(&mut controller, &mut workspace, first, false, true);
	assert_eq!(selection(&workspace), ["entity-1"]);
	click(&mut controller, &mut workspace, [10.0, 10.0], false, false);
	assert!(selection(&workspace).is_empty());
	assert!(!workspace.backend.session().is_dirty());
}

#[test]
fn marquee_selection_uses_containment_left_to_right_and_overlap_right_to_left() {
	let fixture = Fixture::new(2);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_down", [350.0, 250.0], Some("left")),
	);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_move", [620.0, 350.0], None),
	);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_up", [620.0, 350.0], Some("left")),
	);
	assert_eq!(selection(&workspace), ["entity-0", "entity-1"]);
	select(&mut workspace, &[]);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_down", [590.0, 290.0], Some("left")),
	);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_move", [560.0, 310.0], None),
	);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_up", [560.0, 310.0], Some("left")),
	);
	assert_eq!(selection(&workspace), ["entity-1"]);
}

#[test]
fn one_completed_gizmo_drag_creates_exactly_one_undo_action() {
	let fixture = Fixture::new(1);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	let original = workspace.backend.session().authored_document().entities[0]
		.transform
		.clone();
	let start = axis_drag_start(&mut controller, &mut workspace);
	for delta in [10.0, 20.0, 30.0] {
		send(
			&mut controller,
			&mut workspace,
			event("pointer_move", [start[0] + delta, start[1]], None),
		);
	}
	assert!(workspace.backend.session().has_transaction());
	assert!(
		!workspace.backend.session().state().can_undo,
		"Preview samples are not independent undo actions"
	);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_up", [start[0] + 30.0, start[1]], Some("left")),
	);
	assert!(!workspace.backend.session().has_transaction());
	assert!(workspace.backend.session().state().can_undo);
	assert_ne!(
		workspace.backend.session().authored_document().entities[0].transform,
		original
	);
	command(&mut workspace, Command::Undo);
	assert_eq!(
		workspace.backend.session().authored_document().entities[0].transform,
		original
	);
	assert!(!workspace.backend.session().state().can_undo);
}

#[test]
fn selected_bounds_draw_short_corner_brackets_instead_of_full_edges() {
	let fixture = Fixture::new(1);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	select(&mut workspace, &["entity-0"]);
	controller.settings.tool = ViewportTool::Hand;
	controller.settings.colliders = false;
	for projection in [
		ViewportProjection::Perspective,
		ViewportProjection::Orthographic,
	] {
		controller.settings.projection = projection;
		let visuals = controller.visuals(&workspace, EXTENT, 1.0).unwrap();
		visuals.validate().unwrap();
		assert!((8..=24).contains(&visuals.lines.len()));
		for line in &visuals.lines {
			let project = |point| {
				controller
					.camera
					.project(Vec3::from_array(point), EXTENT, projection)
					.unwrap()
			};
			assert!(project(line.start).distance(project(line.end)) <= 14.02);
			assert!(!line.depth_test);
		}
	}
}

#[test]
fn the_active_gizmo_handle_stays_high_contrast_without_a_hover_target() {
	const HIGHLIGHT: [f32; 4] = [1.0, 0.86, 0.4, 1.0];
	let fixture = Fixture::new(1);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	axis_drag_start(&mut controller, &mut workspace);
	controller.hover_handle = None;
	let visuals = controller.visuals(&workspace, EXTENT, 2.0).unwrap();
	visuals.validate().unwrap();
	let line = visuals.lines.last().unwrap();
	assert_eq!(line.color, HIGHLIGHT);
	assert!(line.width > 6.0);
	assert!(!line.depth_test);
	assert!(visuals.lines.iter().any(|outline| {
		outline.start == line.start
			&& outline.end == line.end
			&& outline.color != HIGHLIGHT
			&& outline.width > line.width
	}));
}

#[test]
fn the_move_handle_commits_the_release_position_without_a_motion_event() {
	let fixture = Fixture::new(1);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	let original = workspace.backend.session().authored_document().entities[0]
		.transform
		.clone();
	let start = axis_drag_start(&mut controller, &mut workspace);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_up", [start[0] + 25.0, start[1]], Some("left")),
	);
	assert!(!workspace.backend.session().has_transaction());
	let moved = workspace.backend.session().authored_document().entities[0]
		.transform
		.clone();
	assert!((moved.position[0] - original.position[0]).abs() > 0.05);
	assert_eq!(moved.position[1..], original.position[1..]);
	assert_eq!(moved.rotation, original.rotation);
	assert_eq!(moved.scale, original.scale);
	command(&mut workspace, Command::Undo);
	assert_eq!(
		workspace.backend.session().authored_document().entities[0].transform,
		original
	);
}

#[test]
fn a_move_handle_explains_when_another_edit_owns_the_transaction() {
	let fixture = Fixture::new(1);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	select(&mut workspace, &["entity-0"]);
	let gizmo = controller.gizmo(&workspace, EXTENT, 1.0).unwrap().unwrap();
	let start = pixel(&controller, gizmo.pivot + gizmo.axes[0] * gizmo.size * 0.7);
	command(
		&mut workspace,
		Command::BeginTransaction {
			label: "Edit Position".into(),
		},
	);

	send(
		&mut controller,
		&mut workspace,
		event("pointer_down", start, Some("left")),
	);

	assert!(!controller.captured());
	assert_eq!(
		controller.state().error.as_deref(),
		Some("Finish the current edit before dragging a transform handle")
	);
	assert_eq!(
		workspace.backend.session().state().transaction.as_deref(),
		Some("Edit Position")
	);
}

#[test]
fn native_transform_motion_publishes_only_a_compact_local_preview_until_commit() {
	let fixture = Fixture::new(1);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	let start = axis_drag_start(&mut controller, &mut workspace);
	assert!(
		controller.take_document_changed(),
		"The begin boundary must refresh the transaction owner"
	);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_move", [start[0] + 24.0, start[1]], None),
	);
	let state = controller.state();
	let preview = state
		.transform_preview
		.expect("Live Inspector requires a compact native preview");
	let document = workspace.backend.session().authored_document();
	assert_eq!(preview.key, document.entities[0].key.as_str());
	assert_eq!(
		preview.transform.position,
		document.entities[0].transform.position
	);
	assert_eq!(
		preview.transform.rotation,
		document.entities[0].transform.rotation
	);
	assert_eq!(
		preview.transform.scale,
		document.entities[0].transform.scale
	);
	assert_ne!(preview.transform.position, [0.0; 3]);
	assert!(
		!controller.take_document_changed(),
		"Pointer motion must not request a full authored snapshot"
	);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_up", [start[0] + 24.0, start[1]], Some("left")),
	);
	assert!(controller.state().transform_preview.is_none());
	assert!(
		controller.take_document_changed(),
		"The commit boundary must publish authoritative history and dirty state"
	);
	assert!(!controller.take_document_changed());
}

#[test]
fn escape_and_focus_loss_restore_the_captured_transform_without_history() {
	for boundary in ["escape", "blurred"] {
		let fixture = Fixture::new(1);
		let mut workspace = fixture.open();
		let mut controller = controller(&mut workspace);
		focus_keyboard(&mut controller, &mut workspace);
		let original = workspace.backend.session().authored_document().entities[0]
			.transform
			.clone();
		let start = axis_drag_start(&mut controller, &mut workspace);
		send(
			&mut controller,
			&mut workspace,
			event("pointer_move", [start[0] + 30.0, start[1]], None),
		);
		send(
			&mut controller,
			&mut workspace,
			if boundary == "escape" {
				key("Escape")
			} else {
				event("blurred", start, None)
			},
		);
		assert!(!workspace.backend.session().has_transaction());
		assert_eq!(
			workspace.backend.session().authored_document().entities[0].transform,
			original
		);
		assert!(!workspace.backend.session().state().can_undo);
		assert!(!controller.captured());
		assert!(controller.state().transform_preview.is_none());
	}
}

#[test]
fn unrelated_button_release_does_not_commit_or_lose_the_primary_gesture() {
	let fixture = Fixture::new(1);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	let start = axis_drag_start(&mut controller, &mut workspace);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_move", [start[0] + 25.0, start[1]], None),
	);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_up", start, Some("right")),
	);
	assert!(workspace.backend.session().has_transaction());
	assert!(controller.drag.is_some());
	assert!(!workspace.backend.session().state().can_undo);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_up", [start[0] + 25.0, start[1]], Some("left")),
	);
	assert!(!workspace.backend.session().has_transaction());
	assert!(workspace.backend.session().state().can_undo);
}

#[test]
fn secondary_pointer_press_cannot_replace_an_active_transform_or_click() {
	let fixture = Fixture::new(1);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	let start = axis_drag_start(&mut controller, &mut workspace);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_down", start, Some("middle")),
	);
	assert!(controller.drag.is_some());
	assert!(controller.navigation.is_none());
	controller.cancel(&mut workspace).unwrap();
	select(&mut workspace, &[]);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_down", [10.0, 10.0], Some("left")),
	);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_down", [10.0, 10.0], Some("right")),
	);
	assert!(controller.click.is_some());
	assert!(controller.navigation.is_none());
	controller.cancel(&mut workspace).unwrap();
}

#[test]
fn changing_tool_with_a_shortcut_cancels_its_active_transform() {
	let fixture = Fixture::new(1);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	focus_keyboard(&mut controller, &mut workspace);
	let original = workspace.backend.session().authored_document().entities[0]
		.transform
		.clone();
	let start = axis_drag_start(&mut controller, &mut workspace);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_move", [start[0] + 20.0, start[1]], None),
	);
	send(&mut controller, &mut workspace, key("KeyE"));
	assert_eq!(controller.settings.tool, ViewportTool::Rotate);
	assert!(!workspace.backend.session().has_transaction());
	assert_eq!(
		workspace.backend.session().authored_document().entities[0].transform,
		original
	);
	assert!(!workspace.backend.session().state().can_undo);
}

#[test]
fn resizing_or_changing_dpi_cancels_capture_and_does_not_commit() {
	let fixture = Fixture::new(1);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	let original = workspace.backend.session().authored_document().entities[0]
		.transform
		.clone();
	let start = axis_drag_start(&mut controller, &mut workspace);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_move", [start[0] + 20.0, start[1]], None),
	);
	controller
		.geometry_changed(&mut workspace, [1200, 900], 1.5)
		.unwrap();
	assert!(!workspace.backend.session().has_transaction());
	assert!(!controller.captured());
	assert_eq!(
		workspace.backend.session().authored_document().entities[0].transform,
		original
	);
	assert!(!workspace.backend.session().state().can_undo);
}

#[test]
fn invalid_or_cancelled_model_placement_never_creates_authored_entities() {
	let fixture = Fixture::new(1);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	let before = serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap();
	assert!(controller
		.action(
			ViewportActionDto::BeginPlacement {
				path: "../outside.glb".into()
			},
			&mut workspace,
			EXTENT
		)
		.is_err());
	controller
		.action(
			ViewportActionDto::BeginPlacement {
				path: "models/triangle.glb".into(),
			},
			&mut workspace,
			EXTENT,
		)
		.unwrap();
	controller
		.action(
			ViewportActionDto::UpdatePlacement { normalized: None },
			&mut workspace,
			EXTENT,
		)
		.unwrap();
	assert!(controller
		.action(ViewportActionDto::CommitPlacement, &mut workspace, EXTENT)
		.is_err());
	controller
		.action(ViewportActionDto::CancelPlacement, &mut workspace, EXTENT)
		.unwrap();
	assert_eq!(
		serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap(),
		before
	);
	assert!(controller.state().placement.is_none());
	controller
		.action(
			ViewportActionDto::Orient {
				orientation: ViewportOrientation::Top,
			},
			&mut workspace,
			EXTENT,
		)
		.unwrap();
	controller
		.action(
			ViewportActionDto::BeginPlacement {
				path: "models/triangle.glb".into(),
			},
			&mut workspace,
			EXTENT,
		)
		.unwrap();
	controller
		.action(
			ViewportActionDto::UpdatePlacement {
				normalized: Some([0.5, 0.5]),
			},
			&mut workspace,
			EXTENT,
		)
		.unwrap();
	assert!(controller.state().placement.unwrap().position.is_some());
	controller
		.action(ViewportActionDto::CancelPlacement, &mut workspace, EXTENT)
		.unwrap();
	assert_eq!(
		serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap(),
		before
	);
}

#[test]
fn resizing_reprojects_an_active_placement_and_preserves_the_submitted_position() {
	let fixture = Fixture::new(1);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	begin_valid_placement(&mut controller, &mut workspace);
	let normalized = [0.8, 0.5];
	controller
		.action(
			ViewportActionDto::UpdatePlacement {
				normalized: Some(normalized),
			},
			&mut workspace,
			EXTENT,
		)
		.unwrap();
	let original = controller.placement.as_ref().unwrap().position.unwrap();
	let resized = [1200, 600];
	controller
		.geometry_changed(&mut workspace, resized, 1.0)
		.unwrap();
	let placement = controller.placement.as_ref().unwrap();
	assert_eq!(placement.normalized, Some(normalized));
	let position = placement.position.unwrap();
	assert!(position.abs_diff_eq(Vec3::new(original.x * 1.5, original.y, original.z), 1e-4));

	controller
		.action(ViewportActionDto::CommitPlacement, &mut workspace, resized)
		.unwrap();
	let ticket = controller.import.as_ref().unwrap().job_id;
	controller
		.geometry_changed(&mut workspace, [900, 600], 1.0)
		.unwrap();
	assert_eq!(
		controller.placement.as_ref().unwrap().position,
		Some(position)
	);
	drain_until_complete(&mut controller, &mut workspace, ticket);
	assert!(controller.state().error.is_none());
	let document = workspace.backend.session().authored_document();
	assert_eq!(document.entities.len(), 2);
	assert_eq!(document.entities[1].transform.position, position.to_array());
}

#[test]
fn a_valid_drop_adopts_one_import_and_one_undo_restores_the_authored_document() {
	let fixture = Fixture::new(1);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	let original = workspace.backend.session().authored_document();
	begin_valid_placement(&mut controller, &mut workspace);
	controller
		.action(ViewportActionDto::CommitPlacement, &mut workspace, EXTENT)
		.unwrap();
	let ticket = controller.import.as_ref().unwrap().job_id;
	assert_eq!(controller.state().gesture.as_deref(), Some("asset_loading"));
	drain_until_complete(&mut controller, &mut workspace, ticket);
	assert!(
		controller.state().error.is_none(),
		"{:?}",
		controller.state().error
	);
	assert!(controller.state().placement.is_none());
	assert!(controller.import.is_none());
	assert_eq!(
		workspace
			.backend
			.session()
			.authored_document()
			.entities
			.len(),
		original.entities.len() + 1
	);
	assert_eq!(workspace.backend.session().selection().len(), 1);
	assert_ne!(
		workspace.backend.session().selection()[0],
		original.entities[0].key
	);
	assert!(controller.take_document_changed());
	assert!(!controller.take_document_changed());
	assert!(workspace.backend.session().state().can_undo);
	command(&mut workspace, Command::Undo);
	assert_eq!(*workspace.backend.session().authored_document(), *original);
	assert!(!workspace.backend.session().state().can_undo);
}

#[test]
fn native_placement_reconciles_a_completion_already_adopted_by_the_ui_notifier_once() {
	let fixture = Fixture::new(1);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	begin_valid_placement(&mut controller, &mut workspace);
	controller
		.action(ViewportActionDto::CommitPlacement, &mut workspace, EXTENT)
		.unwrap();
	let ticket = controller.import.as_ref().unwrap().job_id;
	let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
	loop {
		workspace.drain_asset_jobs();
		if let Some(completion) = workspace
			.recent_completions()
			.into_iter()
			.find(|completion| completion.job_id == ticket)
		{
			assert!(completion.accepted, "{:?}", completion.error);
			break;
		}
		assert!(
			std::time::Instant::now() < deadline,
			"UI notifier did not receive the small fixture import"
		);
		std::thread::sleep(std::time::Duration::from_millis(1));
	}
	assert_eq!(
		workspace
			.backend
			.session()
			.authored_document()
			.entities
			.len(),
		2
	);
	assert!(
		controller.import.is_some(),
		"The native controller has not consumed the UI notification yet"
	);
	assert!(!controller.take_document_changed());
	controller.drain_imports(&mut workspace);
	assert!(controller.import.is_none());
	assert!(controller.placement.is_none());
	assert!(controller.state().gesture.is_none());
	assert!(controller.take_document_changed());
	let revision = controller.state().revision;
	controller.drain_imports(&mut workspace);
	assert!(!controller.take_document_changed());
	assert_eq!(controller.state().revision, revision);
}

#[test]
fn cancelling_a_submitted_drop_before_adoption_cannot_create_a_late_instance() {
	let fixture = Fixture::new(1);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	let original = workspace.backend.session().authored_document();
	begin_valid_placement(&mut controller, &mut workspace);
	controller
		.action(ViewportActionDto::CommitPlacement, &mut workspace, EXTENT)
		.unwrap();
	let ticket = controller.import.as_ref().unwrap().job_id;
	controller
		.action(ViewportActionDto::CancelPlacement, &mut workspace, EXTENT)
		.unwrap();
	drain_until_complete(&mut controller, &mut workspace, ticket);
	assert!(controller.state().placement.is_none());
	assert!(controller.import.is_none());
	assert_eq!(*workspace.backend.session().authored_document(), *original);
	assert!(!workspace.backend.session().state().can_undo);
	assert!(!controller.take_document_changed());
	let completion = workspace
		.recent_completions()
		.into_iter()
		.find(|completion| completion.job_id == ticket)
		.unwrap();
	assert!(!completion.accepted);
	assert_eq!(completion.error_code, Some("cancelled"));
}

#[test]
fn native_move_under_a_nonuniform_rotated_parent_preserves_the_childs_authored_basis() {
	let fixture = Fixture::new(2);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	command(
		&mut workspace,
		Command::SetTransforms {
			edits: vec![
				TransformEditDto {
					key: "entity-0".into(),
					transform: TransformDto {
						position: [0.0; 3],
						rotation: [0.0, 0.35, 0.0],
						scale: [2.0, 1.0, 0.5],
					},
				},
				TransformEditDto {
					key: "entity-1".into(),
					transform: TransformDto {
						position: [1.5, 0.0, 0.0],
						rotation: [0.0, 0.7, 0.0],
						scale: [1.0; 3],
					},
				},
			],
			space: TransformSpace::Local,
			snap: false,
		},
	);
	command(
		&mut workspace,
		Command::Reparent {
			key: "entity-1".into(),
			parent: Some("entity-0".into()),
			keep_world: false,
		},
	);
	cache_targets(&mut controller, &workspace);
	select(&mut workspace, &["entity-1"]);
	let before = workspace.backend.session().authored_document();
	let world_before = before.world_matrices().unwrap()[1].w_axis.truncate();
	let gizmo = controller.gizmo(&workspace, EXTENT, 1.0).unwrap().unwrap();
	let start = pixel(&controller, gizmo.pivot + gizmo.axes[0] * gizmo.size * 0.7);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_down", start, Some("left")),
	);
	assert!(controller.drag.is_some());
	send(
		&mut controller,
		&mut workspace,
		event("pointer_move", [start[0] + 24.0, start[1]], None),
	);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_up", [start[0] + 24.0, start[1]], Some("left")),
	);
	let after = workspace.backend.session().authored_document();
	let world_after = after.world_matrices().unwrap()[1].w_axis.truncate();
	assert!(world_after.x > world_before.x);
	assert!((world_after.y - world_before.y).abs() < 1e-4);
	assert!((world_after.z - world_before.z).abs() < 1e-4);
	assert_eq!(after.entities[0].transform, before.entities[0].transform);
	assert_eq!(
		after.entities[1].transform.rotation,
		before.entities[1].transform.rotation
	);
	assert_eq!(
		after.entities[1].transform.scale,
		before.entities[1].transform.scale
	);
	command(&mut workspace, Command::Undo);
	assert_eq!(*workspace.backend.session().authored_document(), *before);
}
