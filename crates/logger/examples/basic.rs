use engine_logger::{Context, Level, ResultLogExt, SnapshotRequest};
fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let _logging = engine_logger::init_application("logger-example")?;
    engine_logger::info!(target: "runtime", "Engine initialized");
    engine_logger::warn!(target: "renderer", "Example validation warning");
    let result: Result<(), std::io::Error> = Err(std::io::Error::other("example missing asset"));
    let _ = result.log_error("load texture");
    let mut context = Context::default();
    context.correlation.operation_id = Some("example-operation".into());
    context.attributes.insert("duration_ms".into(), "12".into());
    engine_logger::event(
        Level::Info,
        "compiler",
        format_args!("Example compilation finished"),
        context,
    );
    let first_page = engine_logger::snapshot(SnapshotRequest {
        limit: 2,
        ..Default::default()
    });
    assert!(first_page.has_more);
    let next_page = engine_logger::snapshot(SnapshotRequest {
        session_id: first_page.session_id,
        after_sequence: first_page.next_cursor,
        limit: 200,
    });
    assert_eq!(next_page.entries.len(), 2);
    assert!(engine_logger::shutdown(std::time::Duration::from_secs(2)));
    assert!(
        engine_logger::snapshot(Default::default())
            .health
            .persistence_error
            .is_none()
    );
    Ok(())
}
