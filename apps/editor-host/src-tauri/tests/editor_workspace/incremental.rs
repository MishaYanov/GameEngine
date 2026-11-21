use super::*;

#[test]
fn incremental_commands_match_snapshots_through_edit_history_and_play() {
	let fixture = Fixture::new();
	let mut workspace = fixture.open();
	let mut client = workspace.backend.snapshot().unwrap();
	let mut camera = client.document.camera.clone();
	camera.position[0] += 2.0;
	let commands = vec![
		Command::Select {
			keys: vec!["a".into()],
		},
		Command::Rename {
			key: "a".into(),
			name: "Renamed".into(),
		},
		Command::TranslateSelection {
			delta: [1.0, 0.0, 0.0],
			snap: false,
		},
		Command::SetCamera { camera },
		Command::UpsertLight {
			light: runtime::scene_data::LightData {
				key: EntityKey::new("sun").unwrap(),
				name: "Sun".into(),
				kind: runtime::scene_data::LightKind::Directional,
				position: [0.0; 3],
				direction: [0.0, -1.0, 0.0],
				color: [1.0; 3],
				intensity: 1.0,
				range: 10.0,
			},
		},
		Command::RemoveLight { key: "sun".into() },
		Command::BeginTransaction {
			label: "Drag".into(),
		},
		Command::TranslateSelection {
			delta: [2.0, 0.0, 0.0],
			snap: false,
		},
		Command::CancelTransaction,
		Command::Undo,
		Command::Redo,
		Command::DuplicateSelection {
			offset: [1.0, 0.0, 0.0],
		},
		Command::DeleteSelection,
		Command::Undo,
		Command::StartPlay,
		Command::StopPlay,
	];
	for command in commands {
		let update = incremental(&mut workspace, command);
		// The actual JSON contract must survive transport as well as the Rust enum.
		let wire = serde_json::to_vec(&update).unwrap();
		let update = serde_json::from_slice(&wire).unwrap();
		client = apply_incremental(client, &update);
		assert_eq!(
			serde_json::to_value(&client).unwrap(),
			serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap()
		);
	}
}

#[test]
fn incremental_selection_and_rename_payloads_do_not_scale_with_scene_size() {
	let fixture = Fixture::new();
	let mut workspace = fixture.open();
	let mut data = (*workspace.backend.session().authored_document()).clone();
	for index in 0..1000 {
		let mut entity = data.entities[0].clone();
		entity.key = EntityKey::new(format!("extra-{index}")).unwrap();
		data.entities.push(entity);
	}
	let project = workspace.backend.session().project().clone();
	workspace.backend = runtime::Engine::new(data, project).unwrap();
	let full_bytes = serde_json::to_vec(&workspace.backend.snapshot().unwrap())
		.unwrap()
		.len();
	for command in [
		Command::Select {
			keys: vec!["a".into()],
		},
		Command::Rename {
			key: "a".into(),
			name: "Small update".into(),
		},
		Command::Rename {
			key: "a".into(),
			name: "Small update".into(),
		},
	] {
		let update = incremental(&mut workspace, command);
		let EditorCommandUpdateDto::Delta { entities, .. } = &update else {
			panic!("expected compact update")
		};
		assert!(entities.len() <= 1);
		let bytes = serde_json::to_vec(&update).unwrap().len();
		assert!(bytes < 4096, "compact update was {bytes} bytes");
		assert!(bytes * 20 < full_bytes);
	}
}

#[test]
fn incremental_structural_and_project_changes_resync_and_conflicts_preserve_state() {
	let fixture = Fixture::new();
	let mut workspace = fixture.open();
	incremental(
		&mut workspace,
		Command::Select {
			keys: vec!["a".into()],
		},
	);
	let update = incremental(
		&mut workspace,
		Command::DuplicateSelection {
			offset: [1.0, 0.0, 0.0],
		},
	);
	assert!(matches!(update, EditorCommandUpdateDto::Snapshot { .. }));
	let mut project = workspace.backend.session().project().clone();
	project.name = "New name".into();
	assert!(matches!(
		incremental(&mut workspace, Command::SetProject { project }),
		EditorCommandUpdateDto::Snapshot { .. }
	));
	let before = serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap();
	let error = workspace
		.execute_incremental(EditorCommandRequest {
			expected_revision: 0,
			command: Command::DeleteSelection,
		})
		.unwrap_err();
	assert_eq!(error.code, runtime::BackendErrorCode::Conflict);
	assert_eq!(
		before,
		serde_json::to_value(workspace.backend.snapshot().unwrap()).unwrap()
	);
}
