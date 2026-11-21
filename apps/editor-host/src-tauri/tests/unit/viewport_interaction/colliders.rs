use super::*;

#[test]
fn selected_collider_outlines_use_effective_hierarchy_geometry_including_hidden_entities() {
	use runtime::{collision::DebugShape, scene_data::ColliderData};
	let fixture = Fixture::new(3);
	let mut workspace = fixture.open();
	let mut document = (*workspace.backend.session().authored_document()).clone();
	document.entities[0].transform.scale = [2.0, 3.0, 4.0];
	document.entities[0].transform.rotation = [0.0, 0.4, 0.0];
	for entity in &mut document.entities[1..] {
		entity.parent = Some(EntityKey::new("entity-0").unwrap());
		entity.transform.rotation = [0.0, 0.7, 0.0];
		entity.renderable = false;
	}
	document.entities[1].collider = Some(ColliderData::Box {
		interaction: Default::default(),
		half_extents: [1.0, 2.0, 3.0],
		offset: [0.5, 0.0, 0.0],
	});
	document.entities[2].collider = Some(ColliderData::Sphere {
		interaction: Default::default(),
		radius: 0.7,
		offset: [0.0, 1.0, 0.0],
	});
	let expected = runtime::collision::debug_shapes(&document).unwrap();
	workspace.backend.replace_document(document).unwrap();
	let mut controller = controller(&mut workspace);
	controller
		.action(
			ViewportActionDto::SetColliders { visible: true },
			&mut workspace,
			EXTENT,
		)
		.unwrap();
	controller.settings.tool = ViewportTool::Hand;
	for shape in expected {
		let (entity, min, max, count) = match shape {
			DebugShape::Box { entity, min, max } => (entity, min, max, 12),
			DebugShape::Sphere {
				entity, min, max, ..
			} => (entity, min, max, 96),
		};
		command(
			&mut workspace,
			Command::Select {
				keys: vec![entity.as_str().into()],
			},
		);
		controller.sync(&mut workspace).unwrap();
		let visuals = collider_overlay(&controller);
		assert_eq!(visuals.lines.len(), count);
		for axis in 0..3 {
			let points = visuals
				.lines
				.iter()
				.flat_map(|line| [line.start[axis], line.end[axis]])
				.collect::<Vec<_>>();
			let actual_min = points.iter().copied().fold(f32::INFINITY, f32::min);
			let actual_max = points.iter().copied().fold(f32::NEG_INFINITY, f32::max);
			assert!((actual_min - min[axis]).abs() < 1e-4);
			assert!((actual_max - max[axis]).abs() < 1e-4);
		}
	}
	assert!(controller.state().collider_error.is_none());
}

#[test]
fn sensor_outlines_change_style_without_changing_geometry_and_follow_undo() {
	use runtime::scene_data::{ColliderData, ColliderInteraction};
	let fixture = Fixture::new(1);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	select(&mut workspace, &["entity-0"]);
	controller
		.action(
			ViewportActionDto::SetColliders { visible: true },
			&mut workspace,
			EXTENT,
		)
		.unwrap();
	for sphere in [false, true] {
		let collider = |sensor| {
			let interaction = ColliderInteraction {
				sensor,
				..Default::default()
			};
			if sphere {
				ColliderData::Sphere {
					radius: 0.75,
					offset: [0.0, 1.0, 0.0],
					interaction,
				}
			} else {
				ColliderData::Box {
					half_extents: [0.5, 1.0, 0.5],
					offset: [0.0, 1.0, 0.0],
					interaction,
				}
			}
		};
		command(
			&mut workspace,
			Command::SetCollider {
				key: "entity-0".into(),
				collider: Some(collider(false)),
			},
		);
		controller.sync(&mut workspace).unwrap();
		let solid = collider_overlay(&controller);
		let builds = controller.collider_visuals.rebuilds;
		command(
			&mut workspace,
			Command::SetCollider {
				key: "entity-0".into(),
				collider: Some(collider(true)),
			},
		);
		controller.sync(&mut workspace).unwrap();
		let sensor = collider_overlay(&controller);
		assert!(controller.collider_visuals.rebuilds > builds);
		assert_ne!(sensor.lines[0].color, solid.lines[0].color);
		assert_ne!(sensor.lines.len(), solid.lines.len());
		for axis in 0..3 {
			let bounds = |visuals: &EditorVisuals| {
				visuals
					.lines
					.iter()
					.flat_map(|line| [line.start[axis], line.end[axis]])
					.fold([f32::INFINITY, f32::NEG_INFINITY], |[min, max], point| {
						[min.min(point), max.max(point)]
					})
			};
			let expected = bounds(&solid);
			let actual = bounds(&sensor);
			assert!((expected[0] - actual[0]).abs() < 1e-4);
			assert!((expected[1] - actual[1]).abs() < 1e-4);
		}
		command(&mut workspace, Command::Undo);
		controller.sync(&mut workspace).unwrap();
		assert_eq!(collider_overlay(&controller), solid);
		command(&mut workspace, Command::Redo);
		controller.sync(&mut workspace).unwrap();
		assert_eq!(collider_overlay(&controller), sensor);
	}
	controller
		.action(
			ViewportActionDto::SetView {
				view: ViewportView::Game,
			},
			&mut workspace,
			EXTENT,
		)
		.unwrap();
	assert!(controller
		.visuals(&workspace, EXTENT, 1.0)
		.unwrap()
		.lines
		.is_empty());
}

#[test]
fn collider_cache_reuses_stationary_geometry_and_tracks_native_preview_cancel_and_undo() {
	use runtime::scene_data::ColliderData;
	let fixture = Fixture::new(2);
	let mut workspace = fixture.open();
	command(
		&mut workspace,
		Command::SetCollider {
			key: "entity-0".into(),
			collider: Some(ColliderData::Sphere {
				interaction: Default::default(),
				radius: 0.5,
				offset: [0.0; 3],
			}),
		},
	);
	command(
		&mut workspace,
		Command::Select {
			keys: vec!["entity-0".into()],
		},
	);
	let mut controller = controller(&mut workspace);
	controller
		.action(
			ViewportActionDto::SetColliders { visible: true },
			&mut workspace,
			EXTENT,
		)
		.unwrap();
	let original = collider_overlay(&controller);
	let builds = controller.collider_visuals.rebuilds;
	let index_builds = controller.collider_visuals.index_builds;
	for _ in 0..120 {
		controller.sync(&mut workspace).unwrap();
		controller.visuals(&workspace, EXTENT, 1.0).unwrap();
	}
	assert_eq!(controller.collider_visuals.rebuilds, builds);
	assert_eq!(controller.collider_visuals.index_builds, index_builds);
	let start = axis_drag_start(&mut controller, &mut workspace);
	send(
		&mut controller,
		&mut workspace,
		event("pointer_move", [start[0] + 24.0, start[1]], None),
	);
	assert_ne!(collider_overlay(&controller), original);
	assert_eq!(
		controller.collider_visuals.index_builds, index_builds,
		"Live transforms reuse entity indices"
	);
	controller.cancel(&mut workspace).unwrap();
	controller.sync(&mut workspace).unwrap();
	assert_eq!(collider_overlay(&controller), original);
	let start = axis_drag_start(&mut controller, &mut workspace);
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
	command(&mut workspace, Command::Undo);
	controller.sync(&mut workspace).unwrap();
	assert_eq!(collider_overlay(&controller), original);
	command(
		&mut workspace,
		Command::Select {
			keys: vec!["entity-1".into()],
		},
	);
	controller.sync(&mut workspace).unwrap();
	assert!(collider_overlay(&controller).lines.is_empty());
	workspace.generation += 1;
	controller.sync(&mut workspace).unwrap();
	assert!(controller.collider_visuals.index_builds > index_builds);
}

#[test]
fn invalid_selected_collision_geometry_is_nonfatal_and_does_not_hide_other_errors_or_shapes() {
	use runtime::scene_data::ColliderData;
	let fixture = Fixture::new(2);
	let mut workspace = fixture.open();
	let mut document = (*workspace.backend.session().authored_document()).clone();
	document.entities[0].transform.scale = [1e20; 3];
	document.entities[0].collider = Some(ColliderData::Sphere {
		interaction: Default::default(),
		radius: 1e30,
		offset: [0.0; 3],
	});
	document.entities[1].collider = Some(ColliderData::Sphere {
		interaction: Default::default(),
		radius: 0.5,
		offset: [0.0; 3],
	});
	workspace.backend.replace_document(document).unwrap();
	command(
		&mut workspace,
		Command::Select {
			keys: vec!["entity-0".into(), "entity-1".into()],
		},
	);
	let mut controller = controller(&mut workspace);
	controller.settings.tool = ViewportTool::Hand;
	controller
		.action(
			ViewportActionDto::SetColliders { visible: true },
			&mut workspace,
			EXTENT,
		)
		.unwrap();
	controller.report("Existing action failure");
	assert!(controller
		.state()
		.collider_error
		.as_deref()
		.unwrap()
		.contains("entity-0"));
	assert_eq!(collider_overlay(&controller).lines.len(), 96);
	controller
		.visuals(&workspace, EXTENT, 1.0)
		.unwrap()
		.validate()
		.unwrap();
	assert_eq!(
		controller.state().error.as_deref(),
		Some("Existing action failure")
	);
	let revision = controller.revision();
	command(
		&mut workspace,
		Command::Select {
			keys: vec!["entity-1".into()],
		},
	);
	controller.sync(&mut workspace).unwrap();
	assert!(controller.state().collider_error.is_none());
	assert!(controller.revision() > revision);
	assert_eq!(
		controller.state().error.as_deref(),
		Some("Existing action failure")
	);
}

#[test]
fn collider_outlines_are_bounded_and_disabled_in_game_view() {
	use runtime::scene_data::ColliderData;
	let fixture = Fixture::new(128);
	let mut workspace = fixture.open();
	let mut document = (*workspace.backend.session().authored_document()).clone();
	for entity in &mut document.entities {
		entity.collider = Some(ColliderData::Sphere {
			interaction: Default::default(),
			radius: 0.5,
			offset: [0.0; 3],
		});
	}
	let keys = document
		.entities
		.iter()
		.map(|entity| entity.key.as_str().into())
		.collect();
	workspace.backend.replace_document(document).unwrap();
	command(&mut workspace, Command::Select { keys });
	let mut controller = controller(&mut workspace);
	controller.settings.tool = ViewportTool::Hand;
	controller
		.action(
			ViewportActionDto::SetColliders { visible: true },
			&mut workspace,
			EXTENT,
		)
		.unwrap();
	assert!(controller
		.state()
		.collider_error
		.as_deref()
		.unwrap()
		.contains("limit"));
	let visuals = controller.visuals(&workspace, EXTENT, 1.0).unwrap();
	assert!(visuals.lines.len() <= renderer::MAX_EDITOR_LINES);
	visuals.validate().unwrap();
	controller
		.action(
			ViewportActionDto::SetView {
				view: ViewportView::Game,
			},
			&mut workspace,
			EXTENT,
		)
		.unwrap();
	assert!(controller
		.visuals(&workspace, EXTENT, 1.0)
		.unwrap()
		.lines
		.is_empty());
	assert!(controller.state().collider_error.is_none());
	controller
		.action(
			ViewportActionDto::SetView {
				view: ViewportView::Scene,
			},
			&mut workspace,
			EXTENT,
		)
		.unwrap();
	assert!(controller.state().collider_error.is_some());
	controller
		.action(
			ViewportActionDto::SetColliders { visible: false },
			&mut workspace,
			EXTENT,
		)
		.unwrap();
	assert!(collider_overlay(&controller).lines.is_empty());
	assert!(controller.state().collider_error.is_none());
	let mut document = (*workspace.backend.session().authored_document()).clone();
	for entity in &mut document.entities[..127] {
		entity.collider = None;
	}
	let keys = document
		.entities
		.iter()
		.map(|entity| entity.key.as_str().into())
		.collect();
	workspace.backend.replace_document(document).unwrap();
	command(&mut workspace, Command::Select { keys });
	controller
		.action(
			ViewportActionDto::SetColliders { visible: true },
			&mut workspace,
			EXTENT,
		)
		.unwrap();
	assert_eq!(
		collider_overlay(&controller).lines.len(),
		96,
		"Noncollider selections do not consume the geometry budget"
	);
	assert!(controller.state().collider_error.is_none());
}

#[test]
fn a_4096_entity_selection_has_a_bounded_visual_payload() {
	let fixture = Fixture::new(4096);
	let mut workspace = fixture.open();
	let mut controller = controller(&mut workspace);
	let keys = workspace
		.backend
		.session()
		.authored_document()
		.entities
		.iter()
		.map(|entity| entity.key.as_str().to_owned())
		.collect();
	command(&mut workspace, Command::Select { keys });
	for tool in [
		ViewportTool::Move,
		ViewportTool::Rotate,
		ViewportTool::Scale,
	] {
		controller
			.action(ViewportActionDto::SetTool { tool }, &mut workspace, EXTENT)
			.unwrap();
		let visuals = controller.visuals(&workspace, EXTENT, 2.0).unwrap();
		assert!(visuals.grid.is_some());
		assert!(
			visuals.lines.len() < 512,
			"Selected groups must use bounded group decoration"
		);
		visuals.validate().unwrap();
	}
}
