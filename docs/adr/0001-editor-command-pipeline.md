# ADR-0001: Typed editor commands, change messages, and project builds

Status: Accepted architectural direction; implementation is planned.

Date: 2026-10-26

Scope: Angular editor, Tauri host, bridge contracts, engine-core, runtime synchronization, and project compiler.

## Context

The editor needs one easy interface for submitting UI changes, recording their outcomes, updating the running engine,
and building the saved game project. Panels should express editing intent without knowing IPC names, revision handling,
filesystem rules, or renderer lifetimes.

There are two kinds of backend work:

- Runtime updates apply accepted edits to authored CPU state and synchronize affected runtime and GPU resources.
- Project builds validate saved inputs, compile the player, and install the project artifact at a chosen destination.

A property edit can require the first immediately. Compilation is an explicit operation with its own lifecycle.
Compiling the Rust engine host after changes to its source remains a development workflow outside this editor protocol.

### Existing implementation inspected

The inspection baseline is commit `a377b1f87dcc2972851c1518e4fd3cf1dc1e8951`, together with the staged, unstaged, and
untracked tree inspected on the date above. This ADR describes a target architecture, not completed implementation.

| Existing code                                       | Current behavior                                                                                                                                                                      |
|-----------------------------------------------------|---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `apps/editor/src/app/core/editor/editor.service.ts` | Owns Angular signals, an ordered document queue, revision checks, Tauri calls, and native subscriptions. Commands currently use the full-snapshot `editor_execute` endpoint.          |
| `apps/editor/src/app/core/editor/editor.models.ts`  | Handwritten TypeScript wire models and command builders.                                                                                                                              |
| `crates/bridge/src/editor.rs`                       | Re-exports engine-core commands, receipts, errors, and change types; defines viewport DTOs.                                                                                           |
| `crates/engine-core/src/engine.rs`                  | Validates commands and returns `CommandReceipt` with revisions, change cursor, affected scenes, and change categories.                                                                |
| `crates/engine-core/src/events.rs`                  | Keeps a pull-based journal bounded to 256 entries and 1 MiB of serialized event payload; slow consumers receive `resync_required`. It is not a durable audit log.                     |
| `apps/editor-host/src-tauri/src/commands/editor.rs` | Exposes `editor_execute_receipt`, `editor_changes`, snapshots, and project builds. Receipt execution emits `editor://changes-ready`. Many other endpoints still return string errors. |
| `apps/editor-host/src-tauri/src/viewport.rs`        | Uses scene revisions and journal changes to synchronize runtime/GPU state, with resynchronization when needed.                                                                        |
| `apps/editor-host/src-tauri/src/editor.rs`          | `prepare_build` requires clean saved state, no open transaction, and stopped Play; compares disk project/scenes with retained authoring state.                                        |
| `compiler/src/lib.rs`                               | Captures validated inputs and rechecks assets around compilation/copying. These checks do not create an atomic immutable filesystem snapshot.                                         |

The recent subject and graph UI stores are presentation models. Their records are not automatically engine entities,
components, or executable behavior. A feature adapter must map supported domain edits into explicit backend commands.

## Decision

Use a typed command pipeline with backend-owned authoring state, a bounded message recorder, and a separate build
service. Reuse existing commands, transactions, receipts, change journal, and compiler boundaries.

```text
Editor panel / inspector / graph adapter
  -> EditorService: dispatch intent, order mutations, expose signals
  -> EditorClient: typed request methods and subscriptions
  -> Tauri host: trusted window, native ownership, filesystem adapters
  -> Engine: validate, apply command, update undo history, publish changes
  -> receipt + change messages
       -> EditorService refreshes authoritative UI state
       -> runtime/viewport synchronizes affected resources

Explicit Build action
  -> BuildService -> EditorClient -> Tauri build coordinator
  -> validate saved project -> capture inputs -> compiler -> artifact result

Message recorder observes command and build lifecycles by correlation ID.
```

### Ownership and public interface

| Layer                           | Owns                                                                                                                                                                      |
|---------------------------------|---------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| Components and feature adapters | Form drafts, input bindings, and mapping UI actions to domain intent.                                                                                                     |
| `EditorService`                 | Command ordering, authoritative snapshot signals, revision/cursor tracking, transactions, and synchronization recovery.                                                   |
| `EditorClient`                  | Named, typed calls and subscription cleanup. This is the editor's boundary for Tauri APIs. A mock implementation supports browser previews and tests.                     |
| `BuildService`                  | Build requests, job state, progress presentation, and artifact results.                                                                                                   |
| Bridge public surface           | Serializable commands, queries, receipts, events, errors, and proposed message/build envelopes. Domain types may remain defined in engine-core and re-exported by bridge. |
| Tauri host                      | Trusted-window checks, workspace locking, native events, filesystem access, worker scheduling, and build coordination.                                                    |
| Engine and runtime              | Validation, accepted mutations, scene-aware undo/redo, simulation, and runtime/GPU synchronization.                                                                       |

Generate TypeScript wire contracts from the public Rust contract graph instead of maintaining a second schema by hand.
Keep UI-only view models and convenience command builders separate. Select a generator after proving Serde enum tags,
optional fields, nested types, and numeric serialization against representative fixtures. Generated types alone do not
validate runtime data; Rust still validates incoming commands.

Expose methods such as `execute(request)`, `snapshot()`, `changes(cursor)`, `openProject(path)`, and
`startBuild(request)`.
Keep transport command names and argument casing inside the client. Use discriminated unions for domain commands and
messages rather than an arbitrary topic/payload dictionary.

### Commands, queries, and messages

- A command asks for a mutation: rename, transform, spawn, delete, reparent, change project settings, undo, or redo.
- A query reads state: capabilities, snapshots, changes, assets, or job status.
- A message records an observed outcome: accepted/rejected command, changed backend state, or build progress/result.
- A notification wakes a consumer to fetch authoritative state. It is not the only copy of that state.

Retain `EditorCommandRequest` and `CommandReceipt` as the initial execution contract. Add correlation and recording
metadata through an explicitly versioned envelope; do not silently change strict existing request schemas.

The target envelope identifies protocol version, request ID, workspace/session generation, expected revision, target
scene where relevant, optional transaction ID, and typed payload. The host validates session and scene targeting before
mutation. Responses identify the originating request and workspace, plus authoritative revisions and cursor. Journal
messages keep their backend sequence and scene ID. Revisions, scene revisions, event sequence, and workspace generation
have different meanings and must remain separate.

Transport integers must have an explicit JavaScript-safe representation. Preserve compatibility initially; before
exporting additional `u64` identifiers, define checked safe-number limits or decimal-string encoding across both ends.
Never silently lose precision by casting to JavaScript `number` or emit JSON-incompatible `bigint` values.

### Recording UI changes

Record the lifecycle of submitted domain intent, rather than every DOM event or intermediate form keystroke:

1. `command_queued`: capture an immutable copy of the intent and its correlation ID.
2. `command_sent`: record the actual workspace, target scene, and revision selected when the queue dispatches it.
3. `command_accepted`: record the receipt, affected scenes, change categories, and authoritative revision.
4. `command_rejected`: record the structured error when the backend confirms rejection before commit.
5. `command_outcome_unknown`: record transport interruption when mutation may already have committed.
6. `synchronization_failed`: record failure to refresh after a known accepted command, without reclassifying the edit
   as rejected.

Use a bounded in-memory recorder with configurable entry and byte limits, an eviction counter, and explicit payload
truncation markers. Defaults for a first implementation are 256 records and 1 MiB of retained serialized payload.
Do not retain full scene snapshots or imported asset bytes in each record. Local timestamps aid diagnostics; backend
sequence and revision establish order. Recording does not affect authored dirty flags or engine undo history.

The recorder and bounded backend journal serve diagnostics and synchronization. Authored files remain the persisted
project source, and engine history remains the undo authority. Durable command replay, crash recovery, and event-sourced
project storage require a separate ADR. A truncated diagnostic record must never be offered as a replayable command.

Treat correlation as distinct from idempotency. Until host-side duplicate detection and outcome lookup exist, do not
automatically retry a mutation after a timeout or unknown outcome. Refresh authoritative state and surface the
unresolved
outcome. A later duplicate detector must reject reused IDs with different payloads and scope retained outcomes to the
workspace generation and documented retention bounds.

### Ordering, gestures, and synchronization

All document mutations use one bounded ordered queue. Read the current expected revision at dispatch time. Capture a
scene-scoped edit's intended scene when it is queued; reject it if that target is no longer valid instead of applying it
to whichever scene happens to be active. Explicit navigation selects the target for subsequent edits.

Use the existing begin/commit/cancel transaction commands to make a drag or other continuous gesture one undo step.
Coalesce only pending replaceable samples, such as an absolute transform for the same target and transaction. Preserve
relative deltas and discrete operations unless their composition is explicitly defined. Never coalesce across scene,
transaction, save, build, or undo boundaries. UI layout changes stay in presentation/preferences storage.

Subscribe before the initial synchronization and reconcile the authoritative change cursor afterward. Use
`editor://changes-ready` as a wakeup, pull `editor_changes(cursor)` in sequence, and coalesce concurrent drain requests.
Also drain after accepted commands and on reconnect/focus recovery so missed wakeups cannot permanently stale the UI.

The existing change records identify invalidation categories and entity keys; they do not contain complete replacement
values. The first migration therefore uses receipts plus authoritative snapshot queries when displayed state changes.
Later targeted queries or value-bearing deltas may reduce copying after a separate wire-contract extension.

Provide a consistent snapshot-and-cursor handshake before relying on incremental consumption. On `resync_required`, an
invalid cursor, or workspace replacement, obtain a new consistent baseline. Reset cursor ownership on replacement and
discard late results from the previous generation. Do not advance a cursor past unapplied data or treat a receipt cursor
as proof that all prior changes have been consumed. Reject or refresh stale revisions without silently replaying edits.

### Updating the running backend

Apply validated intent through the existing engine coordinator. Publish committed change facts and invalidate the
viewport after success. Continue using change categories to choose synchronization work:

| Change                                                                   | Runtime response                                                                  |
|--------------------------------------------------------------------------|-----------------------------------------------------------------------------------|
| Selection or other editor-only state                                     | Update presentation state; avoid document/resource reconstruction where possible. |
| Entity transform                                                         | Synchronize changed entities and dependent world transforms.                      |
| Camera, material, or light values                                        | Refresh affected runtime data and GPU uploads as required.                        |
| Spawn, deletion, reparenting, model assignment, or other topology change | Reconcile affected scene structure, bounds, collision/render data, and resources. |
| Scene replacement, Play transition, or lost journal history              | Resynchronize the required scene/runtime state.                                   |

The backend decides the required work from accepted changes. Panels do not issue renderer rebuild instructions.
A runtime synchronization failure after commit is reported separately from command rejection, with the authored state
retained and a recovery/resynchronization path. These policies extend the existing viewport behavior; selective work
and performance must be measured rather than assumed.

### Building the project

Build is an explicit host job outside the undoable scene-command union. Property edits update the running engine;
they do not automatically launch Cargo. The initial workflow is:

1. Reach a document-queue barrier so prior edits have completed; reject Build while a transaction or Play is active.
2. Require all retained scenes and project settings to be saved. Report dirty inputs to the UI without silently saving
   or discarding them. The current save endpoint saves the active scene; a future Save All action needs its own
   all-scene workflow and partial-failure reporting.
3. Under the workspace lock, validate the expected revision, saved manifest, all scene documents, asset references,
   and destination. Capture prepared inputs plus workspace generation and source revision.
4. Release the workspace lock before compilation and installation. Emit correlated build job messages and retain a
   queryable bounded status record so missed events can be recovered.
5. Return artifact path, source revision, asset counts/bytes, and structured diagnostics. New edits may continue after
   preparation; completion belongs to the captured inputs and must not mark newer edits as saved or built.

Use proposed job states `queued`, `validating`, `building`, `installing`, `succeeded`, and `failed`. Progress is
phase-based
unless measurable totals exist. Limit the initial coordinator to one active build, reject overlapping requests, and
bound retained job metadata and diagnostic output. Concurrent editing may save new disk inputs; existing compiler
stale-input guards should fail the older job rather than silently mix versions. An immutable build snapshot would be
a separate compiler improvement.

The existing `editor_build_project` endpoint already prepares inputs and runs compilation outside the workspace lock,
but returns only its final result. Job IDs, status queries, lifecycle messages, and correlation are planned additions.
Do not advertise cancellation until worker, child process, and partial output handling support it. Build failure leaves
the authored workspace intact; output cleanup and atomic installation guarantees must be specified and tested by the
build implementation. This ADR does not claim those guarantees already exist.

### Errors and compatibility

Use structured errors with stable code, message, correlation ID, and current revision when available. Reuse existing
`BackendError` codes for engine execution and define typed host/build errors for filesystem, trust, worker, and compiler
failures. Preserve causes for diagnostics without making the UI parse error strings.

Negotiate protocol version and capabilities before enabling commands. Add message/build capability flags during
migration. Keep existing snapshot endpoints available until their consumers migrate, and check generated-contract drift
in CI. The proposed envelope and lifecycle messages require deliberate protocol compatibility handling.

## Alternatives considered

| Alternative                                                    | Reason not selected                                                                                |
|----------------------------------------------------------------|----------------------------------------------------------------------------------------------------|
| Every component calls Tauri directly                           | Repeats wire names, revision logic, error handling, and cleanup across panels.                     |
| Generic event bus with arbitrary payloads                      | Hides command ownership and weakens compile-time contract checking.                                |
| Full backend rebuild and compilation after every property edit | Couples interactive latency to expensive work and duplicates existing incremental synchronization. |
| Durable event sourcing as the initial storage model            | Adds replay/versioning/recovery requirements beyond current authored files and bounded history.    |
| Reliable delivery inferred from native notifications           | Notifications can be missed; cursor queries and status lookup provide recovery.                    |

## Consequences

Components gain a small typed API, recorded command/build outcomes, consistent errors, and reactive authoritative state.
The existing engine remains the single mutation and undo authority. Runtime synchronization and project compilation
have distinct lifecycles while sharing contracts and correlation infrastructure.

The cost is an additional client boundary, generated-contract maintenance, consistent cursor/session handling,
and a host build coordinator. The initial snapshot-based refresh remains potentially expensive. Bounded recorders can
evict history and do not provide durable audit or exactly-once delivery.

## Implementation sequence and acceptance

1. Extract an injectable typed `EditorClient` while preserving current service behavior; prove a mock client can run
   frontend tests without Tauri. Generate wire types and add representative Serde/TypeScript contract fixtures.
2. Add versioned correlation/session metadata, structured error mapping, and a bounded lifecycle recorder. Verify
   caller mutation cannot alter queued intent, and cover rejection, unknown outcome, and recorder eviction.
3. Move execution to receipts and cursor synchronization with a consistent baseline. Cover ordered revisions,
   scene targeting, missed notifications, duplicate wakeups, cursor overflow/resync, workspace replacement, and late
   responses. Ensure accepted edits are never retried after a refresh failure.
4. Connect inspector/subject/graph adapters for supported engine concepts and transaction gestures. Verify dependent
   runtime updates, one-step gesture undo, cancel restoration, and no engine mutation for presentation-only edits.
5. Add typed build requests, bounded job status/progress, and a Build UI. Cover dirty/Play/transaction rejection,
   all-scene validation, duplicate builds, captured source revision, edits during builds, stale disk input, compiler
   failure, and final artifact metadata. Use native/GPU and installed-player smoke checks for their respective claims.

Documentation-site checks validate this ADR's publication only. They do not establish frontend, backend, native,
compiler, or installed-artifact acceptance for this planned work.

## Related documents

- [Workspace architecture](../architecture.md)
- [Project lifecycle and multi-scene authoring](../project-lifecycle.md)
- [Scene editing and transactions](../scene-editing.md)
- [Bridge contracts](../crates/bridge/README.md)
- [Editor frontend](../apps/editor/README.md)
- [Editor desktop host](../apps/editor-host/README.md)
- [Compiler and installer](../compiler/README.md)
- [Proposed error-handling design](../../designs/error-handling.md)
