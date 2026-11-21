use super::*;

#[test]
fn collider_actions_on_a_base_prop_roundtrip_incremental_state_and_each_undo_once() {
	use runtime::scene_data::{ColliderData, ColliderInteraction};
	let fixture = collider_fixture();
	let mut workspace = fixture.open();
	let mut client = workspace.backend.snapshot().unwrap();
	assert!(client.project.gameplay_module.is_none());
	let original = client.document.clone();
	let project = client.project.clone();
	let states = [
		None,
		Some(ColliderData::Box {
			interaction: Default::default(),
			half_extents: [0.5; 3],
			offset: [0.0; 3],
		}),
		Some(ColliderData::Box {
			interaction: Default::default(),
			half_extents: [0.5; 3],
			offset: [1.0, -0.25, 2.0],
		}),
		Some(ColliderData::Box {
			interaction: Default::default(),
			half_extents: [0.25, 0.75, 1.5],
			offset: [1.0, -0.25, 2.0],
		}),
		Some(ColliderData::Box {
			interaction: ColliderInteraction {
				sensor: true,
				..Default::default()
			},
			half_extents: [0.25, 0.75, 1.5],
			offset: [1.0, -0.25, 2.0],
		}),
		Some(ColliderData::Box {
			interaction: ColliderInteraction {
				sensor: true,
				layers: 1 << 31,
				mask: 0,
			},
			half_extents: [0.25, 0.75, 1.5],
			offset: [1.0, -0.25, 2.0],
		}),
		Some(ColliderData::Sphere {
			interaction: ColliderInteraction {
				sensor: true,
				layers: 1 << 31,
				mask: 0,
			},
			radius: 1.25,
			offset: [1.0, -0.25, 2.0],
		}),
		None,
	];
	let mut collider_identity = None;
	for (index, collider) in states.iter().enumerate().skip(1) {
		let update = apply_collider_update(
			&mut workspace,
			&mut client,
			Command::SetCollider {
				key: "crate".into(),
				collider: collider.clone(),
			},
		);
		assert_eq!(
			matches!(update, EditorCommandUpdateDto::Snapshot { .. }),
			states[index - 1].is_some() != collider.is_some()
		);
		let mut expected = original.clone();
		expected.entities[0].collider = collider.clone();
		if collider.is_some() {
			let id = client.document.entities[0]
				.component_ids
				.get(runtime::components::COLLIDER_TYPE)
				.expect("A newly added collider has its own stable component identity")
				.clone();
			if let Some(previous) = &collider_identity {
				assert_eq!(&id, previous, "Collider edits retain component identity");
			}
			assert_ne!(
				id.as_str(),
				"collider",
				"New attachments are distinct from migrated singleton identities"
			);
			collider_identity = Some(id.clone());
			expected.entities[0]
				.component_ids
				.insert(runtime::components::COLLIDER_TYPE.into(), id);
		}
		assert_eq!(client.document, expected);
		assert_eq!(client.project, project);
		assert!(!client.project_dirty);
	}
	for collider in states[..states.len() - 1].iter().rev() {
		apply_collider_update(&mut workspace, &mut client, Command::Undo);
		assert_eq!(&client.document.entities[0].collider, collider);
		assert_eq!(
			client.document.entities[0]
				.component_ids
				.get(runtime::components::COLLIDER_TYPE),
			collider.as_ref().and(collider_identity.as_ref())
		);
	}
	assert!(
		!client.can_undo,
		"Each accepted collider action must create exactly one history entry"
	);
	assert_eq!(client.document, original);
	for collider in &states[1..] {
		apply_collider_update(&mut workspace, &mut client, Command::Redo);
		assert_eq!(&client.document.entities[0].collider, collider);
	}
	assert!(!client.can_redo);
}

#[test]
fn collider_previews_cancel_without_history_and_commit_as_one_edit() {
	use runtime::scene_data::{ColliderData, ColliderInteraction};
	let fixture = collider_fixture();
	let mut workspace = fixture.open();
	let mut client = workspace.backend.snapshot().unwrap();
	let original = client.document.clone();
	let previews = [
		ColliderData::Box {
			interaction: Default::default(),
			half_extents: [0.5; 3],
			offset: [0.0; 3],
		},
		ColliderData::Box {
			interaction: ColliderInteraction {
				sensor: true,
				layers: 4,
				mask: 8,
			},
			half_extents: [1.0, 0.5, 0.25],
			offset: [2.0, 0.0, -1.0],
		},
		ColliderData::Sphere {
			interaction: ColliderInteraction {
				sensor: true,
				layers: 4,
				mask: 8,
			},
			radius: 0.75,
			offset: [2.0, 0.0, -1.0],
		},
	];
	for commit in [false, true] {
		apply_collider_update(
			&mut workspace,
			&mut client,
			Command::BeginTransaction {
				label: "Edit collider".into(),
			},
		);
		for collider in &previews {
			apply_collider_update(
				&mut workspace,
				&mut client,
				Command::SetCollider {
					key: "crate".into(),
					collider: Some(collider.clone()),
				},
			);
			assert_eq!(
				client.document.entities[0].collider.as_ref(),
				Some(collider)
			);
		}
		apply_collider_update(
			&mut workspace,
			&mut client,
			if commit {
				Command::CommitTransaction
			} else {
				Command::CancelTransaction
			},
		);
		assert!(client.transaction.is_none());
		if !commit {
			assert_eq!(client.document, original);
			assert!(!client.can_undo);
			assert!(!client.can_redo);
			assert!(!client.dirty);
		}
	}
	let accepted = client.document.clone();
	apply_collider_update(&mut workspace, &mut client, Command::Undo);
	assert_eq!(client.document, original);
	assert!(!client.can_undo);
	apply_collider_update(&mut workspace, &mut client, Command::Redo);
	assert_eq!(client.document, accepted);
	assert!(!client.can_redo);
}

#[test]
fn collider_scene_drafts_and_project_settings_save_together_and_reopen_independently() {
	use runtime::scene_data::{ColliderData, ColliderInteraction};
	let fixture = collider_fixture();
	let main_path = fixture.0.join("scenes/main.json");
	let document = runtime::scene_io::load_data(&main_path).unwrap().data;
	fs::write(
		fixture.0.join("scenes/second.json"),
		document.to_json().unwrap(),
	)
	.unwrap();
	let manifest = fixture.0.parent().unwrap().join("project.json");
	let mut project = runtime::project::ProjectData::load(&manifest).unwrap();
	let second = SceneId::new("second").unwrap();
	project
		.scenes
		.insert(SceneDescriptor::new(second.clone(), "scenes/second.json").unwrap())
		.unwrap();
	fs::write(&manifest, project.to_json().unwrap()).unwrap();
	let mut workspace = fixture.open();
	let mut client = workspace.backend.snapshot().unwrap();
	let first_shape = Some(ColliderData::Box {
		interaction: ColliderInteraction {
			sensor: true,
			layers: 1 << 31,
			mask: 0,
		},
		half_extents: [0.5, 1.0, 2.0],
		offset: [0.0, 0.5, 0.0],
	});
	let second_shape = Some(ColliderData::Sphere {
		interaction: ColliderInteraction {
			sensor: false,
			layers: 0,
			mask: u32::MAX,
		},
		radius: 0.75,
		offset: [1.0, 0.0, 0.0],
	});
	apply_collider_update(
		&mut workspace,
		&mut client,
		Command::SetCollider {
			key: "crate".into(),
			collider: first_shape.clone(),
		},
	);
	assert!(matches!(
		apply_collider_update(
			&mut workspace,
			&mut client,
			Command::SwitchScene {
				scene_id: second.clone()
			}
		),
		EditorCommandUpdateDto::Snapshot { .. }
	));
	assert!(
		client.document.entities[0].collider.is_none(),
		"Reused entity keys must not leak the first scene's collider"
	);
	apply_collider_update(
		&mut workspace,
		&mut client,
		Command::SetCollider {
			key: "crate".into(),
			collider: second_shape.clone(),
		},
	);
	let mut project = client.project.clone();
	project.name = "Collision authoring".into();
	assert!(matches!(
		apply_collider_update(&mut workspace, &mut client, Command::SetProject { project }),
		EditorCommandUpdateDto::Snapshot { .. }
	));
	assert_eq!(client.document.entities[0].collider, second_shape);
	let saved = workspace.save_all(client.revision).unwrap();
	assert_eq!(saved.active_scene_id, second);
	assert!(!saved.dirty);
	assert!(saved.scenes.iter().all(|scene| !scene.dirty));
	for (path, expected) in [
		("scenes/main.json", &first_shape),
		("scenes/second.json", &second_shape),
	] {
		assert_eq!(
			&runtime::scene_io::load_data(&fixture.0.join(path))
				.unwrap()
				.data
				.entities[0]
				.collider,
			expected
		);
	}
	let mut reopened = fixture.open();
	assert_eq!(
		reopened.backend.session().project().name,
		"Collision authoring"
	);
	assert_eq!(
		reopened.backend.session().authored_document().entities[0].collider,
		first_shape
	);
	run(&mut reopened, Command::SwitchScene { scene_id: second });
	let snapshot = reopened.backend.snapshot().unwrap();
	assert_eq!(snapshot.document.entities[0].collider, second_shape);
	assert!(!snapshot.dirty);
	assert!(!snapshot.can_undo);
	let unrelated = collider_fixture();
	assert!(unrelated
		.open()
		.backend
		.session()
		.authored_document()
		.entities[0]
		.collider
		.is_none());
}

#[test]
fn invalid_collider_sizes_preserve_host_state_redo_and_event_journal() {
	use runtime::scene_data::ColliderData;
	let fixture = collider_fixture();
	let mut workspace = fixture.open();
	run(
		&mut workspace,
		Command::SetCollider {
			key: "crate".into(),
			collider: Some(ColliderData::Sphere {
				interaction: Default::default(),
				radius: 0.5,
				offset: [0.0; 3],
			}),
		},
	);
	run(&mut workspace, Command::Undo);
	assert!(workspace.backend.snapshot().unwrap().can_redo);
	let before = serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap();
	let cursor = workspace.backend.change_cursor();
	let event_bytes = workspace.backend.retained_event_bytes();
	let scene_bytes = fs::read(fixture.0.join("scenes/main.json")).unwrap();
	let mut invalid = vec![
		ColliderData::Box {
			interaction: Default::default(),
			half_extents: [0.0, 1.0, 1.0],
			offset: [0.0; 3],
		},
		ColliderData::Box {
			interaction: Default::default(),
			half_extents: [1.0, -1.0, 1.0],
			offset: [0.0; 3],
		},
		ColliderData::Box {
			interaction: Default::default(),
			half_extents: [1.0, 1.0, f32::INFINITY],
			offset: [0.0; 3],
		},
		ColliderData::Sphere {
			interaction: Default::default(),
			radius: f32::NAN,
			offset: [0.0; 3],
		},
		ColliderData::Sphere {
			interaction: Default::default(),
			radius: 1.0,
			offset: [f32::INFINITY, 0.0, 0.0],
		},
	];
	// Exercise real JSON-to-f32 conversion at the host boundary, including a
	// positive JavaScript number that rounds to zero and an overflowing number.
	for radius in [1.0e-50, 1.0e40] {
		let request: EditorCommandRequest = serde_json::from_value(serde_json::json!({
			"expected_revision": workspace.backend.session().revision(),
			"command": {"type": "set_collider", "key": "crate", "collider": {"kind": "sphere", "radius": radius, "offset": [0, 0, 0]}}
		})).unwrap();
		let Command::SetCollider {
			collider: Some(collider),
			..
		} = request.command
		else {
			unreachable!()
		};
		invalid.push(collider);
	}
	for collider in invalid {
		let error = workspace
			.execute_incremental(EditorCommandRequest {
				expected_revision: workspace.backend.session().revision(),
				command: Command::SetCollider {
					key: "crate".into(),
					collider: Some(collider),
				},
			})
			.unwrap_err();
		assert_eq!(error.code, runtime::BackendErrorCode::InvalidCommand);
		assert_eq!(
			serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap(),
			before
		);
		assert_eq!(workspace.backend.change_cursor(), cursor);
		assert_eq!(workspace.backend.retained_event_bytes(), event_bytes);
		assert_eq!(
			fs::read(fixture.0.join("scenes/main.json")).unwrap(),
			scene_bytes
		);
	}
	run(&mut workspace, Command::Redo);
	assert_eq!(
		workspace.backend.session().authored_document().entities[0].collider,
		Some(ColliderData::Sphere {
			interaction: Default::default(),
			radius: 0.5,
			offset: [0.0; 3]
		})
	);
}

#[test]
fn collider_interaction_authoring_is_rejected_during_play_without_changing_history_or_events() {
	use runtime::scene_data::{ColliderData, ColliderInteraction};
	let fixture = collider_fixture();
	let mut workspace = fixture.open();
	run(
		&mut workspace,
		Command::SetCollider {
			key: "crate".into(),
			collider: Some(ColliderData::Box {
				half_extents: [0.5; 3],
				offset: [0.0; 3],
				interaction: ColliderInteraction {
					sensor: false,
					layers: 1 << 31,
					mask: u32::MAX,
				},
			}),
		},
	);
	let authored = workspace.backend.session().authored_document();
	run(&mut workspace, Command::StartPlay);
	let before = serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap();
	let cursor = workspace.backend.change_cursor();
	let bytes = workspace.backend.retained_event_bytes();
	for collider in [
		Some(ColliderData::Box {
			half_extents: [0.5; 3],
			offset: [0.0; 3],
			interaction: ColliderInteraction {
				sensor: true,
				layers: 0,
				mask: 0,
			},
		}),
		Some(ColliderData::Sphere {
			radius: 0.75,
			offset: [1.0, 0.0, 0.0],
			interaction: ColliderInteraction {
				sensor: true,
				layers: 1 << 31,
				mask: 0,
			},
		}),
		None,
	] {
		let error = workspace
			.execute_incremental(EditorCommandRequest {
				expected_revision: workspace.backend.session().revision(),
				command: Command::SetCollider {
					key: "crate".into(),
					collider,
				},
			})
			.unwrap_err();
		assert_eq!(error.code, runtime::BackendErrorCode::CommandRejected);
		assert_eq!(
			serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap(),
			before
		);
		assert_eq!(workspace.backend.change_cursor(), cursor);
		assert_eq!(workspace.backend.retained_event_bytes(), bytes);
		assert_eq!(workspace.backend.session().authored_document(), authored);
	}
	run(&mut workspace, Command::StopPlay);
	assert_eq!(workspace.backend.session().authored_document(), authored);
	run(&mut workspace, Command::Undo);
	let restored = workspace.backend.snapshot().unwrap();
	assert!(restored.document.entities[0].collider.is_none());
	assert!(
		!restored.can_undo,
		"Rejected Play edits must not create history entries"
	);
}
