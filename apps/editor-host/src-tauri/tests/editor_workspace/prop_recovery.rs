use super::*;

#[test]
fn recovering_unavailable_prop_checks_reviewed_bytes_and_preserves_the_instance() {
	let fixture = Fixture::new();
	let mut workspace = fixture.open();
	run(
		&mut workspace,
		Command::Select {
			keys: vec!["a".into()],
		},
	);
	let revision = workspace.backend.session().revision();
	let original = workspace
		.create_linked_prefab_source("prefabs/original.json", Default::default(), revision)
		.unwrap();
	let replacement = workspace
		.create_linked_prefab_source("prefabs/replacement.json", Default::default(), revision)
		.unwrap();
	let placed = workspace
		.instantiate_linked_source(
			&original.path,
			&original.draft_id,
			[0.0; 3],
			Default::default(),
			revision,
		)
		.unwrap();
	let root = &placed.selection[0];
	let revision = workspace.backend.session().revision();
	let review = workspace
		.open_prefab_source(&replacement.path, revision)
		.unwrap();
	assert!(workspace
		.review_linked_source_recovery(root, &review.path, &review.draft_id, revision)
		.unwrap_err()
		.contains("original Prop source is available"));
	fs::remove_file(fixture.0.join(&original.path)).unwrap();
	let before = workspace.backend.session().authored_document();
	let checked = workspace
		.review_linked_source_recovery(root, &review.path, &review.draft_id, revision)
		.unwrap();
	assert_eq!(checked.prior_source_id, original.asset_id);
	assert_eq!(checked.source_id, replacement.asset_id);
	assert_eq!(checked.member_count, 1);
	let path = fixture.0.join(&replacement.path);
	let mut bytes = fs::read(&path).unwrap();
	bytes.extend_from_slice(b"\n");
	fs::write(path, bytes).unwrap();
	assert!(workspace
		.recover_linked_source(root, &review.path, &review.draft_id, revision)
		.unwrap_err()
		.contains("changed on disk"));
	assert_eq!(workspace.backend.session().authored_document(), before);
	workspace.close_prefab_source(&review.draft_id).unwrap();
	let review = workspace
		.open_prefab_source(&replacement.path, revision)
		.unwrap();
	let source = runtime::linked_prefab::LinkedPrefabSource {
		source_id: review.asset_id.clone(),
		path: review.path.clone(),
		fingerprint: review.fingerprint.clone(),
		prefab: runtime::editor::PrefabData::from_json(&review.prefab.to_string()).unwrap(),
	};
	assert!(workspace
		.execute(EditorCommandRequest {
			expected_revision: revision,
			command: Command::RelinkLinkedPrefab {
				root_key: root.clone(),
				source
			},
		})
		.unwrap_err()
		.contains("guarded prefab command"));
	workspace
		.review_linked_source_recovery(root, &review.path, &review.draft_id, revision)
		.unwrap();
	workspace
		.recover_linked_source(root, &review.path, &review.draft_id, revision)
		.unwrap();
	let after = workspace.backend.session().authored_document();
	assert_eq!(
		runtime::linked_prefab::resolve_scene(&after),
		runtime::linked_prefab::resolve_scene(&before)
	);
	let status = workspace
		.linked_prefab_status(root, workspace.backend.session().revision())
		.unwrap()
		.unwrap();
	assert_eq!(status.source_id, replacement.asset_id);
	assert!(status.source_error.is_none());
	run(&mut workspace, Command::Undo);
	assert_eq!(workspace.backend.session().authored_document(), before);
}
