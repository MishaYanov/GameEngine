# Engine Admin

Separate Angular/Tauri development application. The client frontend in `apps/editor` does not depend on this application.

From `apps/admin-dashboard`:

```powershell
npm.cmd install
npm.cmd run desktop
```

Requires Rust 1.89+, Cargo, Node compatible with Angular 22, and the existing Windows Tauri prerequisites.
`npm.cmd start` serves frontend only at port 4301. Visit `http://127.0.0.1:4301/?preview` for explicitly labelled
read-only sample data; browser-only mode cannot manage processes or execute jobs.

```powershell
npm.cmd run build
npm.cmd test
npm.cmd run package
cargo test -p engine-admin --offline
cargo check -p engine-admin --features desktop --offline
```

Select a workspace and logger directory in Overview. Defaults use the current engine checkout and the shared logger directory. `ENGINE_WORKSPACE_DIR`, `ENGINE_LOG_DIR`, and `ENGINE_ADMIN_DATA_DIR` override defaults. Administration history lives separately from client logs.

Logs are incrementally collected from per-session JSONL. Retention is bounded to 5,000 records / 16 MiB; browser display retains 1,000. Pagination indicates lost history. Collection scans the newest 256 session directories and limits scan work. Incomplete trailing lines wait for completion. Malformed and oversized records have explicit counters. Pause freezes display while collection continues. Export writes retained matching JSONL into the admin data directory. Source navigation requires a file inside the configured workspace and a local VS Code installation.

Filtered pages count all retained matches in one scan and clone only the returned page, capped at 500 records. Log
checkpoint serialization borrows retained history. Component registry persistence writes only when its stored state
changes; a failed write stays dirty for retry and appears as a checkpoint error. Stale heartbeat status remains a
snapshot-time calculation, so an unchanged registry need not be rewritten every polling tick. JSON writes are buffered.
These implementation changes preserve the cursor/storage contracts; no measured throughput improvement is claimed.

## Vulkan resource monitor

Open **Metrics** to inspect persisted Vulkan resource snapshots from the configured logger directory. Run a debug
game/editor viewport, or set `ENGINE_METRICS=1` in its environment, with the application logger initialized. The
dashboard
reads `vulkan-metrics-*.json` without initializing Vulkan itself; browser preview uses labelled sample data.

The view refreshes every second. Select an application/device session, search kind/asset/handle/details, or switch to
recent releases. Inventory pages contain 100 rows. Cards show live and peak allocation bytes, object counts, memory
categories and latest frame samples. **STALE** means the last snapshot is over five seconds old; **CLOSED** preserves
the final recorded state. Neither status proves that a process is currently running.

The reader examines at most 64 recent session directories, returns at most 16 device snapshots and has a 16 MiB read
budget. Each snapshot is limited to 4 MiB, schema 1, 2,048 live details and 128 recent releases. Storage errors and
truncation appear explicitly. Aggregate totals can exceed the retained detail inventory. These values measure
[engine-owned instrumented resources](../../crates/renderer/README.md#resource-accounting), excluding driver memory,
presentation image backing allocations and CPU RAM; device-local and host-visible bytes can overlap.

The shared 16 MiB allowance charges bytes actually read, including malformed snapshots and partial read failures.
Cached scan metadata is only a scheduling hint; replacing/growing a file between scan and open cannot enlarge that
aggregate allowance. Opening each snapshot retains its own size/schema/detail checks.

## Process and job management

Component health uses explicit lifecycle events and heartbeats; errors alone do not imply component failure. Expected heartbeats become stale after 15 seconds. Start/stop/restart controls apply only to dashboard-owned processes. Windows job objects stop descendants and close them when the dashboard exits. Restored process history is interrupted and never adopts old PIDs.

Builds and Tests queue allowlisted Cargo packages with development or release profiles. One active job serializes shared Cargo output directories; at most 50 unfinished jobs and 200 historical runs are retained. Each run records workspace, Cargo version, timestamps, exit code, artifacts and an operation ID. Logs links filter diagnostics by that operation. Supported Cargo JSON compiler messages expose primary source spans. Test command status is aggregate; stable Rust test harness text is retained without fabricated structured counts. Raw output lines are bounded to 8 KiB and the output queue to 1,024 lines, with drop counters. Exceptionally large Cargo JSON messages may therefore remain raw output rather than parsed diagnostics.

Cancellation of queued jobs does not launch them. Active cancellation stops the owned tree. Restart marks unfinished jobs interrupted; they are not silently resumed. Finish/cancel jobs and stop applications before changing workspace. Cargo must be on PATH. GUI-native interaction and release installer testing remain manual checks.

The Board provides Backlog, In Progress, Blocked and Done. Create/edit/delete cards, drag between columns or use status selects and reorder buttons. Cards retain diagnostic session IDs and up to 100 job links. From Automation settings choose a package/profile, then build/test from a card or link an existing run. Suggestions describe the latest outcome; successful tests/builds never silently mark a task Done. Log details can create a diagnostic task. Limits are 1000 cards / 4 MiB; unreadable board storage is protected from automatic overwriting.

The scene JSON asset allowlist gap is now fixed: bounded validated asset JSON is included with native model/image
dependencies. Traversal is sorted and capped; unsafe/secret-like JSON and links are rejected rather than copying
arbitrary metadata. Generated compilation remains separate from installed/runtime acceptance.
See [packaging step22](../../editor-steps-21-35.md#step-22-json-aware-packaging).

Project Builder v1 creates an independent **native game workspace on Windows**. It does not generate the Angular editor/client host. Enter a name, source and new absolute destination outside the source workspace; the destination parent must already exist. Preview the allowlisted sources/assets, then generate. Offline validation uses cached Cargo dependencies by default; disable the checkbox to permit downloads. Rust and Vulkan SDK/glslc are required.

The builder copies game sources/assets and engine-core, logger, renderer, runtime and bridge into sibling staging,
writes a fresh root workspace, and queues validation. Only success publishes the destination with a Windows move that
cannot replace an existing path. Preview fingerprints detect changed files; template symlinks are rejected. Failure,
cancellation and interruption retain staging for inspection. No automatic recursive cleanup is performed. Project
history retains at most 50 runs / 32 MiB. Keep the generated workspace layout when running its game so
executable-relative asset paths remain valid. The manifest records copied inputs, revision where available, toolchain,
operation and relative output artifacts; local source modifications are included and labelled as a working tree
snapshot.

Desktop preview/generation reserve a token under the service lock, perform filesystem preparation on a blocking
worker without that lock, then reacquire it to finish and queue validation. Up to 16 previews may be preparing; only
one generation preparation/validation may be active. Tokens reject expired results; abandoning a task releases its
reservation. Workspace changes are blocked during preparation, and successful reconfiguration clears old previews.
Preparation failures retain staging where created; a terminal-open or preparation-complete result does not certify a
successful native game build. Metrics collection caches file metadata per scan instead of querying it repeatedly
during sorting; existing read budgets and last-snapshot semantics remain.

Verification includes a full error → card → failed build → successful rerun → generated project fixture, source-independent fixture compilation, publication collisions, source navigation after moving staging and the actual copied native game build. Existing logger tests cover redaction before persistence; exports preserve retained records, while raw process output is not automatically redacted. Add redaction at the emitting source before logging sensitive output.

```powershell
cargo test -p engine-admin --offline
cargo test -p engine-admin --offline real_native_template_validation -- --ignored --nocapture
cargo clippy -p engine-admin --all-targets --features desktop --offline -- -D warnings
```

The full native template smoke test is opt-in because it compiles Vulkan/game dependencies and retains generated temporary output for inspection. Native Tauri interaction, interactive game rendering and installer smoke checks remain manual. See [automation and logging design](../../../designs/automation-and-logging-prd.md) for implementation status and future Angular-client templates.

For step-by-step CLI, editor and dashboard instructions, see [the usage guide](../../usage.md).

Native template validation checks every project catalog scene and its scene-scoped behaviors, not only the
startup file. See [project lifecycle](../../project-lifecycle.md) for catalog and packaging contracts.
