use engine_logger::{Config, LevelFilter, ResultLogExt, SnapshotRequest};
use std::{
    error::Error,
    fmt,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
#[derive(Debug)]
struct Outer(std::io::Error);
impl fmt::Display for Outer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "outer")
    }
}
impl Error for Outer {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.0)
    }
}
#[test]
fn process_wide_initialization_error_chain_and_panic_hook() {
    engine_logger::info!("before initialization");
    assert!(
        engine_logger::snapshot(Default::default())
            .entries
            .is_empty()
    );
    let mut invalid = Config::new("invalid");
    invalid.capacity = 0;
    assert!(engine_logger::init(invalid).is_err());
    let previous_calls = Arc::new(AtomicUsize::new(0));
    let calls = previous_calls.clone();
    std::panic::set_hook(Box::new(move |_| {
        calls.fetch_add(1, Ordering::Relaxed);
    }));
    let mut config = Config::new("integration");
    config.console = false;
    config.level = LevelFilter::Trace;
    let _guard = engine_logger::init(config).unwrap();
    assert!(engine_logger::init(Config::new("duplicate")).is_err());
    let error = Outer(std::io::Error::other("root cause"));
    let result: Result<(), Outer> = Err(error);
    let result_line = line!() + 1;
    assert!(result.log_error("load asset").is_err());
    engine_logger::warn!(target: "renderer", "validation warning");
    let panic_line = line!() + 1;
    assert!(std::panic::catch_unwind(|| panic!("fixture panic")).is_err());
    let snapshot = engine_logger::snapshot(SnapshotRequest::default());
    assert_eq!(snapshot.entries.len(), 3);
    let error = &snapshot.entries[0];
    assert_eq!(error.line, Some(result_line));
    assert_eq!(error.error.as_ref().unwrap().causes, ["root cause"]);
    assert_eq!(
        error.stack_trace.as_ref().unwrap().capture_kind,
        "reporting_boundary"
    );
    let panic = &snapshot.entries[2];
    assert_eq!(panic.line, Some(panic_line));
    assert_eq!(
        panic.stack_trace.as_ref().unwrap().capture_kind,
        "panic_hook"
    );
    assert!(!panic.stack_trace.as_ref().unwrap().text.is_empty());
    assert_eq!(previous_calls.load(Ordering::Relaxed), 1);

    #[derive(Debug)]
    struct CyclicError;
    impl fmt::Display for CyclicError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "cyclic cause")
        }
    }
    impl Error for CyclicError {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            Some(self)
        }
    }
    let cyclic: Result<(), CyclicError> = Err(CyclicError);
    assert!(cyclic.log_error_for("fixture", "bounded chain").is_err());
    let page = engine_logger::snapshot(Default::default());
    let entry = page.entries.last().unwrap();
    assert_eq!(entry.component, "fixture");
    assert_eq!(entry.error.as_ref().unwrap().causes.len(), 32);

    let okay: Result<(), std::io::Error> = Ok(());
    assert!(okay.log_error("success must not log").is_ok());
    assert_eq!(engine_logger::snapshot(Default::default()).entries.len(), 4);
}
