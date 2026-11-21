---
name: logger-agent
description: Inspect code diffs and implement appropriate logging using the system logger, including regular and debug runtime modes and a dedicated error log file. Use for logging instrumentation or logging configuration work, not general code review or log analysis.
---

# Logger Agent

Implement logging around changed behavior and the directly affected execution paths. The goal is actionable diagnostics
with a quiet default, explicit debug detail, and durable error records. When asked only to create or revise this skill,
edit the skill; do not also instrument the application.

## Inspect the actual change

Read applicable AGENTS.md instructions and inspect the current logger, application startup, configuration, storage, and
relevant tests before editing. Respect existing local changes.

Use the user's requested diff or revision range. Otherwise inspect `git status --short`, `git diff`,
`git diff --cached`, and relevant untracked source files; staged and unstaged versions can differ. Read surrounding code
and callers to identify where operations start, fail, recover, and finish. If no relevant diff exists, report that and
address only explicitly requested logger infrastructure. Do not invent an audit baseline or sweep unrelated modules.

Reuse the shared logger and existing structured fields. Initialize once at each application entry point, keep its
lifetime guard alive through shutdown, and let libraries emit without initializing global state. Add infrastructure only
when the requested behavior is missing.

## Choose useful instrumentation

- INFO: meaningful lifecycle transitions and completed user operations, such as loading a scene or completing a build.
- WARN: recoverable degradation, fallback, or retry that matters to the operator.
- ERROR: failed operations or unrecoverable state with operation, component, safe identifiers, and the error cause chain
  where supported.
- DEBUG: diagnostic branch decisions, resource metadata, state transitions, and timings that explain the changed
  behavior.
- TRACE: exceptionally detailed, explicitly enabled diagnostics; keep disabled in ordinary debug mode.

Log an error once at the boundary that owns handling or reporting it. Preserve Result propagation, return values, and
recovery behavior. Do not turn expected cancellation or normal missing optional resources into errors. Avoid per-frame,
per-entity, or polling-loop INFO logs; aggregate, rate-limit, or use gated DEBUG/TRACE when useful. Do not add logs
mechanically to every function.

Use existing correlation IDs across asynchronous boundaries where available. Never dump secrets, credentials, full user
content, or entire request objects. Redact before any sink, including the error file. Guard costly diagnostic
computation before it happens when debug logging is disabled.

## Runtime modes

These are application logging modes, independent of the agent's workflow and compiler build profile.

| Mode                    | Console and ordinary log  | Dedicated error file             |
|-------------------------|---------------------------|----------------------------------|
| Regular (default)       | INFO, WARN, ERROR         | ERROR, including captured panics |
| Debug (explicit opt-in) | DEBUG plus regular levels | ERROR, including captured panics |

Reuse a documented selector if one exists. Otherwise implement a shared runtime selector, defaulting to regular;
`ENGINE_LOG_MODE=regular|debug` is the preferred convention for Game Engine. Reject invalid values clearly or report a
visible warning and use the documented default. Do not silently equate debug mode with a debug compiler build. Leave
TRACE as a separate explicit override only if supported or requested. Console enablement must not disable persistence.
Ensure selecting debug mode changes the effective logger filter, rather than merely adding an unused setting.

## Dedicated error file

Reuse an existing error sink if present; otherwise extend shared storage with a session-local `errors.jsonl` containing
only ERROR records. Duplicate errors into this sink while keeping them in the ordinary stream. Preserve the existing
record schema, timestamps, session/correlation IDs, redaction, and size bounds. A filtered viewer over the ordinary file
does not satisfy the dedicated-file requirement.

Keep file I/O off render and other latency-sensitive threads using the existing writer. Bound retention for the error
sink separately or within a clearly bounded shared storage budget. Include rotated error files in owned-file cleanup
without deleting active sessions or following unrelated paths. Isolate sessions/processes to avoid competing writes.
Flush errors promptly on the background writer and use bounded shutdown flushing. Treat panic persistence as best
effort; do not promise survival of forced process termination.

Make sink failures observable through existing health reporting or a non-recursive fallback without crashing normal
execution or recursively logging a logger error. Failure of one sink must not prevent attempts to write to another
healthy sink. Preserve overload/drop accounting rather than claiming guaranteed delivery through a bounded queue.

## Game Engine integration

When working in the Game Engine Rust workspace, recheck these current integration points rather than assuming their APIs
remain unchanged:

- `crates/logger` / package `engine-logger`: shared facade, Config, init_application, Context, ResultLogExt, and bounded
  asynchronous JSONL storage.
- Applications own initialization and LoggingGuard; runtime, renderer, and bridge use the shared facade.
- `ENGINE_LOG_DIR` chooses the log root; `ENGINE_LOG_CONSOLE` controls console output. Preserve these controls when
  adding mode selection.
- Logger tests currently live in `tests_logger.rs` and `tests_storage.rs`. Inspect current test discovery before adding
  cases.

Do not update engine documentation or documentation checkpoints unless explicitly requested. Follow repository
documentation maintenance rules when an update is requested.

## Verify and deliver

Run focused checks appropriate to the implementation. For shared logger changes, verify regular mode suppresses DEBUG,
debug mode admits DEBUG, errors reach both ordinary and dedicated files in both modes, non-errors stay out of the error
file, and console disablement leaves files working. Use isolated processes where global logger initialization prevents
repeated setup in one process.

For storage changes, exercise rotation/retention and sink failure behavior; verify redaction reaches every sink and
shutdown flushes pending records. Test the actual environment/configuration path, not only a directly constructed
filter. For Game Engine, start with `cargo test -p engine-logger` and check affected application packages as needed. Do
not run disruptive production workloads just to generate logs.

Review the final diff for accidental behavior changes, duplicate errors, sensitive fields, and hot-loop noise. Report
instrumented areas, how to select each mode, error file location/retention, checks performed, and any verification
limits. Distinguish implemented behavior from recommendations. Do not commit or publish unless requested.
