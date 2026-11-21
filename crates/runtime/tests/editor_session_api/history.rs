use super::support::{entity, key, scene};
use super::{fixture, reject, run, select, transform};
use bridge::editor::{EditorCommandDto as Command, EditorCommandRequest, TransformSpace};
use runtime::editor::{CommandFactory, EditorSession};

#[test]
fn stale_revisions_and_invalid_commands_are_atomic_and_noops_do_not_bump() {
	let mut s = fixture();
	let before = serde_json::to_value(s.snapshot().unwrap()).unwrap();
	assert!(
		s.execute(EditorCommandRequest {
			expected_revision: 1,
			command: Command::DeleteSelection
		})
		.is_err()
	);
	assert_eq!(serde_json::to_value(s.snapshot().unwrap()).unwrap(), before);
	run(
		&mut s,
		Command::Rename {
			key: "a".into(),
			name: "a".into(),
		},
	);
	assert_eq!(s.revision(), 0);
	reject(
		&mut s,
		Command::Rename {
			key: "a".into(),
			name: " \n".into(),
		},
	);
	reject(
		&mut s,
		Command::Select {
			keys: vec!["a".into(), "missing".into()],
		},
	);
	select(&mut s, &["b", "a", "b"]);
	assert_eq!(s.selection(), &[key("b"), key("a")]);
	let revision = s.revision();
	select(&mut s, &["b", "a"]);
	assert_eq!(s.revision(), revision);
}

#[test]
fn selection_limit_is_enforced_before_mutation_and_capabilities_have_unique_names() {
	let entities = (0..4097)
		.map(|i| entity(&format!("entity-{i}"), [0.0; 3]))
		.collect();
	let mut s = EditorSession::new(scene(entities)).unwrap();
	let keys = (0..4096).map(|i| format!("entity-{i}")).collect::<Vec<_>>();
	run(&mut s, Command::Select { keys: keys.clone() });
	assert_eq!(s.selection().len(), 4096);
	let mut excessive = keys;
	excessive.push("entity-4096".into());
	reject(&mut s, Command::Select { keys: excessive });
	let descriptors = CommandFactory::capabilities();
	let names = descriptors
		.iter()
		.map(|d| d.name.as_str())
		.collect::<std::collections::HashSet<_>>();
	assert_eq!(names.len(), descriptors.len());
	assert!(names.contains("reparent"));
	assert!(names.contains("commit_transaction"));
}

#[test]
fn grouped_edits_commit_one_record_restore_selection_and_cancel_preserves_redo() {
	let mut s = fixture();
	select(&mut s, &["a"]);
	let original = s.authored_document();
	run(
		&mut s,
		Command::BeginTransaction {
			label: "Group".into(),
		},
	);
	run(
		&mut s,
		Command::Rename {
			key: "a".into(),
			name: "Changed".into(),
		},
	);
	select(&mut s, &["b"]);
	run(
		&mut s,
		Command::TranslateSelection {
			delta: [0.0, 3.0, 0.0],
			snap: false,
		},
	);
	reject(&mut s, Command::Undo);
	reject(&mut s, Command::StartPlay);
	assert!(s.mark_saved().is_err());
	run(&mut s, Command::CommitTransaction);
	let changed = s.authored_document();
	run(&mut s, Command::Undo);
	assert_eq!(s.authored_document(), original);
	assert_eq!(s.selection(), &[key("a")]);
	assert!(!s.snapshot().unwrap().can_undo);
	run(
		&mut s,
		Command::BeginTransaction {
			label: "Cancel".into(),
		},
	);
	run(
		&mut s,
		Command::Rename {
			key: "b".into(),
			name: "Discard".into(),
		},
	);
	select(&mut s, &["b"]);
	run(&mut s, Command::CancelTransaction);
	assert_eq!(s.authored_document(), original);
	assert_eq!(s.selection(), &[key("a")]);
	run(&mut s, Command::Redo);
	assert_eq!(s.authored_document(), changed);
	assert_eq!(s.selection(), &[key("b")]);
}

#[test]
fn noop_and_rejected_transactions_leave_redo_available() {
	let mut s = fixture();
	run(
		&mut s,
		Command::Rename {
			key: "a".into(),
			name: "Changed".into(),
		},
	);
	run(&mut s, Command::Undo);
	reject(&mut s, Command::BeginTransaction { label: "\n".into() });
	run(
		&mut s,
		Command::BeginTransaction {
			label: "No-op".into(),
		},
	);
	reject(
		&mut s,
		Command::BeginTransaction {
			label: "Nested".into(),
		},
	);
	reject(
		&mut s,
		Command::SetTransforms {
			edits: vec![transform("a", [1.0; 3]), transform("missing", [2.0; 3])],
			space: TransformSpace::Local,
			snap: false,
		},
	);
	run(&mut s, Command::CommitTransaction);
	assert!(s.snapshot().unwrap().can_redo);
	run(&mut s, Command::Redo);
	assert_eq!(s.authored_document().entities[0].name, "Changed");
}

#[test]
fn history_evicts_oldest_record_at_128_and_a_new_edit_discards_redo() {
	let mut s = fixture();
	for n in 1..=130 {
		run(
			&mut s,
			Command::Rename {
				key: "a".into(),
				name: format!("Name {n}"),
			},
		);
	}
	let mut undone = 0;
	while s.snapshot().unwrap().can_undo {
		run(&mut s, Command::Undo);
		undone += 1;
	}
	assert_eq!(undone, 128);
	assert_eq!(s.authored_document().entities[0].name, "Name 2");
	run(
		&mut s,
		Command::Rename {
			key: "b".into(),
			name: "Branch".into(),
		},
	);
	assert!(!s.snapshot().unwrap().can_redo);
}

#[test]
fn oversized_patch_fails_atomically_and_preserves_redo_and_open_transaction() {
	let entities = (0..2000)
		.map(|i| {
			let mut e = entity(&format!("entity-{i}"), [0.0; 3]);
			e.name = "n".repeat(1024);
			// Keep each command below the target limit while making the owned
			// before/after history patch exceed its separate 16 MiB budget.
			e.model_path = format!("models/{}.glb", "p".repeat(3980));
			e
		})
		.collect();
	let mut s = EditorSession::new(scene(entities)).unwrap();
	run(
		&mut s,
		Command::Rename {
			key: "entity-0".into(),
			name: "small".into(),
		},
	);
	run(&mut s, Command::Undo);
	let edits = (0..2000)
		.map(|i| transform(&format!("entity-{i}"), [1.0; 3]))
		.collect::<Vec<_>>();
	reject(
		&mut s,
		Command::SetTransforms {
			edits: edits.clone(),
			space: TransformSpace::Local,
			snap: false,
		},
	);
	assert!(s.snapshot().unwrap().can_redo);
	run(
		&mut s,
		Command::BeginTransaction {
			label: "Too large".into(),
		},
	);
	run(
		&mut s,
		Command::SetTransforms {
			edits,
			space: TransformSpace::Local,
			snap: false,
		},
	);
	reject(&mut s, Command::CommitTransaction);
	assert!(s.has_transaction());
	run(&mut s, Command::CancelTransaction);
	run(&mut s, Command::Redo);
	assert_eq!(s.authored_document().entities[0].name, "small");
}

#[test]
fn saved_baseline_and_document_replacement_reset_history_and_supersede_old_requests() {
	let mut s = fixture();
	run(
		&mut s,
		Command::Rename {
			key: "a".into(),
			name: "Saved".into(),
		},
	);
	assert!(s.snapshot().unwrap().dirty);
	s.mark_saved().unwrap();
	assert!(!s.snapshot().unwrap().dirty);
	run(&mut s, Command::Undo);
	assert!(s.snapshot().unwrap().dirty);
	let old = s.revision();
	let mut next = EditorSession::new(scene(vec![])).unwrap();
	next.supersede_revision(old).unwrap();
	assert!(next.revision() > old);
	assert!(
		next.execute(EditorCommandRequest {
			expected_revision: old,
			command: Command::StartPlay
		})
		.is_err()
	);
	run(&mut s, Command::StartPlay);
	s.replace_document(scene(vec![])).unwrap();
	let snapshot = s.snapshot().unwrap();
	assert_eq!(snapshot.mode, "edit");
	assert!(!snapshot.dirty);
	assert!(!snapshot.can_undo);
	assert!(snapshot.selection.is_empty());
	assert!(next.supersede_revision(u64::MAX).is_err());
}
