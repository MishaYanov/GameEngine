use super::*;
use log::Log;
fn collector(capacity: usize) -> Logger {
    let mut config = Config::new("test");
    config.capacity = capacity;
    config.console = false;
    config.error_backtraces = false;
    Logger::new(config).unwrap()
}
pub(super) fn emit(logger: &Logger, message: &str) {
    logger.log(
        &log::Record::builder()
            .level(log::Level::Info)
            .target("renderer")
            .args(format_args!("{message}"))
            .file(Some("fixture.rs"))
            .line(Some(42))
            .build(),
    );
}
#[test]
fn serialization_filtering_and_pagination() {
    let logger = collector(3);
    logger.log(
        &log::Record::builder()
            .level(log::Level::Debug)
            .args(format_args!("filtered"))
            .build(),
    );
    for i in 0..5 {
        emit(&logger, &format!("entry {i}"));
    }
    let page = logger.snapshot(SnapshotRequest {
        limit: 1,
        ..Default::default()
    });
    assert!(page.history_gap);
    assert!(page.has_more);
    assert_eq!(page.next_cursor, 3);
    assert_eq!(page.latest_sequence, 5);
    let entry = &page.entries[0];
    assert_eq!(entry.file.as_deref(), Some("fixture.rs"));
    assert_eq!(entry.line, Some(42));
    assert_eq!(
        *entry,
        serde_json::from_str::<LogEntry>(&serde_json::to_string(entry).unwrap()).unwrap()
    );
    let next = logger.snapshot(SnapshotRequest {
        session_id: page.session_id,
        after_sequence: page.next_cursor,
        limit: 5,
    });
    assert_eq!(
        next.entries.iter().map(|e| e.sequence).collect::<Vec<_>>(),
        [4, 5]
    );
    assert!(!next.has_more);
    let reset = logger.snapshot(SnapshotRequest {
        session_id: Some("old-session".into()),
        after_sequence: 500,
        limit: 1,
    });
    assert!(reset.session_reset);
    assert_eq!(reset.next_cursor, 3);
    for level in [
        Level::Error,
        Level::Warning,
        Level::Info,
        Level::Debug,
        Level::Trace,
    ] {
        assert_eq!(
            level,
            serde_json::from_str::<Level>(&serde_json::to_string(&level).unwrap()).unwrap()
        );
    }
    assert_eq!(
        serde_json::to_string(&Level::Warning).unwrap(),
        "\"WARNING\""
    );
}
#[test]
fn concurrent_sequences_are_unique_and_ordered() {
    let logger = Arc::new(collector(1000));
    std::thread::scope(|scope| {
        for _ in 0..8 {
            let logger = logger.clone();
            scope.spawn(move || {
                for _ in 0..100 {
                    emit(&logger, "parallel");
                }
            });
        }
    });
    let snapshot = logger.snapshot(SnapshotRequest {
        limit: 1000,
        ..Default::default()
    });
    assert_eq!(snapshot.entries.len(), 800);
    assert_eq!(
        snapshot
            .entries
            .iter()
            .map(|e| e.sequence)
            .collect::<Vec<_>>(),
        (1..=800).collect::<Vec<_>>()
    );
}
#[test]
fn bounds_context_and_redaction() {
    let mut config = Config::new("redaction");
    config.console = false;
    config.error_backtraces = false;
    config.max_message_bytes = 32;
    config.redact = vec!["secret".into()];
    let logger = Logger::new(config).unwrap();
    let mut context = Context::default();
    context.correlation.build_id = Some("secret".into());
    context.attributes.insert("token".into(), "secret".into());
    logger.emit(Event {
        level: Level::Info,
        component: "secret",
        message: format_args!("secret {}", "é".repeat(100)),
        file: Some("secret"),
        line: None,
        module: None,
        error: None,
        context,
        panic: false,
    });
    let entry = logger.snapshot(Default::default()).entries.remove(0);
    assert!(entry.message.len() <= 32);
    assert!(!serde_json::to_string(&entry).unwrap().contains("secret"));
    assert_eq!(entry.correlation.build_id.as_deref(), Some("[REDACTED]"));
}
#[test]
fn panic_does_not_wait_for_history_lock() {
    let logger = collector(2);
    let _busy = logger.state.lock().unwrap();
    logger.emit(Event {
        level: Level::Error,
        component: "panic",
        message: format_args!("busy"),
        file: None,
        line: None,
        module: None,
        error: None,
        context: Context::default(),
        panic: true,
    });
    assert_eq!(logger.health.snapshot().dropped_memory, 1);
}
#[test]
fn invalid_config_and_sessions() {
    let mut config = Config::new("../../escape");
    assert!(Logger::new(config.clone()).is_err());
    config.application = "valid".into();
    config.capacity = 0;
    assert!(Logger::new(config).is_err());
    assert_ne!(collector(1).session, collector(1).session);
}

#[test]
fn redaction_handles_truncation_and_does_not_rewrite_replacements() {
    let mut config = Config::new("redaction-boundary");
    config.console = false;
    config.error_backtraces = false;
    config.max_message_bytes = 12;
    config.redact = vec!["password".into(), "[".into()];
    let logger = Logger::new(config).unwrap();
    emit(&logger, "123456789password");
    let message = logger
        .snapshot(Default::default())
        .entries
        .remove(0)
        .message;
    assert_eq!(message, "123456789[RE");
}
