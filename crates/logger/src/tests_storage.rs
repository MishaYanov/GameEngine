use super::*;
use crate::{Config, Logger, SnapshotRequest};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "engine-logger-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn entry() -> LogEntry {
    let mut config = Config::new("fixture");
    config.console = false;
    config.error_backtraces = false;
    let logger = Logger::new(config).unwrap();
    crate::tests::emit(&logger, "persist me");
    logger
        .snapshot(SnapshotRequest::default())
        .entries
        .remove(0)
}

#[test]
fn reader_rejects_malformed_complete_records_and_future_schemas() {
    let fixture = Fixture::new();
    let path = fixture.0.join("corrupt.jsonl");
    fs::write(&path, b"{not json}\n").unwrap();
    assert!(read_jsonl(&path, 4096).is_err());
    let mut record = entry();
    record.schema_version = crate::SCHEMA_VERSION + 1;
    let mut bytes = serde_json::to_vec(&record).unwrap();
    bytes.push(b'\n');
    fs::write(&path, bytes).unwrap();
    assert!(
        read_jsonl(&path, 4096)
            .unwrap_err()
            .to_string()
            .contains("unsupported log schema")
    );
}
#[test]
fn rotation_retention_and_trailing_line_recovery() {
    let fixture = Fixture::new();
    let record = entry();
    let record_size = serde_json::to_vec(&record).unwrap().len() + 1;
    let mut config = StorageConfig::new(&fixture.0);
    config.max_record_bytes = record_size;
    config.max_file_bytes = record_size;
    config.max_files = 2;
    let health = Health::default();
    let mut sink = Sink::new(config, "fixture-123-1", &health).unwrap();
    for _ in 0..5 {
        sink.write(&record).unwrap();
    }
    sink.flush().unwrap();
    assert_eq!(sink.ordinary.as_ref().unwrap().paths.len(), 2);
    for path in &sink.ordinary.as_ref().unwrap().paths {
        assert!(fs::metadata(path).unwrap().len() <= record_size as u64);
        assert_eq!(read_jsonl(path, record_size).unwrap().len(), 1);
    }
    let path = sink
        .ordinary
        .as_ref()
        .unwrap()
        .paths
        .back()
        .unwrap()
        .clone();
    drop(sink);
    OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"{\"incomplete\":")
        .unwrap();
    assert_eq!(read_jsonl(&path, record_size).unwrap().len(), 1);
    assert!(read_jsonl(&path, 10).is_err());
}
#[test]
fn unavailable_storage_and_oversized_records_are_observable() {
    let fixture = Fixture::new();
    let file = fixture.0.join("not-a-directory");
    fs::write(&file, "occupied").unwrap();
    let health = Arc::new(Health::default());
    let writer = Writer::start(
        StorageConfig::new(file),
        "fixture-123-2".into(),
        health.clone(),
    )
    .unwrap();
    writer.enqueue(&entry());
    assert!(writer.flush(Duration::from_secs(2)));
    assert!(health.snapshot().persistence_error.is_some());
    assert_eq!(health.snapshot().dropped_persistence, 1);
    assert!(writer.shutdown(Duration::from_secs(2)));
    let mut config = StorageConfig::new(&fixture.0);
    config.max_record_bytes = 64;
    let mut sink = Sink::new(config, "fixture-123-3", &Health::default()).unwrap();
    assert!(sink.write(&entry()).is_err());
    assert!(sink.ordinary.is_none());
}
#[test]
fn full_queue_is_nonblocking_and_counts_drops() {
    let (sender, _receiver) = mpsc::sync_channel(1);
    let health = Arc::new(Health::default());
    let writer = Writer {
        sender: Mutex::new(Some(sender)),
        health: health.clone(),
        completed: Arc::new((Mutex::new(true), Condvar::new())),
    };
    writer.enqueue(&entry());
    writer.enqueue(&entry());
    assert_eq!(health.snapshot().dropped_persistence, 1);
    let start = Instant::now();
    assert!(!writer.flush(Duration::from_millis(5)));
    assert!(start.elapsed() < Duration::from_secs(1));
}
#[test]
fn shutdown_drains_and_stopped_writers_count_drops() {
    let fixture = Fixture::new();
    let health = Arc::new(Health::default());
    let writer = Writer::start(
        StorageConfig::new(&fixture.0),
        "fixture-123-4".into(),
        health.clone(),
    )
    .unwrap();
    for _ in 0..20 {
        writer.enqueue(&entry());
    }
    assert!(writer.shutdown(Duration::from_secs(2)));
    assert!(health.snapshot().writer_stopped);
    assert_eq!(
        read_jsonl(fixture.0.join("fixture-123-4/000000.jsonl"), 8192)
            .unwrap()
            .len(),
        20
    );
    writer.enqueue(&entry());
    assert_eq!(health.snapshot().dropped_persistence, 1);
}
#[test]
fn inactive_sessions_are_pruned_but_active_sessions_survive() {
    let fixture = Fixture::new();
    let mut config = StorageConfig::new(&fixture.0);
    config.max_sessions = 1;
    let health = Health::default();
    let mut first = Sink::new(config.clone(), "fixture-123-1", &health).unwrap();
    let mut error = entry();
    error.level = crate::Level::Error;
    first.write(&error).unwrap();
    let second = Sink::new(config.clone(), "fixture-123-2", &health).unwrap();
    assert!(fixture.0.join("fixture-123-1").exists());
    drop(first);
    let third = Sink::new(config, "fixture-123-3", &health).unwrap();
    assert!(!fixture.0.join("fixture-123-1").exists());
    assert!(fixture.0.join("fixture-123-2").exists());
    drop(second);
    drop(third);
}
#[test]
fn single_file_rotation_stays_bounded() {
    let fixture = Fixture::new();
    let record = entry();
    let size = serde_json::to_vec(&record).unwrap().len() + 1;
    let mut config = StorageConfig::new(&fixture.0);
    config.max_files = 1;
    config.max_file_bytes = size;
    config.max_record_bytes = size;
    let mut sink = Sink::new(config, "fixture-123-5", &Health::default()).unwrap();
    for _ in 0..4 {
        sink.write(&record).unwrap();
    }
    sink.flush().unwrap();
    assert_eq!(sink.ordinary.as_ref().unwrap().paths.len(), 1);
    assert_eq!(
        read_jsonl(sink.ordinary.as_ref().unwrap().paths.front().unwrap(), size)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn shutdown_timeout_is_bounded() {
    let (sender, _receiver) = mpsc::sync_channel(1);
    let completed = Arc::new((Mutex::new(false), Condvar::new()));
    let writer = Writer {
        sender: Mutex::new(Some(sender)),
        health: Arc::new(Health::default()),
        completed: completed.clone(),
    };
    let started = Instant::now();
    assert!(!writer.shutdown(Duration::from_millis(5)));
    assert!(started.elapsed() < Duration::from_secs(1));
    *completed.0.lock().unwrap() = true;
}
#[test]
fn writer_reports_rotation_failure_after_startup() {
    let fixture = Fixture::new();
    let record = entry();
    let size = serde_json::to_vec(&record).unwrap().len() + 1;
    let mut config = StorageConfig::new(&fixture.0);
    config.max_file_bytes = size;
    config.max_record_bytes = size;
    let health = Arc::new(Health::default());
    let writer = Writer::start(config, "fixture-123-6".into(), health.clone()).unwrap();
    fs::write(fixture.0.join("fixture-123-6/000001.jsonl"), "occupied").unwrap();
    writer.enqueue(&record);
    writer.enqueue(&record);
    assert!(writer.flush(Duration::from_secs(2)));
    assert!(health.snapshot().persistence_error.is_some());
    assert_eq!(health.snapshot().dropped_persistence, 1);
    assert!(writer.shutdown(Duration::from_secs(2)));
}

#[test]
fn error_stream_rotation_is_bounded_and_excludes_non_errors() {
    for max_files in [1, 2] {
        let fixture = Fixture::new();
        let mut record = entry();
        record.level = crate::Level::Error;
        let size = serde_json::to_vec(&record).unwrap().len() + 1;
        let mut config = StorageConfig::new(&fixture.0);
        config.max_file_bytes = size;
        config.max_record_bytes = size;
        config.max_files = max_files;
        let mut sink = Sink::new(config, "fixture-123-7", &Health::default()).unwrap();
        for _ in 0..5 {
            sink.write(&record).unwrap();
        }
        let errors = sink.errors.as_ref().unwrap();
        assert_eq!(errors.paths.len(), max_files);
        for path in &errors.paths {
            assert!(fs::metadata(path).unwrap().len() <= size as u64);
            assert_eq!(read_jsonl(path, size).unwrap(), [record.clone()]);
        }
        let paths = errors.paths.clone();
        record.level = crate::Level::Info;
        sink.write(&record).unwrap();
        assert_eq!(sink.errors.as_ref().unwrap().paths, paths);
        assert_eq!(sink.errors.as_ref().unwrap().bytes, size);
    }
}

#[test]
fn failed_stream_does_not_disable_the_other_stream() {
    for fail_errors in [false, true] {
        let fixture = Fixture::new();
        let mut record = entry();
        record.level = crate::Level::Error;
        let size = serde_json::to_vec(&record).unwrap().len() + 1;
        let mut config = StorageConfig::new(&fixture.0);
        config.max_file_bytes = size;
        config.max_record_bytes = size;
        let health = Arc::new(Health::default());
        let writer = Writer::start(config, "fixture-123-8".into(), health.clone()).unwrap();
        let directory = fixture.0.join("fixture-123-8");
        fs::write(directory.join(Segment::name(fail_errors, 1)), "occupied").unwrap();
        for _ in 0..3 {
            writer.enqueue(&record);
        }
        assert!(writer.shutdown(Duration::from_secs(2)));
        assert!(health.snapshot().persistence_error.is_some());
        assert_eq!(health.snapshot().dropped_persistence, 2);
        for index in 0..3 {
            assert_eq!(
                read_jsonl(directory.join(Segment::name(!fail_errors, index)), size).unwrap(),
                [record.clone()]
            );
        }
        assert!(!directory.join(Segment::name(fail_errors, 2)).exists());
    }
}

#[test]
fn redacted_errors_are_identical_in_both_files() {
    use log::Log;
    let fixture = Fixture::new();
    let mut config = Config::new("redacted-files");
    config.console = false;
    config.error_backtraces = false;
    config.redact = vec!["secret".into()];
    config.storage = Some(StorageConfig::new(&fixture.0));
    let logger = Logger::new(config).unwrap();
    logger.log(
        &log::Record::builder()
            .level(log::Level::Error)
            .target("secret")
            .args(format_args!("failure: secret"))
            .build(),
    );
    assert!(
        logger
            .writer
            .as_ref()
            .unwrap()
            .shutdown(Duration::from_secs(2))
    );
    let directory = fixture.0.join(&logger.session);
    let ordinary = fs::read_to_string(directory.join("000000.jsonl")).unwrap();
    let errors = fs::read_to_string(directory.join("errors.jsonl")).unwrap();
    assert_eq!(ordinary, errors);
    assert!(errors.contains("[REDACTED]"));
    assert!(!errors.contains("secret"));
}
