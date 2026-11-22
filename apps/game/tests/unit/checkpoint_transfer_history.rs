use super::*;
use std::{
	fs,
	sync::{
		Arc, Barrier,
		atomic::{AtomicU64, Ordering},
	},
};

struct Fixture(PathBuf);
impl Fixture {
	fn new() -> Self {
		static NEXT: AtomicU64 = AtomicU64::new(0);
		let root = std::env::temp_dir().join(format!(
			"transfer-history-{}-{}",
			std::process::id(),
			NEXT.fetch_add(1, Ordering::Relaxed),
		));
		fs::create_dir(&root).unwrap();
		Self(root)
	}

	fn history(&self) -> TransferHistory<'_> {
		TransferHistory {
			profile: &self.0,
			project_save_id: "test-project",
		}
	}
}

impl Drop for Fixture {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.0);
	}
}

#[test]
fn competing_writers_merge_and_store_no_payload_tokens_or_paths() {
	let fixture = Fixture::new();
	let history = fixture.history();
	assert!(history.slot(SaveSlot::Two).unwrap().is_empty());
	let barrier = Arc::new(Barrier::new(2));
	std::thread::scope(|scope| {
		for (direction, id) in [
			(Direction::Export, "garden_entry"),
			(Direction::Import, "after_gate"),
		] {
			let barrier = Arc::clone(&barrier);
			let history = &history;
			scope.spawn(move || {
				barrier.wait();
				history.append(SaveSlot::Two, direction, id.into()).unwrap();
			});
		}
	});
	let entries = history.slot(SaveSlot::Two).unwrap();
	assert_eq!(entries.len(), 2);
	assert!(
		entries
			.iter()
			.any(|entry| entry.checkpoint_id == "garden_entry"
				&& entry.direction == Direction::Export)
	);
	assert!(
		entries.iter().any(
			|entry| entry.checkpoint_id == "after_gate" && entry.direction == Direction::Import
		)
	);
	assert!(history.slot(SaveSlot::One).unwrap().is_empty());
	let bytes = fs::read(history.folder(false).unwrap().unwrap().join(FILE)).unwrap();
	let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
	for entry in json["entries"].as_array().unwrap() {
		assert_eq!(entry.as_object().unwrap().len(), 4);
	}
}

#[test]
fn snapshot_is_bounded_newest_first_and_corrupt_bytes_survive() {
	let fixture = Fixture::new();
	let history = fixture.history();
	for index in 0..=MAX_ENTRIES {
		history
			.append(
				SaveSlot::Three,
				Direction::Import,
				format!("checkpoint_{index}"),
			)
			.unwrap();
	}
	let entries = history.slot(SaveSlot::Three).unwrap();
	assert_eq!(entries.len(), MAX_ENTRIES);
	assert_eq!(entries[0].checkpoint_id, "checkpoint_64");
	assert_eq!(entries.last().unwrap().checkpoint_id, "checkpoint_1");
	let file = history.folder(false).unwrap().unwrap().join(FILE);
	let invalid = vec![b'x'; MAX_BYTES + 1];
	fs::write(&file, &invalid).unwrap();
	assert!(
		history
			.append(SaveSlot::Three, Direction::Import, "will_not_store".into())
			.is_err()
	);
	assert_eq!(fs::read(&file).unwrap(), invalid);
}

#[test]
fn audit_projects_the_reviewed_snapshot_and_preserves_existing_output() {
	let fixture = Fixture::new();
	let history = fixture.history();
	history
		.append(SaveSlot::One, Direction::Export, "courtyard".into())
		.unwrap();
	history
		.append(SaveSlot::Two, Direction::Import, "garden".into())
		.unwrap();
	history
		.append(SaveSlot::Two, Direction::Export, "finish".into())
		.unwrap();
	let profile_snapshot = history.snapshot(None).unwrap();
	let reviewed = history.slot(SaveSlot::Two).unwrap();
	history
		.append(SaveSlot::Two, Direction::Import, "late_append".into())
		.unwrap();
	let output = fixture.0.join("audit.json");
	assert_eq!(publish_audit(&output, &reviewed).unwrap(), 2);
	let original = fs::read(&output).unwrap();
	assert!(publish_audit(&output, &profile_snapshot).is_err());
	assert_eq!(fs::read(&output).unwrap(), original);
	let audit: serde_json::Value = serde_json::from_slice(&original).unwrap();
	assert_eq!(audit["schema_version"], 1);
	let entries = audit["entries"].as_array().unwrap();
	assert_eq!(entries[0]["checkpoint_id"], "finish");
	assert_eq!(entries[1]["checkpoint_id"], "garden");
	assert_eq!(entries[1]["direction"], "import");
	assert!(
		entries
			.iter()
			.all(|row| row["slot"] == 2 && row.as_object().unwrap().len() == 4)
	);
	assert_eq!(profile_snapshot[2].checkpoint_id, "courtyard");
	assert_eq!(
		history.slot(SaveSlot::Two).unwrap()[0].checkpoint_id,
		"late_append"
	);
}

#[test]
fn legacy_retention_migrates_on_write_and_keeps_newest_entries_in_order() {
	let fixture = Fixture::new();
	let history = fixture.history();
	let file = history.folder(true).unwrap().unwrap().join(FILE);
	let mut entries = Vec::new();
	for index in 0..40 {
		entries.push(serde_json::json!({
			"slot": 1,
			"direction": "export",
			"checkpoint_id": format!("legacy_{index}"),
			"completed_unix_seconds": index,
		}));
	}
	let original =
		serde_json::to_vec(&serde_json::json!({"schema_version": 1, "entries": entries})).unwrap();
	fs::write(&file, &original).unwrap();
	let first = history.view(None).unwrap();
	assert_eq!(first.retention, HistoryRetention::Entries64);
	assert_eq!(first.entries.len(), 40);
	let reduce = history
		.review_retention(HistoryRetention::Entries32)
		.unwrap();
	assert_eq!(reduce.rows_to_remove, 8);
	assert_eq!(fs::read(&file).unwrap(), original);
	assert_eq!(history.set_retention(&reduce).unwrap(), 8);

	let mut migrated: serde_json::Value =
		serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
	assert_eq!(migrated["schema_version"], 2);
	assert_eq!(migrated["retention"], 32);
	let middle = history.view(None).unwrap();
	assert_eq!(middle.entries[0].checkpoint_id, "legacy_39");
	assert_eq!(middle.entries[31].checkpoint_id, "legacy_8");
	let stale = history
		.review_retention(HistoryRetention::Entries16)
		.unwrap();
	let other = fixture.history();
	other
		.append(SaveSlot::One, Direction::Import, "newest".into())
		.unwrap();
	assert!(
		history
			.set_retention(&stale)
			.unwrap_err()
			.contains("history changed")
	);
	let fresh = history
		.review_retention(HistoryRetention::Entries16)
		.unwrap();
	assert_eq!(history.set_retention(&fresh).unwrap(), 16);
	let small = fixture.history().view(None).unwrap();
	assert_eq!(small.retention, HistoryRetention::Entries16);
	assert_eq!(small.entries[0].checkpoint_id, "newest");
	assert_eq!(small.entries[15].checkpoint_id, "legacy_25");
	let increase = history
		.review_retention(HistoryRetention::Entries64)
		.unwrap();
	assert_eq!(history.set_retention(&increase).unwrap(), 0);
	assert_eq!(history.view(None).unwrap().entries, small.entries);

	migrated["retention"] = 31.into();
	let invalid = serde_json::to_vec(&migrated).unwrap();
	fs::write(&file, &invalid).unwrap();
	assert!(history.view(None).is_err());
	assert!(
		history
			.append(SaveSlot::One, Direction::Export, "blocked".into())
			.is_err()
	);
	assert_eq!(fs::read(&file).unwrap(), invalid);
}

#[test]
fn clear_review_is_scoped_cancelable_and_bound_to_the_live_record() {
	let fixture = Fixture::new();
	let history = fixture.history();
	history
		.append(SaveSlot::One, Direction::Export, "courtyard".into())
		.unwrap();
	history
		.append(SaveSlot::Two, Direction::Import, "garden".into())
		.unwrap();
	let audit = fixture.0.join("audit.json");
	publish_audit(&audit, &history.snapshot(None).unwrap()).unwrap();
	let audit_bytes = fs::read(&audit).unwrap();
	let folder = history.folder(false).unwrap().unwrap();
	let slot = folder.join("slot-2.json");
	fs::write(&slot, b"existing checkpoint").unwrap();
	let payload = fixture.0.join("archive.courtyard-save.json");
	fs::write(&payload, b"existing export").unwrap();

	let selected = ClearHistoryScope::Slot(SaveSlot::Two);
	let before = fs::read(folder.join(FILE)).unwrap();
	let canceled = history.review_clear(selected).unwrap();
	assert_eq!(canceled.rows_to_remove, 1);
	assert_eq!(fs::read(folder.join(FILE)).unwrap(), before);
	let other = fixture.history();
	std::thread::scope(|scope| {
		scope.spawn(|| {
			other
				.append(SaveSlot::Three, Direction::Import, "later".into())
				.unwrap();
		});
	});
	assert!(
		history
			.clear_reviewed(&canceled)
			.unwrap_err()
			.contains("history changed")
	);
	let fresh = history.review_clear(selected).unwrap();
	assert_eq!(history.clear_reviewed(&fresh).unwrap(), 1);
	assert!(other.slot(SaveSlot::Two).unwrap().is_empty());
	assert_eq!(other.slot(SaveSlot::One).unwrap().len(), 1);
	assert_eq!(other.slot(SaveSlot::Three).unwrap().len(), 1);
	let all = other
		.review_clear(ClearHistoryScope::CurrentProject)
		.unwrap();
	assert_eq!(all.rows_to_remove, 2);
	assert_eq!(other.clear_reviewed(&all).unwrap(), 2);
	assert!(fixture.history().snapshot(None).unwrap().is_empty());
	assert_eq!(fs::read(audit).unwrap(), audit_bytes);
	assert_eq!(fs::read(slot).unwrap(), b"existing checkpoint");
	assert_eq!(fs::read(payload).unwrap(), b"existing export");
}
