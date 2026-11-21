use super::*;

#[test]
fn incremental_collection_deduplication_and_restart() {
	let fixture = Fixture::new();
	fixture.write(&record("game-123-1", 1));
	fixture.write(&record("compiler-124-2", 1));
	let mut collector = Collector::new(fixture.logs(), &fixture.data());
	assert_eq!(collector.scan().len(), 2);
	assert!(collector.scan().is_empty());
	fixture.write(&record("game-123-1", 2));
	assert_eq!(collector.scan().len(), 1);
	collector.persist();
	let first = collector.page(0, 1, &LogFilter::default());
	assert!(first.has_more);
	assert_eq!(first.next_cursor, 1);
	let mut restored = Collector::new(fixture.logs(), &fixture.data());
	assert!(restored.scan().is_empty());
	assert_eq!(
		restored.page(0, 500, &LogFilter::default()).entries.len(),
		3
	);
	assert_eq!(
		first.collector_id,
		restored.page(0, 1, &LogFilter::default()).collector_id
	);
	fixture.write(&record("game-123-1", 3));
	assert_eq!(restored.scan().len(), 1);
}
#[test]
fn partial_lines_malformed_lines_and_rotated_segments() {
	let fixture = Fixture::new();
	let path = fixture.write(&record("game-123-1", 1));
	let second = serde_json::to_vec(&record("game-123-1", 2)).unwrap();
	let split = second.len() / 2;
	let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
	file.write_all(&second[..split]).unwrap();
	let mut collector = Collector::new(fixture.logs(), &fixture.data());
	assert_eq!(collector.scan().len(), 1);
	assert!(collector.scan().is_empty());
	file.write_all(&second[split..]).unwrap();
	file.write_all(b"\ninvalid\n").unwrap();
	assert_eq!(collector.scan().len(), 1);
	assert_eq!(collector.health.malformed_lines, 1);
	drop(file);
	let rotated = path.with_file_name("000001.jsonl");
	fs::write(
		&rotated,
		format!(
			"{}\n",
			serde_json::to_string(&record("game-123-1", 4)).unwrap()
		),
	)
	.unwrap();
	fs::remove_file(path).unwrap();
	assert_eq!(collector.scan().len(), 1);
	assert_eq!(collector.health.history_gaps, 1);
}
#[test]
fn missing_directory_and_oversized_lines_are_visible() {
	let fixture = Fixture::new();
	let mut collector = Collector::new(fixture.0.join("missing"), &fixture.data());
	collector.scan();
	assert!(collector.health.storage_error.is_some());
	collector.directory = fixture.logs();
	let path = fixture.write(&record("game-123-1", 1));
	let mut file = fs::OpenOptions::new().append(true).open(path).unwrap();
	file.write_all(&vec![b'x'; 512 * 1024 + 10]).unwrap();
	file.write_all(b"\n").unwrap();
	collector.scan();
	assert!(collector.health.storage_error.is_none());
	assert_eq!(collector.health.oversized_lines, 1);
}
#[test]
fn filtered_pagination_retention_and_export() {
	let fixture = Fixture::new();
	let mut collector = Collector::new(fixture.logs(), &fixture.data());
	for i in 1..=5005 {
		let mut entry = record("game-123-1", i);
		entry.level = if i % 2 == 0 {
			Level::Error
		} else {
			Level::Info
		};
		collector.ingest(entry);
	}
	let filter = LogFilter {
		severity: Some(Level::Error),
		search: Some("record".into()),
		..Default::default()
	};
	let page = collector.page(0, 1, &filter);
	assert_eq!(page.matching_count, 2500);
	assert!(page.history_gap);
	assert!(page.has_more);
	assert_eq!(page.entries[0].entry.level, Level::Error);
	let next = collector.page(page.next_cursor, 1, &filter);
	assert!(next.entries[0].cursor > page.next_cursor);
	let (path, count) = collector.export(&fixture.data(), &filter).unwrap();
	assert!(count > 0);
	let exported = engine_logger::read_jsonl(path, 512 * 1024).unwrap();
	assert_eq!(exported.len(), count);
	assert!(exported.iter().all(|e| e.level == Level::Error));
	assert_eq!(collector.page(0, usize::MAX, &filter).entries.len(), 500);
}
#[test]
fn pagination_preserves_filtered_counts_cursor_boundaries_and_unicode_search() {
	let fixture = Fixture::new();
	let mut collector = Collector::new(fixture.logs(), &fixture.data());
	assert_eq!(collector.page(0, 0, &LogFilter::default()).next_cursor, 0);
	for sequence in 1..=7 {
		let mut entry = record("game-123-1", sequence);
		if sequence % 2 == 0 {
			entry.message = "Scene ÄbC loaded".into();
			entry.level = Level::Error;
		}
		entry.correlation.operation_id = Some("load".into());
		collector.ingest(entry);
	}
	let filter = LogFilter {
		search: Some("ÄBC".into()),
		severity: Some(Level::Error),
		application: Some("game".into()),
		component: Some("game".into()),
		session_id: Some("game-123-1".into()),
		operation_id: Some("load".into()),
		from_ms: Some(102),
		to_ms: Some(106),
	};
	let first = collector.page(0, 0, &filter);
	assert_eq!(first.matching_count, 3);
	assert_eq!(first.entries.len(), 1);
	assert_eq!(first.next_cursor, 2);
	assert!(first.has_more);
	let tail = collector.page(first.next_cursor, 2, &filter);
	assert_eq!(tail.matching_count, 3);
	assert_eq!(
		tail.entries.iter().map(|e| e.cursor).collect::<Vec<_>>(),
		[4, 6]
	);
	assert!(!tail.has_more);
	assert_eq!(tail.next_cursor, 7);
	assert_eq!(tail.latest_cursor, 7);
	let end = collector.page(tail.next_cursor, 2, &filter);
	assert!(end.entries.is_empty());
	assert!(!end.has_more);
	assert_eq!(end.matching_count, 3);
	assert_eq!(end.next_cursor, 7);
	let no_matches = collector.page(
		0,
		2,
		&LogFilter {
			search: Some("not found".into()),
			..Default::default()
		},
	);
	assert!(no_matches.entries.is_empty());
	assert_eq!(no_matches.matching_count, 0);
	assert_eq!(no_matches.next_cursor, 7);
	let (path, count) = collector.export(&fixture.data(), &filter).unwrap();
	assert_eq!(count, 3);
	assert!(
		engine_logger::read_jsonl(path, 512 * 1024)
			.unwrap()
			.iter()
			.all(|e| filter.matches(e))
	);
}

#[test]
fn failed_log_checkpoint_retries_and_retains_restart_deduplication() {
	let fixture = Fixture::new();
	let path = fixture.data().join("logs.json");
	fs::create_dir_all(&path).unwrap();
	let mut collector = Collector::new(fixture.logs(), &fixture.data());
	collector.ingest(record("game-123-1", 1));
	collector.persist();
	assert!(collector.health.checkpoint_error.is_some());
	fs::remove_dir(&path).unwrap();
	collector.persist();
	assert!(collector.health.checkpoint_error.is_none());
	let mut restored = Collector::new(fixture.logs(), &fixture.data());
	assert!(!restored.ingest(record("game-123-1", 1)));
	assert!(restored.ingest(record("game-123-1", 2)));
	assert_eq!(restored.page(0, 2, &LogFilter::default()).entries.len(), 2);
}
