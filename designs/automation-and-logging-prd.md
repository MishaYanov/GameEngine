# Game Engine Automation and Logging PRD

Status: Steps 1 through 14 implemented; Step 15 core workflow verified, native GUI and installer smoke checks remaining  
Date: October 3, 2026  
Audience: Engine developers and maintainers

This document defines a shared logging foundation and two tools built around it: a development dashboard for engine health, builds, tests, and diagnostics, and a project builder that produces a separate client project. The first delivery is the Rust logger and its integration across the workspace. Dashboard and builder capabilities follow in separate phases.

The logger must be usable by both development tools and shipped client applications. Logging belongs in an independent Rust crate. The admin dashboard is a separate application with its own frontend, host, and build configuration. The existing Angular project is the client application and must not contain the admin dashboard. Administration, process execution, and project generation remain outside the client runtime.

## Problem and product goals

Engine diagnostics currently appear across terminal output and subsystem errors. Developers need a single place to identify which component failed, read useful context, navigate to its source location, and relate the failure to a build, test, or running application.

The project builder must turn a selected engine project into a separate, reproducible client workspace without requiring the development dashboard to ship with it.

The product should:

- Provide one consistent Rust logging API across engine libraries and executables.
- Automatically capture Rust panics and supported subsystem diagnostics.
- Make explicitly reported errors readable, with source locations, cause chains, and stack traces when available.
- Preserve diagnostics locally even when the dashboard is closed.
- Track component health separately from build, test, and process state.
- Let developers run and inspect builds and tests from a development dashboard.
- Create independent client projects with the runtime logging capability included.

## Existing workspace

The workspace currently contains these Cargo members:

| Component | Role | Logging responsibility |
| --- | --- | --- |
| `crates/runtime` | Engine orchestration | Report initialization and runtime boundary failures |
| `crates/renderer` | Rendering and Vulkan integration | Report renderer lifecycle and Vulkan validation diagnostics |
| `crates/bridge` | Shared data contracts | Share diagnostic contracts where needed without initializing logging |
| `compiler` | Compiler executable | Initialize its process logger and report compilation work |
| `apps/game` | Game executable | Initialize client logging and report application and rendering failures |
| `apps/editor-host/src-tauri` | Tauri editor host | Initialize editor logging and expose authorized diagnostic access |

The existing Angular project resides in `apps/editor`. Despite its directory name, the user has designated this project as the client application for this architecture. The admin dashboard must be created separately and must not add administrative screens or services to this Angular project or its client build. Existing Rust output uses `println!` and `eprintln!` in several paths. Cargo build-script directives are build protocol output and must remain intact.

## Scope and delivery order

| Phase | Deliverable | Completion condition |
| --- | --- | --- |
| 1 | Independent Rust logger | Structured records, bounded retention, persistence, panic capture, and tests |
| 2 | Workspace integration | Shared API available in every Rust member; executables initialize once; existing diagnostic paths migrated |
| 3 | Development dashboard | Logs, component health, application tracking, and build and test runs visible and controllable |
| 4 | Client project builder | Separate destination project generated, validated, built, and linked to its build diagnostics |

The initial implementation does not include a hosted logging service, remote telemetry, distributed tracing, a general CI platform, or a complete Unreal-style project format. These can be considered separately after local workflows work reliably.

## Architecture and ownership

### Shared logger

Proposed location: `crates/logger`, published within the workspace as `engine-logger`.

Use Rust for collection and persistence because the engine is primarily Rust and client applications must collect diagnostics without Angular. Use the standard `log` facade so dependencies that already emit `log` records can feed the same collector. Preserve an extension path for structured spans and tracing later.

Each executable initializes one process-wide static logger before starting engine services. Libraries use the shared API and never install competing global loggers. A Rust static is shared within one process; separate game, editor, and compiler processes each have their own instance. Cross-process aggregation therefore requires an explicit transport or ingestion layer.

### Development services

A separate admin dashboard application owns component registration, child-process execution, build and test history, and aggregation of process logs. It has its own frontend project, Tauri host, dependencies, configuration, and build commands. Angular may be used for this new frontend, but it must be independent of the existing client Angular project.

Proposed location: `apps/admin-dashboard`, containing the dashboard frontend and its own `src-tauri` host. This location is a proposed implementation choice; separation from the client application is a confirmed requirement. The dashboard can live in the same repository and share Rust diagnostic contracts without entering the client dependency graph or packaged output.

For the first local collector, processes write separate session JSONL files into a configured directory. The development service reads those files incrementally and identifies records by session and sequence. A future IPC stream may reduce latency, but file persistence must remain available for recovery.

### Client project builder

The builder shares diagnostic contracts and build orchestration with the development service. It owns templates, project generation, manifest creation, dependency resolution, and output validation. It does not become a dependency of the engine runtime.

### Dependency boundaries

- Engine libraries depend on the logger API.
- Editor and game executables initialize the logger.
- Development dashboard services consume logger records and execute authorized development jobs.
- The dashboard frontend and Tauri host are built and packaged separately from the existing client Angular project and host.
- The project builder generates a client workspace that includes runtime logging.
- Client applications have no dependency on the development dashboard or administrative process controls.

## Logger functional requirements

### Log levels and API

Support `ERROR`, `WARNING`, `INFO`, `DEBUG`, and `TRACE`, with a configurable minimum severity. The Rust facade may use its conventional `warn!` spelling while serialized records use `WARNING`.

Provide easy macros for ordinary diagnostics and a result helper that logs an error at a handling boundary without changing its return type. Producers can identify their subsystem through a target or component field. Normal library use must not require passing a logger through every constructor.

Initialization must reject duplicate installation clearly. Logging before initialization must have documented behavior; executable startup must install the logger before engine initialization. Filtered messages should avoid expensive formatting and backtrace work.

### Structured record contract

The shared contract must be serializable for Tauri and persistent storage. Fields marked optional must remain explicitly absent when unavailable rather than imply a fabricated source location.

| Field | Requirement |
| --- | --- |
| Schema version | Required for storage and transport compatibility |
| Session identifier | Required to distinguish restarts and concurrent runs |
| Sequence | Monotonically increasing within one process session |
| Timestamp | UTC Unix timestamp in milliseconds |
| Application and component | Identify the executable and emitting subsystem |
| Process and thread | Process ID and thread name or ID |
| Severity | One of the supported log levels |
| Message | Human-readable summary |
| Source | File, line, and module when the producer provides them |
| Error details | Optional error category and ordered cause chain |
| Stack trace | Optional captured trace with its capture location identified |
| Correlation | Optional project ID, operation ID, build ID, or test run ID |
| Attributes | Optional structured context and metric values |

Metrics such as duration, counts, and memory usage are additional attributes or metric events. A stack trace is diagnostic context, not a performance metric.

### Automatic error capture

Install a panic hook that records panic text, panic source location, thread, and stack trace before preserving the prior hook's behavior. Supported Vulkan validation messages must flow into the collector with severity mapped correctly.

Ordinary Rust `Result::Err` values do not automatically reach a global logger. Use explicit reporting at application and subsystem handling boundaries. Prefer logging once where an error is handled, rather than repeatedly logging the same propagated error at every layer.

Returned errors should preserve their original types. Where an error implements `source()`, capture its cause chain with a bound that prevents malformed cycles from hanging collection.

A backtrace captured while logging describes the reporting boundary. An originating trace is available only when the error type captured and preserved it earlier. The UI must distinguish these cases. Native access violations, forced termination, out-of-memory aborts, and GPU faults are not guaranteed to invoke a Rust panic hook; native crash reporting is separate future work.

### Persistence and retention

Maintain bounded in-memory history per process. Write structured JSONL into a configurable local directory with unique filenames per session, allowing simultaneous executable runs without collisions.

The completed logger must enforce configurable file size and retention limits. Development and client defaults may differ. File write failures must be visible through logger health without recursively logging through the same failed sink. Logging must not panic because a file is unavailable.

Expose retained sequence bounds so consumers can detect overwritten history. Consumers must tolerate an incomplete trailing JSONL line after an abrupt exit and avoid treating it as a complete record.

### Performance and concurrency

Logging must be thread-safe, preserve unique sequence ordering, and use bounded resources. Do not perform unbounded disk work or stack symbolization on the rendering hot path. Proposed delivery approach: a bounded background writer with an explicit overload policy and dropped-record counters; error and panic persistence should make a best effort to flush promptly.

The memory queue, file queue, maximum message size, and maximum backtrace size must all have limits. Flush on orderly application shutdown. Panic capture must avoid recursion and account for a panic occurring while another logger operation holds a lock.

Backtrace collection is configurable and reserved by default for errors and panics. Benchmark logging with persistence enabled before setting production performance targets; no latency claims are established yet.

## Development dashboard requirements

### Log explorer

Display live logs and retained session history. Filter by severity, application, component, session, time range, and operation. Search message text. Allow pausing the live display, inspecting full error details, copying a record, and exporting a filtered diagnostic bundle.

Selecting an error shows its summary, source location, cause chain, trace, and related operation. Source navigation works only when the local source file can be resolved. Clearly indicate missing source, missing traces, history gaps, collection failures, and dropped events.

### Component and application status

Display each registered component with independent health and activity states. Proposed health states: `UNKNOWN`, `HEALTHY`, `DEGRADED`, `FAILED`, and `STOPPED`. Proposed operation states: `QUEUED`, `RUNNING`, `SUCCEEDED`, `FAILED`, and `CANCELLED`.

Health must come from explicit lifecycle events, heartbeats, and health checks. An `ERROR` record alone does not prove a component is permanently failed. Likewise, an old successful build does not mean a running process is healthy.

Track launched application PID, session, start and end time, exit code, last heartbeat, and associated project. Distinguish an intentional stop from an unexpected exit. Support start, stop, and restart for processes owned by the development service.

### Build and test automation

Run configured build and test jobs with the selected project, profile, and target visible before execution. Capture stdout and stderr as operation diagnostics. For Cargo builds, prefer its machine-readable diagnostic output so compiler source spans can be shown accurately.

Record job command, working directory, configuration, toolchain, timestamps, duration, exit status, and produced artifacts. Display queued and running work and support cancellation of the owned process tree. Serialize jobs that would conflict over a destination or shared build output.

Show test totals and individual statuses when the selected runner provides a supported structured format. Otherwise display aggregate exit status and raw output without inventing per-test results. A cancelled or interrupted run must remain distinct from a passing run.

## Client project builder requirements

The user selects a source project, project name, destination, engine revision, target platform, and build profile. The first version targets the existing engine's supported local platform; additional platforms require explicit toolchain support and validation.

Generation must:

1. Validate source, template, toolchain, and destination inputs.
2. Show which project assets, configuration, and runtime dependencies will be included.
3. Generate into a temporary staging directory beside the chosen destination.
4. Include required engine sources or pinned dependencies, project assets, manifests, and runtime logger configuration.
5. Exclude development dashboard services, caches, `target`, local logs, credentials, and unrelated repository data.
6. Validate the generated manifest and build the generated project independently of the source checkout.
7. Finalize the destination only when generation and validation succeed.
8. Produce a manifest identifying the engine revision, template version, project inputs, toolchain, and output artifacts.

An existing destination must never be silently overwritten. Failed or cancelled generation must report its stage and preserve useful diagnostics without presenting partial output as a completed client project.

Copying a project is different from packaging a distributable executable. The first builder milestone generates a buildable client workspace. Packaging installers or platform bundles is a subsequent capability.

## Client logging and data handling

Clients retain local diagnostics using the same record format, with configurable lower verbosity and bounded disk use. They can create a diagnostic export without installing the development dashboard.

Logging is local by default. Remote transmission is outside the initial scope and requires an explicit product decision. Avoid logging credentials, tokens, or private asset contents. Provide redaction before persistence and export. Release builds may use source-relative paths and symbol packages rather than expose developer machine paths.

## Consumer API requirements

The first Tauri integration exposes paginated log snapshots and logger health. A consumer supplies its last received session and sequence; the response includes records, retained bounds, and persistence or overload errors. Advance the cursor only through records actually returned, not to the producer's latest sequence when pagination leaves records unread.

Later APIs expose component health, operation history, process controls, project generation, and exports. Administrative commands belong only to the development service and must not be exposed by the client runtime. Responses should use shared typed contracts that Angular can consume consistently.

## Acceptance criteria

### Logger and workspace integration

- Every Rust workspace member can use the shared logging API; executables initialize exactly once before engine startup.
- Concurrent emissions produce unique ordered sequences and valid structured records.
- Each severity survives serialization, filtering, persistence, and retrieval correctly.
- A caught test panic produces a record with the panic location and trace; the prior panic hook still runs.
- A reported result preserves the original error and includes available causes and its reporting location.
- Vulkan diagnostics preserve their severity and message context.
- Bounded history reports gaps; bounded persistence and overload behavior are tested.
- Invalid directories and simulated writer failures are reported without crashing normal application work.
- Separate executable sessions can be aggregated without sequence or file collisions.
- Existing user changes and Cargo build-script directives remain intact.

### Development dashboard

- The dashboard runs as a separate application with independent build commands and configuration; the existing Angular client contains no dashboard screens or administrative services.
- Building or packaging the client does not build, bundle, or depend on the admin dashboard.
- A developer can find a renderer error, inspect its details, and identify its process and operation.
- Component health, process state, build status, and test status are shown separately.
- Successful, failed, cancelled, and unexpectedly interrupted jobs remain distinguishable.
- History remains accessible after the emitting application exits.

### Project builder

- A generated client workspace builds from its destination without relying on the original checkout.
- It includes engine runtime logging and excludes administration dependencies and local secrets.
- Invalid input, an occupied destination, and cancellation produce clear recoverable outcomes.
- Each output has a manifest and associated build logs.

## Verification plan

Use unit and integration tests for filtering, serialization, bounded retention, error chains, concurrent writes, persistence failures, and shutdown behavior. Run global initialization and panic-hook tests in isolated test processes because those APIs affect process-wide state.

Validate the dashboard with representative records, missing locations, long traces, pagination gaps, session restarts, process failures, and unavailable collectors. Verify the builder by generating a small fixture into a fresh destination and building it after removing access to the source checkout.

Record performance measurements for normal logs, error traces, and sustained bursts before choosing release defaults.

## Implementation steps

Implement these steps in order. Each step should produce a working, reviewable change with its own completion check. Do not start the complete dashboard and builder at once. Preserve existing user changes throughout.

Steps 1 through 5 are implemented in `crates/logger` and integrated across the Rust workspace. The logger has no Angular or Tauri dependency. See `crates/logger/README.md` for API usage, defaults, operating limits, and measurements.

| Step | Implementation status | Verification |
| --- | --- | --- |
| 1 Contracts and crate | Implemented | Versioned records, shared contracts, JSON round-trip, and standalone example |
| 2 Process logger | Implemented | Filtering, bounded history, pagination, and concurrent sequence tests |
| 3 Errors and panics | Implemented | Cause-chain bounds, caller location, prior hook, lock contention, and reentrant panic tests |
| 4 Persistence | Implemented | Background queue, file rotation, session locks and retention, storage failures, shutdown bounds, and local measurements |
| 5 Rust integration | Implemented | Workspace checks and tests; compiler and live headless Vulkan probe smoke runs |
| 6 Separate dashboard | Implemented | Independent frontend production build and native host compilation |
| 7 Log explorer | Implemented | Incremental ingestion, persistence, filtering, export, and source path tests |
| 8 Health and processes | Implemented | Heartbeats, managed start/stop/restart, failure and interruption tests |
| 9 Build automation | Implemented | Serialized bounded queue, Cargo diagnostics/artifacts, cancellation and persisted history |
| 10 Test automation | Implemented | Cargo fixture failure and success outcomes; raw runner output, no invented per-test counts |
| 11 Manual Kanban | Implemented | CRUD, ordered moves, restart persistence and failed-save tests; drag plus keyboard controls |
| 12 Kanban automation | Implemented | Existing/new job links, preserved rerun history, suggestions without automatic task completion |
| 13 Template preview | Implemented | Native game template v1, file allowlist, bounded input, source fingerprints and destination validation |
| 14 Project generation | Implemented | Staged Cargo validation, cancellation, no-replace publication, source-independent fixture and actual native game builds |
| 15 Integrated verification | Core workflow verified | Error → card → failed build → successful rerun → generated client fixture; native GUI and installer verification remain manual |

Interactive game rendering and editor UI startup remain manual smoke checks. Their Rust targets compile and their entry points initialize logging. Steps 6–14 are implemented in the independent `apps/admin-dashboard` application. Native window interaction remains a manual smoke check. Disk persistence under overload is best effort, with explicit drop counters; native crashes and forced exits remain outside guaranteed capture.

### Step 1 Establish contracts and the logger crate

Register `crates/logger` in the Cargo workspace. Define the versioned log record, session identity, severity, error details, optional attributes, and paginated snapshot contract. Keep the crate independent of Angular, Tauri, and development tooling. Document initialization ownership and provide a minimal executable example.

Completion check: the crate builds independently, records round-trip through JSON, and pagination contracts specify how consumers detect gaps and advance cursors.

### Step 2 Implement the process wide logging API

Install one logger per executable process. Provide severity macros, source metadata, filtering, console output, and bounded in-memory history. Reject duplicate initialization and invalid configuration clearly. Capture application, component, session, process, thread, and sequence consistently.

Completion check: isolated tests verify initialization, severity filtering, bounded history, pagination, and unique ordered sequences under concurrent logging.

### Step 3 Implement error and panic reporting

Add the result reporting helper and bounded cause-chain extraction without changing error types. Install a panic hook that preserves the previous hook, captures the original panic location, and avoids recursion or deadlock. Label reporting traces separately from any originating trace. Make backtrace collection configurable.

Completion check: isolated tests verify a reported error, a caught panic, prior-hook execution, and panic behavior when the logger is busy. Ordinary propagated errors are logged once at their handling boundary.

### Step 4 Add bounded persistent storage

Write per-session JSONL using a bounded background queue. Add rotation, retention, record-size limits, dropped-event counters, persistence health, and orderly shutdown. Keep panic flushing best effort and bounded. Use configurable log directories suitable for development and installed clients.

Completion check: tests cover file rotation, unavailable storage, queue overload, shutdown, and recovery from an incomplete trailing line. Measure normal and burst logging with persistence enabled.

### Step 5 Integrate all Rust workspace members

Add the shared logging dependency to each Rust member. Initialize it in editor-host, game, and compiler entry points before subsystem startup. Replace diagnostic terminal prints and connect Vulkan validation severity. Add explicit error reporting at handling boundaries and preserve Cargo build-script output.

Completion check: available workspace checks pass, each executable emits identifiable session records, and the client runs with logging while no admin dashboard exists or is running. Report any environment-dependent Vulkan or Tauri checks that cannot run.

### Step 6 Create the separate admin dashboard shell

Create `apps/admin-dashboard` with an independent Angular frontend and Tauri host as the proposed default. Add its own dependencies, scripts, configuration, and development instructions. Start with navigation for Overview, Logs, Builds, Tests, Board, and Project Builder. Keep later screens visibly empty until their services exist.

Completion check: the dashboard launches through its own command, and client build commands and dependencies exclude its frontend and administrative host. Existing client UI is untouched.

### Step 7 Collect logs and build the log explorer

Implement incremental ingestion of per-session JSONL files with deduplication, bounded history, pagination, and collector health. Expose typed read APIs through the dashboard's Tauri host. Build log filtering, search, pause, error details, source navigation, and filtered export.

Completion check: concurrent game and compiler sessions appear correctly, restarting a consumer does not duplicate records, and missing files, truncated lines, history gaps, and long traces render clearly.

### Step 8 Track component health and application processes

Add a component registry, explicit health events, heartbeats, and process lifecycle records. Implement start, stop, and restart for processes launched by the dashboard. Show component health separately from process and operation state. Persist session history and detect unexpected exits.

Completion check: a managed fixture process can start, stop, restart, and fail; the dashboard shows accurate PID, timestamps, exit status, stale heartbeat, and intentional versus unexpected termination.

### Step 9 Add build automation

Implement a bounded job queue with typed job configuration and operation IDs. Run configured Cargo builds, parse supported compiler JSON diagnostics, collect stderr, and record toolchain, duration, exit status, and artifacts. Cancel owned process trees and serialize conflicting jobs.

Completion check: passing, failing, queued, and cancelled fixture builds have distinct states and associated logs; compiler source spans navigate correctly when files exist.

### Step 10 Add test automation

Reuse the job service for configured test commands. Persist aggregate results and supported structured per-test results. Show raw output when a runner cannot provide structured results. Keep failures, cancellation, and interruption distinct.

Completion check: passing and failing fixture tests appear accurately, cancellation never reports success, and unsupported output does not produce invented test counts.

### Step 11 Add the manual Kanban board

Add Backlog, In Progress, Blocked, and Done columns with persistent cards. Support creating, editing, deleting, and dragging cards. Card fields include title, description, priority, component, and optional links to diagnostic sessions and build or test runs. Store board data in the admin application's data directory.

Completion check: cards and ordering survive restart, drag operations persist correctly, and deleting a linked run does not make a card unreadable. Task status stays independent of component health.

### Step 12 Connect Kanban cards to automation

Allow cards to launch or link existing build and test jobs and inspect related diagnostics. Suggest status changes after jobs finish, while leaving the final task status under developer control. Avoid marking a feature Done solely because tests passed.

Completion check: a card can link a failed job, expose its error details, and later link a successful rerun without losing history or silently changing task completion.

### Step 13 Define and preview the client project template

Define a minimal versioned template, required runtime dependencies, asset selection rules, and generation manifest. Add project name, source, destination, profile, and target inputs. Validate paths and show a preview of included and excluded files before generation. Initially use the existing supported local platform.

Completion check: preview includes the runtime logger and excludes admin tooling, caches, local logs, credentials, and unrelated repository files. Occupied destinations and invalid inputs are rejected clearly.

### Step 14 Generate and validate an independent client project

Generate into a staging directory and build the result using the existing job service. Finalize a fresh destination only after validation succeeds. Store the engine revision, template version, toolchain, inputs, artifacts, and operation references in a manifest. Handle cancellation and failure without presenting partial output as complete.

Completion check: a generated fixture builds without access to the source checkout, contains no admin dependencies, and has accessible build diagnostics. An existing destination is never overwritten silently.

### Step 15 Verify the complete workflow and release defaults

Exercise the path from an engine error to the log explorer, linked Kanban task, build and test rerun, and generated client project. Finalize retention defaults using measurements. Verify redaction and diagnostic export. Document separate startup and build commands for client and administration tools.

Completion check: the end-to-end fixture passes, logger resource use remains bounded, client packaging excludes the dashboard, and remaining platform or native crash-reporting limitations are documented.

### Practical milestones

| Milestone | Steps | Usable result |
| --- | --- | --- |
| Shared logging foundation | 1 through 5 | Engine and client processes emit consistent local diagnostics |
| Diagnostics dashboard | 6 through 8 | Separate app displays logs, health, and managed processes |
| Development automation | 9 and 10 | Builds and tests can be launched and inspected |
| Work tracking | 11 and 12 | Persistent Kanban cards link to real development activity |
| Client project creation | 13 and 14 | Separate client workspaces can be generated and validated |
| Integrated verification | 15 | Complete workflow and documented operating defaults |

The core implementation is available. Remaining release checks are native desktop interaction, installer packaging, and interactive generated-game rendering. Build and verify each milestone before expanding to the next. Angular-originated log ingestion, remote telemetry, additional target platforms, and installer packaging remain separate follow-up work unless explicitly added to scope.

## Open product decisions

- Resolved: independent Angular/Tauri administration application in `apps/admin-dashboard`. The existing Angular client remains separate.
- Resolved for v1: vendor allowlisted native game, assets, renderer, runtime, bridge and logger sources. The generated root workspace excludes the Angular editor/host and administration dependencies. Angular client generation and installers remain future templates.
- Implemented defaults: shared logger storage documented in `crates/logger/README.md`; collector 5000 records / 16 MiB, browser 1000 records, job history 200, board 1000 cards / 4 MiB, project history 50 / 32 MiB.
- Whether Angular-originated logs enter the Rust collector in the first dashboard milestone.
- Supported target platforms, asset selection rules, and eventual distribution formats.
- Whether remote collection is ever required.

Remaining decisions concern additional templates, platforms, telemetry and distribution. They do not reopen the requirement to keep the admin dashboard separate from the client application.

## Implemented project template and verification limits

Template v1 targets the local Windows native game. Required paths are `apps/game` and the source/manifests of `crates/logger`, `crates/renderer`, `crates/runtime` and `crates/bridge`, including renderer shaders/build script. Rust source and known image/model asset formats are allowlisted; environment files, credentials, VCS data, cache directories, logs and administration/client-editor folders are omitted. Generation permits at most 10000 copied files / 512 MiB. Name/profile/local-target inputs are validated; destination parent must exist and destination must be a fresh absolute path outside the source. Symlinks are rejected in template paths.

Preview fingerprints bind generation to the displayed copied files. The builder creates a sibling staging folder, writes an independent Cargo workspace and readme, adjusts game asset lookup for the retained workspace layout, and queues a Cargo build. Offline validation defaults on and uses cached dependencies; developers may explicitly opt into dependency downloads. Rust and Vulkan SDK/glslc remain prerequisites.

Successful validation writes a manifest with template version, copied source fingerprints, engine revision when available, local platform, profile, Cargo version, operation ID, timestamps and relative artifacts. The manifest identifies the input as a copied working tree, which may include uncommitted source. Windows publication uses a move without replacement; an occupied destination wins even if created during validation. Failed/cancelled/interrupted staging is retained for inspection and is never advertised as a completed project. Unfinished jobs are not automatically resumed after restart. Publication on other operating systems is not implemented.

On October 3, 2026, verification passed for the error/card/build-rerun/generation fixture, copying then removing source access before a fixture build, cancellation and destination collisions, generated source navigation, and the actual native game template using cached dependencies. Generated Cargo metadata contains only game, logger, renderer, runtime and bridge as workspace members; all local manifests reside in the generated destination. Native rendering, Tauri window interaction and release installer smoke checks remain manual. The browser preview is clearly marked sample data and cannot execute native actions.
