# Error handling design and implementation plan

Status: Proposed; implementation steps below are not completed by this document.  
Date: 2026-10-24  
Audience: Engine Developer  
Scope: All ten Cargo workspace members, both Angular applications, and repository tooling.

## Decision

Adopt subsystem-owned typed Rust errors, preserve causes during propagation, and translate them into a stable public
error contract at application boundaries. Use `#[derive(Debug, thiserror::Error)]` to reduce manual error
implementations. Each application owns recovery, user presentation, and final diagnostic reporting.

Start with one editor Tauri command and its Angular consumer. Extend the pattern only after its failure behavior and
compatibility are verified. This document authorizes no claim that the migration is already implemented.

The design builds on
the [existing logging architecture](automation-and-logging-prd.md), [workspace architecture](../docs/architecture.md),
and [test workflow](../docs/testing.md). It does not introduce a second logger, a global error singleton, or a single
enum containing every engine failure.

## Inspected baseline

Inspection covered HEAD `706f87a867ecb0f8694a2a8aa8c20faa7d1671b8`, history since the documentation checkpoint
`a48ff6c5b84096b540f1401eff452c222e167493`, and staged, unstaged, and untracked changes. A commit identifies an
inspection baseline, not proof of complete documentation coverage.

Observed foundations:

- `crates/engine-core/src/engine.rs` exposes `BackendError`, `BackendErrorCode`, and `current_revision`. Existing codes
  include `conflict`, `invalid_command`, and `internal`.
- `crates/bridge/src/editor.rs` already re-exports backend errors and editor contracts. Reuse that ownership rather than
  introducing competing domain definitions.
- Renderer errors implement `std::error::Error` and preserve sources in several Vulkan wrappers.
- `crates/logger/src/lib.rs` provides cause chains, correlation fields, redaction, bounded records, and `ResultLogExt`.
  Its helper already specifies reporting once at a handling boundary.
- Editor command receipts expose typed backend failures, while many snapshot, filesystem, and workspace paths return
  `String`. Host boundary failures are sometimes mapped to `InvalidCommand`, obscuring their actual category.
- `apps/editor/src/app/core/editor/editor.service.ts` classifies revision conflicts using message text. Its command
  queue currently attempts snapshot recovery for failed mutations generally, not only conflicts.
- The current uncommitted Tauri project-opening work adds `ProjectOpenError` with manual `Display` and `Error`
  implementations, a filesystem cause, and logging inside `ProjectLocation::from_manifest`. The command then converts it
  to a string. Preserve that work and review reporting ownership when migrating it.
- Headless failures already use a JSON error object on stderr, currently with the broad `headless_run_failed` code.
  Compiler and administration services also have string-based error paths.
- Three untracked `*.rs~` backup files were inventoried as local backups, not active build inputs.

Source behavior above was inspected, not independently executed for this design. Existing unrelated source and test
edits remain owned by their current work.

## Error ownership and flow

```text
Subsystem detects failure
  -> typed error with structured context and source
  -> caller recovers, or adds operation context and propagates
  -> application boundary records the final failure once
  -> boundary returns a safe response or chooses an exit outcome
  -> frontend presents the message and applies explicit recovery policy
```

| Concern         | Owner                         | Rule                                                              |
|-----------------|-------------------------------|-------------------------------------------------------------------|
| Failure meaning | Subsystem crate               | Use an enum or struct with data needed by callers.                |
| Cause chain     | Every Rust propagation layer  | Retain `source`; avoid early `to_string()` conversion.            |
| IPC contract    | Bridge and host adapters      | Serialize an intentionally limited response.                      |
| Recovery        | Caller with operation context | Decide whether retry, refresh, cancellation, or shutdown is safe. |
| Diagnostics     | Application handling boundary | Emit one final failure record through the existing logger.        |
| Presentation    | Angular service and UI        | Branch on codes; use text for display only.                       |

Errors returned by shared libraries must not require Tauri or Angular dependencies. The logger must remain usable
without depending on engine-core or bridge. App-specific host failures belong in the host; scene and simulation failures
belong in the engine.

## Rust type conventions

Use `Result<T, SubsystemError>` when callers need to distinguish outcomes. Use `?` for propagation and `#[source]` for
causes. `#[from]` is appropriate for direct conversions; when an operation must add a path or other context, construct
the variant explicitly with `map_err`.

Illustrative proposed type, not an installed API:

```rust
#[derive(Debug, thiserror::Error)]
pub enum ProjectOpenError {
	#[error("The manifest must be named project.json")]
	InvalidManifestName,
	#[error("Cannot read project manifest {path}")]
	ReadManifest {
		path: std::path::PathBuf,
		#[source]
		source: std::io::Error,
	},
}
```

`#[derive(Debug)]` only generates debug formatting. `#[derive(thiserror::Error)]` generates standard error behavior; it
does not log, serialize errors, or choose recovery. `#[path = "..."]`, which appears in this repository's tests, selects
a module source file and is unrelated to error handling.

Keep established public types and variant names where feasible. Converting a manual implementation to `thiserror` must
preserve messages and source chains unless an intentional contract change is tested. Select a version compatible with
the repository's Rust minimum and lockfile; this is not a dependency-upgrade project.

Use `anyhow` selectively in application orchestration or CLI startup where the caller only needs a contextual report. Do
not replace typed domain APIs or IPC errors with `anyhow::Error`. Adding it is optional, not a prerequisite for the
pilot.

Expected input, filesystem, resource-budget, and revision failures return `Result`. Reserve panics for violated
programming invariants. A worker panic is a fault boundary: report it through the existing panic policy and do not
assume mutated state can safely continue.

## Public error contract

Propose a bridge-owned `CommandErrorDto` for migrated IPC endpoints. Keep internal source errors in Rust and convert at
the command boundary. This separates the wire format from internal enum layout.

| Field              | Meaning                                                                         |
|--------------------|---------------------------------------------------------------------------------|
| `code`             | Stable machine-readable snake_case code; never inferred from message text.      |
| `operation`        | Stable command or application operation name.                                   |
| `message`          | User-readable, bounded text suitable for this caller.                           |
| `current_revision` | Optional authoritative revision for conflict recovery.                          |
| `operation_id`     | Optional correlation ID when that operation is actually tracked in diagnostics. |

Example proposed response:

```json
{
  "code": "conflict",
  "operation": "editor_execute",
  "message": "The document changed. Refresh before editing.",
  "current_revision": 42
}
```

Use the existing `conflict` spelling rather than renaming it to `revision_conflict` from the earlier discussion. Reuse
existing backend classifications and add host codes only when consumers can use the distinction, initially `not_open`,
`unavailable`, `permission_denied`, and `worker_failed`. Further domain-specific codes need explicit mapping and tests.
Never label lock poisoning or worker failure as an invalid user command.

Do not expose arbitrary source-chain strings, stack traces, file contents, or absolute paths by default. The diagnostic
path may retain useful context subject to existing redaction and size limits. Serialization must not trigger logging or
backtrace capture.

The frontend validates rejected values as `unknown`; a TypeScript annotation alone does not validate IPC. Unknown codes
get a generic actionable message and retain diagnostic context. Optional fields must be checked before use.

### Compatibility

1. Establish the DTO and representative JSON contract fixtures.
2. Deploy a frontend decoder that accepts both the new object and legacy string responses. Keep any legacy string-based
   classification isolated and temporary.
3. Migrate one endpoint and record it in the migration inventory; do not advertise every endpoint as migrated.
4. Use capability negotiation for mixed endpoints. If the existing protocol cannot express this compatibly, bump its
   version and update the frontend's compatibility check in the same change.
5. Remove legacy decoding only after all supported host versions and callers have migrated. Retain a safe fallback for
   unknown future codes.

Do not serialize an internal error enum directly merely because Serde can derive it. Changes to that enum should not
silently alter the public API.

## Recovery and diagnostic policy

| Failure                        | Required behavior                                                                                         | Diagnostic treatment                                                   |
|--------------------------------|-----------------------------------------------------------------------------------------------------------|------------------------------------------------------------------------|
| Invalid user input             | Reject without mutation; identify the relevant field where possible.                                      | Expected rejection; no automatic error backtrace.                      |
| Revision conflict              | Fetch current in-memory state; preserve user intent for review; do not replay the edit automatically.     | Expected contention; bounded contextual record if useful.              |
| Missing or inaccessible file   | Keep the current valid workspace; allow selecting another path or explicit retry.                         | Final failed operation reported once.                                  |
| Cancellation                   | Stop or discard work according to lifecycle rules; distinguish it from failure.                           | Normal cancellation is not an error-level fault.                       |
| Resource limit                 | Return the limit category and preserve atomicity; let the user reduce work.                               | Bounded record, no repeated per-frame reporting.                       |
| Worker panic or poisoned state | Mark the affected service unavailable; recover only through a proven reset path.                          | Panic hook owns the panic record; avoid a duplicate full error report. |
| GPU failure                    | Distinguish expected surface lifecycle outcomes from device failure; preserve resource destruction order. | Report persistent faults once per failure episode.                     |
| Partial filesystem success     | Describe which effects completed; retain appropriate dirty state.                                         | Do not claim rollback or offer an unsafe automatic retry.              |

Each operation must document its mutation boundary. An `Err` after a successful mutation or file write does not imply
that nothing happened. Snapshot retrieval after an accepted command is one such case; callers must not replay the
command because its response failed.

Retries require both a transient condition and an operation safe to repeat. Specify a finite retry budget where retries
are justified. A generic `retryable: true` flag is insufficient authorization to repeat a mutating operation.

Reuse the logger's application, component, operation correlation, and source-chain facilities. Assign one reporting
owner per failure path before removing existing logs. Recovered lower-level attempts may emit bounded debug context.
Logger failures must not recursively log through the failing logger.

## Delivery sequence

Every step below is planned. Complete one vertical slice and its acceptance checks before broadening the migration.

| Stage | Projects                                   | Deliverable                                                                                 |
|-------|--------------------------------------------|---------------------------------------------------------------------------------------------|
| 1     | Bridge, editor host, editor frontend       | One `editor_execute` conflict travels as typed data from engine to UI.                      |
| 2     | Editor host and frontend                   | Project opening, save/reload, asset jobs, and viewport boundaries use the proven contract.  |
| 3     | Engine-core, runtime, renderer, logger     | Preserve domain causes and clarify recovery/reporting ownership behind those boundaries.    |
| 4     | Compiler, administration host and frontend | Installation, jobs, and process operations expose structured outcomes.                      |
| 5     | Game, headless, tooling, documentation     | Apply application boundary policy, finish compatibility cleanup, and verify complete flows. |

The bridge must keep stable conversions while internal crates migrate. Start with existing `BackendError`; a complete
engine-core rewrite is not a prerequisite for the first stage.

## Implementation steps by project

### 1. Editor desktop host: `apps/editor-host/src-tauri` (Cargo package `app`)

1. Inventory the error return types and reporting owners in `commands/editor.rs`, `editor.rs`, `project_location.rs`,
   and `viewport.rs`. Preserve the ongoing project-opening changes.
2. Add local typed host errors using `thiserror`; retain `BackendError` as a source when wrapping it. Distinguish trust
   checks, unopened workspace, lock faults, and worker failures.
3. Pilot `editor_execute`: keep the backend error through command execution and snapshot creation, then convert to
   `CommandErrorDto`. Avoid flattening the error through the existing string-returning workspace helper.
4. Preserve both operation failures and worker/join failures across asynchronous boundaries. Keep existing lock ordering
   and viewport invalidation behavior.
5. Once the pilot passes, migrate project opening and file operations in separate changes. Replace the manual
   `ProjectOpenError` implementation without losing its variants or filesystem cause. Move reporting to the selected
   owner in the same change to avoid duplicate records.
6. Migrate receipt/change APIs, import completions, and viewport errors with explicit compatibility handling. Keep
   asynchronous completion codes consistent with command codes.

Acceptance: stale revisions remain distinguishable despite changed message wording; rejected edits do not mutate the
document; accepted edits with failed response construction are not replayed; failed project opening retains the previous
workspace; partial save success preserves dirty state; worker panic reporting is not duplicated. Exercise the existing
workspace tests plus command response serialization and asynchronous failure fixtures.

### 2. Editor frontend: `apps/editor`

1. Add a runtime validator and normalization function for the proposed IPC response, retaining a temporary legacy-string
   path.
2. Extend `EditorClientError` to retain the stable code, operation, revision, and optional diagnostic correlation.
   Preserve the original rejected value as the cause.
3. Replace message matching for migrated endpoints. Keep existing queue serialization and authoritative revision
   handling.
4. Make recovery policy explicit: conflicts refresh in-memory state; other mutation failures may also require refresh
   when the outcome is uncertain. Never reload the disk file or automatically replay edits as generic recovery.
5. Present expected rejections separately from unavailable-service failures; preserve drafts and expose a safe next
   action.

Acceptance: structured conflicts, legacy responses, malformed objects, unknown codes, failed recovery, and cancellation
all leave queue/busy state usable. Verify no duplicate invocation, no accidental disk reload, and no lost draft. Run
editor service tests and production build.

### 3. Shared bridge: `crates/bridge`

1. Define `CommandErrorDto` and code serialization in a small transport-focused module. Keep core types and existing
   re-exports intact.
2. Specify optional-field behavior, bounded messages, and the compatibility/capability mechanism. Hosts supply operation
   names and perform domain-to-public mappings.
3. Add representative serialization fixtures consumed or mirrored by frontend decoder tests; check every publicly
   exposed category.
4. Keep host-only dependencies and runtime source chains out of the bridge DTO.

Acceptance: stable JSON shape and code spellings; no path/backtrace leakage from internal causes; existing editor
contract tests remain valid; no Tauri dependency added to core libraries.

### 4. CPU engine: `crates/engine-core`

1. Retain `BackendErrorCode` compatibility and audit which string-returning validation, history, simulation, registry,
   and job paths lose distinctions.
2. Introduce typed domain errors incrementally where callers need different recovery. Capture entity IDs, limits, or
   expected/current revisions as data rather than parsing strings.
3. Preserve causes inside domain errors and convert deliberately at the existing serializable backend facade. Adding
   nonserializable sources directly to `BackendError` would affect its current Clone/Eq/Serde contract and requires a
   separate compatibility decision.
4. Verify each rejection leaves revision, history, world, and change journal consistent; retain explicit rollback and
   resource-budget behavior.

Acceptance: conflict, invalid command, resource exhaustion, simulation rejection, and internal faults remain distinct;
failed atomic operations emit no success changes; existing CPU APIs and headless consumers remain compatible.

### 5. Runtime adapters: `crates/runtime`

1. Inventory `scene_io.rs`, scene editing adapters, asset management, image/glTF import, and metadata-job conversion
   points.
2. Add focused errors for scene I/O and import phases, retaining path/asset context and underlying I/O, parsing, or
   renderer causes internally.
3. Reuse engine-core errors where the runtime delegates CPU work; avoid duplicating core classifications.
4. Preserve atomic save behavior, asset adoption checks, cancellation, and generation/revision conflict handling.
   Specify partial side effects where multiple files are involved.

Acceptance: malformed data, missing files, stale imports, budget failures, and upload failures preserve the last usable
scene and release owned resources. Test injected failures at read, decode, write, rename, and adoption boundaries.

### 6. Renderer: `crates/renderer`

1. Inventory existing Vulkan error enums and manual source implementations. Convert to `thiserror` only where behavior
   can remain equivalent.
2. Preserve low-level Vulkan results and identify the failed operation without constructing large strings in hot paths.
3. Keep resize, minimized surfaces, and swapchain lifecycle outcomes distinct from persistent rendering/device faults.
4. Let the owning application choose shutdown or recreation. Preserve GPU resource lifetimes and report repeated
   failures once per episode.

Acceptance: source chains survive conversions; failure injection covers resource cleanup; existing renderer tests pass.
Run Vulkan probe and hidden render/resize smoke tests on supported hardware; record unavailable hardware as a limit
rather than treating CPU tests as GPU acceptance.

### 7. Logger: `crates/logger` (Cargo package `engine-logger`)

1. Audit `ResultLogExt`, source-chain traversal, redaction, record limits, and panic reporting against the chosen
   application boundaries.
2. Reuse correlation fields to connect a frontend operation to its diagnostic record. Populate an operation ID only when
   a real record/context exists.
3. Define severity for expected rejection, cancellation, recovered attempts, and persistent faults; avoid unconditional
   backtraces for expected outcomes.
4. Add only missing capabilities needed by the pilot. Keep error serialization pure and preserve recursive-failure
   protection.

Acceptance: one final record per failed operation, bounded and redacted source chains, no diagnostic recursion, no
duplicate panic record, and safe flushing during normal and failing shutdown. Use existing logger subprocess tests.

### 8. Compiler and installer: `compiler`

1. Separate errors for project validation, input reading, preparation, build process execution, and installation.
2. Retain failed phase and relevant child-process status internally; expose bounded output or a diagnostic reference
   rather than arbitrary full process output.
3. Keep library APIs typed. The CLI decides user text, exit outcome, and final logging; ensure the logging guard flushes
   before any explicit process exit.
4. Define cleanup and partial-install behavior before permitting retry. Preserve destination validation and
   prepared-input consistency.

Acceptance: invalid project, missing dependency, failed build, occupied destination, and interrupted installation are
distinguishable; no false success or deletion of unrelated destination content; existing packaging tests and
copied-client validation pass.

### 9. Administration host: `apps/admin-dashboard/src-tauri` (Cargo package `engine-admin`)

1. Inventory failures in `desktop.rs`, `builder.rs`, `jobs.rs`, `processes.rs`, `collector.rs`, and storage services.
2. Introduce domain-specific job/process/build/storage errors; map at the desktop command boundary using the shared wire
   conventions. Keep administration-only classifications in this application.
3. Preserve job IDs, build IDs, exit status, and cancellation state. Distinguish a failed IPC request from a
   successfully started job that later fails.
4. Adopt the common DTO when the bridge dependency remains lightweight; otherwise extract only the transport definitions
   into a small dependency-neutral contract crate as a reviewed follow-up, rather than copying divergent definitions.
5. Keep the default non-desktop library build viable and validate the optional `desktop` feature separately.

Acceptance: one job terminal state, correct cancellation, no duplicate child process on uncertain replies, bounded
diagnostic output, and stable frontend correlation. Verify service tests and the desktop feature compile.

### 10. Administration frontend: `apps/admin-dashboard`

1. Normalize IPC errors in `src/api.ts` using a validated contract consistent with the editor decoder. Keep app-specific
   presentation in the dashboard.
2. Update build, job, board, and process controls to branch on codes while retaining form inputs and selected items.
3. Handle asynchronous job failure separately from request failure, and link to existing diagnostics when an actual
   correlation is available.
4. Keep preview-mode fixtures representative of both successful and failed native responses.

Acceptance: malformed/legacy errors, unavailable desktop services, failed job starts, cancellations, and asynchronous
job failures have distinct UI behavior; actions re-enable correctly. Run API/model/board tests and production build.

### 11. Native game application: `apps/game`

1. Define application errors for launch configuration, project/scene loading, simulation, and renderer boundaries using
   the existing subsystem types.
2. Preserve the current Development/Player distinction and final logging ownership. Keep expected editor rejections
   separate from fatal runtime failures.
3. Retain failure latches to prevent a recurring frame fault from flooding logs; reset only after a verified recovery
   transition.
4. Specify shutdown or scene retention for each fatal path, including partial save/load operations and device faults.

Acceptance: launch failures exit clearly; failed edits or scene loads preserve valid state; repeated frame errors
produce bounded reports; GPU resources release in the required order. Run game tests and relevant renderer/scene I/O
smoke examples.

### 12. Headless application: `apps/headless` (Cargo package `engine-headless`)

1. Distinguish argument, project, input, simulation, budget, and output failures internally.
2. Preserve success JSON on stdout and failure JSON on stderr. Define stable machine codes and operation fields without
   adding console diagnostics to stdout.
3. Keep existing exit behavior unless an explicitly versioned CLI contract introduces more detailed codes. Migrate the
   broad `headless_run_failed` response with compatibility tests.
4. Keep deterministic successful runs unchanged and distinguish elapsed-time limits from invalid input or engine faults.

Acceptance: subprocess tests assert stream separation, valid JSON, correct exit outcome, and error category;
output-write failures do not report success; existing deterministic simulation checks pass.

### 13. Repository tools and CI

1. Preserve Python exception types and subprocess exit status in `launcher.py` and `test_all.py`; do not force Rust
   derives or IPC DTOs into local scripts.
2. Keep aggregate testing behavior that continues through independent suite failures and returns a failing summary when
   any suite fails.
3. Retain PowerShell failure exit behavior and native process ownership. Classify environment setup failures separately
   from product-test failures in reports.
4. When sandbox temporary-file renames fail, use a process-local temporary directory under `target` for the affected
   test invocation and record the workaround; do not change global environment settings.

Acceptance: launcher and aggregate-runner tests pass; unavailable tools and nonzero child exits remain failures; CI
artifacts identify the failing phase without exposing credentials.

### 14. Documentation site: `docs-site`, `docs`, and `designs`

1. Keep this proposal in Engine Developer and retain an empty Game App Developer space.
2. After each implemented slice, update its project guide with actual codes, recovery policy, compatibility status, and
   verification evidence.
3. Update `docs-state.json` after inspecting both Git history and the working tree; track unimplemented slices and
   unresolved verification limits explicitly.
4. Build into `docs-site/dist` and run the site tests and link checks. Keep documentation build failures explicit rather
   than publishing a partial site.

Acceptance: design and guides are searchable and linked; generated local links resolve; site tests pass; proposal status
is never presented as implemented runtime behavior.

## Validation and completion gates

For the first slice, require Rust contract/host tests, frontend decoder/service tests, and the editor production build.
Include a changed message with the same code, an unknown code, a malformed response, a preserved cause chain, an
unchanged document after rejection, and failure after mutation without automatic replay. Expand testing only as each
project's implementation changes.

Useful existing commands, run from the repository root unless stated otherwise:

```powershell
cargo test -p bridge -p app --all-targets --offline --locked
cargo check -p engine-admin --features desktop --all-targets --offline --locked
npm.cmd test --prefix apps/editor -- --watch=false
npm.cmd run build --prefix apps/editor
npm.cmd test --prefix apps/admin-dashboard
npm.cmd run build --prefix apps/admin-dashboard
python test_all.py
npm.cmd test --prefix docs-site
```

For changes to graphics or packaging behavior, run the relevant explicit native checks from the test workflow on a
suitable machine. The standard suite's ignored native tests are not evidence of GPU or installed-client acceptance.

A slice is complete when its public contract is tested, callers retain needed recovery data, duplicate reporting is
removed, mutation behavior is verified, and its compatibility inventory is updated. Project-wide completion additionally
requires migrating every supported boundary and removing temporary message-based classification.

## Research and rationale

The architecture above is a project-specific recommendation informed by these primary sources; the sources do not
establish that every large project uses one identical design.

- [Rust: recoverable errors and propagation](https://doc.rust-lang.org/book/ch09-02-recoverable-errors-with-result.html):
  use `Result` and propagate failures to callers that can decide how to respond.
- [Rust: deciding when to panic](https://doc.rust-lang.org/book/ch09-03-to-panic-or-not-to-panic.html): distinguish
  expected failures from violated invariants.
- [thiserror](https://docs.rs/thiserror/latest/thiserror/): derive standard error implementations, sources, and
  conversions while retaining ordinary public Rust error types.
- [anyhow](https://docs.rs/anyhow/latest/anyhow/): application-oriented error reports and contextual propagation; an
  optional complement to typed errors.
- [Tauri error handling](https://v2.tauri.app/develop/calling-rust/#error-handling): command errors must serialize, and
  structured error kinds can be exposed to TypeScript.
- [Rust error design: recovery information](https://nrc.github.io/error-docs/error-design/error-handling.html): preserve
  structured information for recovery and choose where recovery belongs.
- [OpenTelemetry: recording errors](https://opentelemetry.io/docs/specs/semconv/general/recording-errors/): avoid
  recording the same exception repeatedly and distinguish handled outcomes from failed operations. This informs logging
  policy; adopting OpenTelemetry is not required.

These sources were consulted during the preceding design discussion on 2026-10-24. This documentation change does not
execute or validate the proposed Rust/TypeScript examples or implementation steps.
