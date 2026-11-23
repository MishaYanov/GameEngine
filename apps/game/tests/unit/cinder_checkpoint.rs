use super::*;
use audit::{AuditReview, Direction};
use serde_json::{Value, json};
use std::{
	fs,
	sync::atomic::{AtomicU64, Ordering},
};
use store::StaleTransfer;

struct Fixture {
	root: PathBuf,
	source: PathBuf,
	destination: PracticeStore,
	archive: TransferFile,
	arena: Checkpoint,
}

impl Fixture {
	fn new() -> Self {
		static NEXT: AtomicU64 = AtomicU64::new(0);
		let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
		let root =
			std::env::temp_dir().join(format!("cinder-practice-{}-{sequence}", std::process::id()));
		fs::create_dir(&root).unwrap();
		let manifest = root.join("project.json");
		fs::write(&manifest, b"fixture").unwrap();
		let source = root.join(format!("arena{CHECKPOINT_SUFFIX}"));
		let destination = PracticeStore::new(&root, &manifest).unwrap();
		let archive = TransferFile::new(
			&root.canonicalize().unwrap(),
			source.file_name().unwrap().to_str().unwrap().into(),
			CHECKPOINT_SUFFIX,
		);
		let arena = Checkpoint::arena_entry(b"manifest", b"authored scene");
		fs::write(&source, serde_json::to_vec(&arena).unwrap()).unwrap();
		Self {
			root,
			source,
			destination,
			archive,
			arena,
		}
	}

	fn preflight(&self) -> Preflight {
		self.destination
			.preflight(&self.archive, || Ok(self.arena.clone()))
	}

	fn reviewed(&self) -> Review {
		let Preflight::Ready(review) = self.preflight() else {
			panic!("expected a Ready review");
		};
		*review
	}

	fn slot_path(&self) -> PathBuf {
		let directory = self.root.join("GameEngine/Cinder Oath Practice");
		fs::read_dir(directory)
			.unwrap()
			.map(|entry| entry.unwrap().path())
			.find(|path| {
				path.extension()
					.is_some_and(|extension| extension == "json")
			})
			.unwrap()
	}
}

impl Drop for Fixture {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.root);
	}
}

#[test]
fn preflight_returns_typed_blocks_and_leaves_empty_profile_untouched() {
	let fixture = Fixture::new();
	let ready = fixture.reviewed();
	assert!(!ready.replaces);
	assert_eq!(ready.archive, fixture.arena);
	assert_eq!(
		ready.source_sha256,
		sha256(&fs::read(&fixture.source).unwrap())
	);
	assert_ne!(ready.destination_sha256, [0; 32]);
	assert!(!fixture.root.join("GameEngine").exists());

	for (archive, cause) in [
		(
			Checkpoint::arena_entry(b"another project", b"authored scene"),
			PreflightBlock::IncompatibleProject,
		),
		(
			Checkpoint::arena_entry(b"manifest", b"another scene"),
			PreflightBlock::InvalidAuthoredScene,
		),
	] {
		fs::write(&fixture.source, serde_json::to_vec(&archive).unwrap()).unwrap();
		assert!(matches!(fixture.preflight(), Preflight::Blocked(actual) if actual == cause));
	}
	fs::write(&fixture.source, b"{}").unwrap();
	assert!(matches!(
		fixture.preflight(),
		Preflight::Blocked(PreflightBlock::InvalidArchive)
	));

	fs::write(&fixture.source, serde_json::to_vec(&fixture.arena).unwrap()).unwrap();
	// A regular file where the profile folder belongs makes the destination unavailable.
	fs::write(fixture.root.join("GameEngine"), b"unchanged").unwrap();
	assert!(matches!(
		fixture.preflight(),
		Preflight::Blocked(PreflightBlock::DestinationUnavailable)
	));
	assert_eq!(
		fs::read(fixture.root.join("GameEngine")).unwrap(),
		b"unchanged"
	);
}

#[test]
fn stale_archive_and_destination_leave_existing_slot_bytes_untouched_until_fresh_review() {
	let fixture = Fixture::new();
	fixture.destination.save(fixture.arena.clone()).unwrap();
	let reviewed = fixture.reviewed();
	assert!(reviewed.replaces);
	let before = fs::read(fixture.slot_path()).unwrap();

	// Equivalent JSON still requires review when its exact bytes change.
	fs::write(
		&fixture.source,
		serde_json::to_vec_pretty(&fixture.arena).unwrap(),
	)
	.unwrap();
	let apply = |review| {
		fixture
			.destination
			.import(review, || Ok(fixture.arena.clone()))
	};
	assert!(matches!(
		apply(&reviewed),
		Attempt::NoSlotWrite(StaleTransfer::Archive)
	));
	assert_eq!(fs::read(fixture.slot_path()).unwrap(), before);

	let reread = fixture.reviewed();
	assert!(matches!(
		fixture
			.destination
			.import(&reread, || Ok(Checkpoint::arena_entry(
				b"manifest",
				b"updated authored scene"
			))),
		Attempt::NoSlotWrite(StaleTransfer::InstalledArena(
			PreflightBlock::InvalidAuthoredScene
		)),
	));
	assert_eq!(fs::read(fixture.slot_path()).unwrap(), before);

	let other_player =
		PracticeStore::new(&fixture.root, &fixture.root.join("project.json")).unwrap();
	other_player.save(fixture.arena.clone()).unwrap();
	let newer = fs::read(fixture.slot_path()).unwrap();
	assert_ne!(before, newer);
	assert!(matches!(
		apply(&reread),
		Attempt::NoSlotWrite(StaleTransfer::Destination)
	));
	assert_eq!(fs::read(fixture.slot_path()).unwrap(), newer);
	assert!(matches!(apply(&fixture.reviewed()), Attempt::Committed));

	let saved = fixture.destination.checkpoint().unwrap().unwrap();
	assert_eq!(saved, fixture.arena);
	let audit = fixture.destination.audit(Some(Direction::Import)).unwrap();
	let path = fixture.root.join("filtered.checkpoint-audit.json");
	fs::write(&path, audit).unwrap();
	let report = AuditReview::read(&path).unwrap();
	assert_eq!(report.scope.unwrap().direction, Some(Direction::Import));
	assert_eq!(report.entries.len(), 1);
	assert_eq!(report.entries[0].checkpoint_id, fixture.arena.id());
}

#[test]
fn slot_review_is_bound_to_profile_and_reports_inspection_on_publication_failure() {
	let first = Fixture::new();
	let second = Fixture::new();
	let reviewed = first.reviewed();
	assert!(matches!(
		second
			.destination
			.import(&reviewed, || Ok(second.arena.clone())),
		Attempt::NoSlotWrite(StaleTransfer::Destination),
	));
	assert!(!second.root.join("GameEngine").exists());
	let Preflight::Ready(second_review) = second
		.destination
		.preflight(&first.archive, || Ok(second.arena.clone()))
	else {
		panic!("a matching arena can review the checkpoint on a second installation");
	};
	assert!(matches!(
		second
			.destination
			.import(&second_review, || Ok(second.arena.clone())),
		Attempt::Committed,
	));

	first.destination.save(first.arena.clone()).unwrap();
	let path = first.slot_path();
	let mut record: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
	record["generation"] = json!(u64::MAX);
	let unchanged = serde_json::to_vec(&record).unwrap();
	fs::write(&path, &unchanged).unwrap();
	let current = first.reviewed();
	assert!(matches!(
		first.destination.import(&current, || Ok(first.arena.clone())),
		Attempt::InspectSlot(problem) if problem.contains("generation"),
	));
	assert_eq!(fs::read(path).unwrap(), unchanged);
}

#[test]
fn scoped_and_legacy_audits_validate_in_place_and_keep_saved_order() {
	let fixture = Fixture::new();
	fixture.destination.save(fixture.arena.clone()).unwrap();
	let before = fs::read(fixture.slot_path()).unwrap();
	let path = fixture.root.join("disposable.checkpoint-audit.json");

	fs::write(
		&path,
		fixture.destination.audit(Some(Direction::Export)).unwrap(),
	)
	.unwrap();
	let empty = AuditReview::read(&path).unwrap();
	assert!(empty.entries.is_empty());
	assert!(empty.first_completed.is_none());
	assert!(empty.last_completed.is_none());
	assert_eq!(empty.scope.unwrap().project_id, PROJECT_ID);

	let legacy = json!({
		"schema_version": 1,
		"entries": [
			{"slot": 2, "direction": "export", "checkpoint_id": "tutorial_entry", "completed_unix_seconds": 12},
			{"slot": 1, "direction": "import", "checkpoint_id": "room_entry", "completed_unix_seconds": 4}
		]
	});
	let data = serde_json::to_vec(&legacy).unwrap();
	fs::write(&path, &data).unwrap();
	let report = AuditReview::read(&path).unwrap();
	assert!(report.scope.is_none());
	assert_eq!(report.first_completed, Some(4));
	assert_eq!(report.last_completed, Some(12));
	assert_eq!(report.entries[0].completed_unix_seconds, 12);
	assert_eq!(report.source_sha256, sha256(&data));

	let outside_filter = json!({
		"schema_version": 2,
		"scope": {"project_id": PROJECT_ID, "slot": 1, "direction": "export"},
		"entries": [
			{"slot": 1, "direction": "import", "checkpoint_id": CHECKPOINT_ID, "completed_unix_seconds": 1}
		]
	});
	for rejected in [outside_filter, json!({"schema_version": 99, "entries": []})] {
		fs::write(&path, serde_json::to_vec(&rejected).unwrap()).unwrap();
		assert!(AuditReview::read(&path).is_err());
	}
	fs::write(&path, vec![b' '; 32 * 1024 + 1]).unwrap();
	assert!(AuditReview::read(&path).is_err());
	assert_eq!(before, fs::read(fixture.slot_path()).unwrap());
}
