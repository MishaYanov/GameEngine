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
