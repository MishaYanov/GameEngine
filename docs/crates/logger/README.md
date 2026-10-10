# Shared Rust Logger

`engine-logger` collects diagnostics for engine libraries, client applications, and development tools. It has no Tauri or Angular dependency. Requires Rust 1.89 or newer.

## Application startup

Keep the guard alive until the application exits. Initialize before any engine services start. Libraries must not initialize or replace the process logger.

```rust
fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let _logging = engine_logger::init_application("game")?;
    engine_logger::info!(target: "runtime", "Engine initialized");
    Ok(())
}
```

Game, editor-host, compiler, and the renderer's Vulkan probe initialize their own process sessions. All Rust workspace members depend on this shared API. Separate processes communicate through persisted records, not through a shared Rust static. Dashboard aggregation belongs to a later implementation step.

Logging before initialization is a no-op. Invalid configuration is rejected before installation; duplicate initialization is an error. A competing global logger is an error and requires fixing application startup rather than retrying installation. Do not initialize `tauri-plugin-log` alongside this logger.

## Emitting diagnostics

```rust
use engine_logger::{Context, Level, ResultLogExt};

engine_logger::error!(target: "renderer", "Rendering failed: {}", "details");
engine_logger::warn!(target: "assets", "Using fallback texture");
engine_logger::info!("Application started");
engine_logger::debug!("Debug state");
engine_logger::trace!("Frame detail");

let result: Result<(), std::io::Error> = Err(std::io::Error::other("asset missing"));
let unchanged_result = result.log_error_for("assets", "load texture");
assert!(unchanged_result.is_err());

let mut context = Context::default();
context.correlation.build_id = Some("build-123".into());
context.attributes.insert("duration_ms".into(), "42".into());
engine_logger::event(Level::Info, "compiler", format_args!("Build completed"), context);
```

Use `log_error` or `log_error_for` once at the handling boundary. Ordinary `Result::Err` values do not log automatically. These helpers preserve the original result type and capture up to 32 causes supplied by `Error::source()`. The renderer and runtime expose their underlying error sources. Existing startup `expect` failures are captured by the panic hook; avoid additionally reporting them at every propagation layer.

Macros include file, line, and module metadata. The result and structured-event helpers capture the caller's file and line. Vulkan callback locations refer to the callback in Rust, not to the original GPU operation. Validation messages identify their Vulkan message category and preserve severity; validation remains enabled according to the renderer's existing debug-build policy.

## Record and consumer contracts

`LogEntry` contains schema version 1, application, component, session ID, sequence, UTC timestamp in milliseconds, process ID, thread name and ID, severity, message, optional source metadata, error causes, stack trace, correlation IDs, and string attributes. Severity serializes as `ERROR`, `WARNING`, `INFO`, `DEBUG`, or `TRACE`.

Error traces are captured at the reporting boundary; panic traces are captured in the panic hook. `capture_kind` identifies the difference. This collector does not recover the original error stack unless the producing error supplies such context separately. Trace symbol quality depends on the binary's symbols.

```rust
use engine_logger::SnapshotRequest;
let page = engine_logger::snapshot(SnapshotRequest {
    session_id: None,
    after_sequence: 0,
    limit: 100,
});
let next = engine_logger::snapshot(SnapshotRequest {
    session_id: page.session_id,
    after_sequence: page.next_cursor,
    limit: 100,
});
```

Advance only to `next_cursor`, the last returned record. `latest_sequence` may be ahead when there are more pages. `history_gap` indicates overwritten history. `session_reset` indicates a changed session and resets pagination. Pages contain at most 2,000 entries. A zero limit returns no entries and does not advance the cursor. `LoggerHealth` reports persistent storage errors, dropped persistence records, dropped memory records during unsafe panic/reentrant collection, and writer shutdown.

`read_jsonl(path, max_line_bytes)` reads complete records from a retained segment, bounds line allocation, rejects unsupported schema versions or malformed complete records, and ignores an incomplete trailing line. It is a convenience reader for bounded local segment files, not the future incremental dashboard collector.

## Configuration and storage

Use `Config::new` for custom settings; it defaults to memory and console logging. `init_application` also enables persistent storage.

| Setting | Default |
| --- | --- |
| Minimum severity | INFO |
| Memory history | 2,000 records |
| Message and individual context text | 8 KiB |
| Stack trace text | 32 KiB |
| Cause chain and attributes | Up to 32 each |
| Persistent writer queue | 1,024 records |
| Maximum serialized disk record | 512 KiB |
| Segment size | 4 MiB |
| Segments per session | 4 |
| Retained sessions per application | 10, with active sessions protected |
| Background flush interval | 100 ms when idle; errors request a flush during writing |
| Normal shutdown wait | Up to 2 seconds |

`ENGINE_LOG_DIR` overrides the storage root. Otherwise Windows uses `%LOCALAPPDATA%/GameEngine/logs`; other systems prefer `$XDG_STATE_HOME/GameEngine/logs` and then `$HOME/.local/state/GameEngine/logs`. A temporary directory is the fallback when neither is available. Each session owns a directory named `<application>-<pid>-<timestamp>` and numbered JSONL segments. Application names are restricted to ASCII letters, digits, underscores, and hyphens.

The background writer uses a bounded queue and buffered output. Queue overload drops the new disk record, increments `dropped_persistence`, and retains normal in-memory history. No disk writes occur on emitting threads. Ordered sequence assignment and queue submission share a short memory lock. Console output is synchronous; disable it for rendering performance measurements or production if appropriate.

Old inactive sessions are pruned at session startup. Session locks protect active applications from deletion. Active sessions can temporarily exceed the session retention count; subsequent starts prune them once inactive. Cleanup only removes recognized numeric segment files and logger-owned session directories. Unknown files cause cleanup to report an error rather than delete unrelated data.

Storage initialization failures keep the memory collector running. Runtime write failures disable that session's disk sink, preventing later records from appending after a partial JSON fragment. Health preserves the error and counts subsequent disk drops. Restart or start a new process session after correcting storage; automatic sink recovery is not implemented.

Set `Config.redact` to known literal secrets before startup. Redaction applies to record text, metadata, error causes, traces, correlation IDs, and attributes before they enter console output, memory, or persistence. It also handles secrets crossing text truncation boundaries. It does not automatically identify credentials; producers must avoid logging secrets and configure required replacements. The previous panic hook is preserved and may independently print unredacted panic output.

## Panic and shutdown behavior

The panic hook preserves the previous hook and makes a nonblocking attempt to collect a panic even when normal severity logging is disabled. If collection is already active on that thread or the history lock is busy, it increments a drop counter instead of recursively collecting or waiting. It does not wait for disk. Backtrace capture itself can take time and is controlled by `error_backtraces` for both errors and panics.

The guard drains the writer during normal exit and unwinding. `shutdown(timeout)` explicitly stops disk collection and returns whether the writer completed within the timeout. It is idempotent; logging after shutdown can still enter memory but is counted as dropped from persistence. `log::logger().flush()` requests a bounded drain without stopping the writer.

Flush refers to buffered output reaching the operating system, not an `fsync` durability guarantee. Forced termination, panic abort, native crashes, and an indefinitely stalled filesystem can lose pending records. A writer that exceeds shutdown's timeout may finish later; the caller is not forced to wait indefinitely.

## Verification and measurements

Run the commands below from the repository root.

```powershell
cargo test --workspace --all-targets --offline --locked
cargo clippy -p engine-logger --all-targets --offline --locked -- -D warnings
$env:ENGINE_LOG_DIR = Join-Path (Get-Location) 'target/logger-smoke'
cargo run -p engine-logger --example basic --offline --locked
cargo run -p compiler --offline --locked
cargo run -p renderer --example vulkan_probe --offline --locked
cargo run -p engine-logger --example measure --release --offline --locked
```

Tests cover serialization, filtering, pagination gaps and session resets, concurrent sequence ordering, text bounds and redaction, bounded cause extraction, panic capture and recursive panic safety, storage failure, rotation, session retention, queue overload, and bounded shutdown. Global initialization tests run in separate integration-test executables.

On October 3, 2026, a local Windows measurement with Rust 1.97.1, persistence enabled, console disabled, and an 8,192-record queue produced these results. These are single-run observations, not production targets:

| Workload | Debug build | Release build |
| --- | --- | --- |
| 5,000 ordinary records | 14.17 ms | 7.37 ms |
| One error with trace | 19.93 ms | 12.08 ms |
| 20,000 records across four burst threads | 87.60 ms | 61.09 ms |
| Total dropped disk records across the run | 8,391 | 0 |

Both runs reported zero dropped memory records and no storage errors. Burst overload remains possible; choose queue sizes and verbosity using measurements from the actual client workload. Metrics attributes and local storage are ready for later dashboard consumers; administrative APIs and dashboard UI are outside steps 1 through 5.
