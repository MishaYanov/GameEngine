# Resolved ADR-0004: State lifetimes and prepared activation

**Decision status:** Accepted core, explicitly agreed on 2026-10-29.

**Implementation status:** Present in the inspected working tree for authored/editor/run/scene separation, typed
in-memory progress, ordinary prepared scene changes and prepared New Game. This is not completion of every proposal
in [original ADR-0004](../0004-authoring-play-and-editor-state.md).

**Verification status:** Project5 reports `cargo check --workspace --lib --bins --offline --locked` passing with
incremental-cache hard-link fallback warnings. Project5 ran no behavioral tests or native interactive checks. Newer
attributed test evidence and its snapshot limits are described below; documentation maintenance runs only publication
checks.

## Decision

Keep four lifetimes explicit:

| Owner             | Data and lifetime                                                                                                                                                                                |
|-------------------|--------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| Authoring session | Authored/saved scene and project documents, transactions and undo. Play does not write simulation values back into these documents.                                                              |
| Editor state      | Selection, tools, navigation and designated workspace preferences. Some state is backend-owned; editor state does not mean all state lives in Angular. Preferences are separate from game saves. |
| GameSession       | One running game's typed progress, factory, shared immutable project configuration and run identity. Ordinary scene changes preserve its latest progress.                                        |
| SimulationSession | The active scene's world, collisions, input, clock and module. Replace scene-local state at activation, beginning with neutral input.                                                            |

Start Play constructs a fresh run from validated authored state; Stop discards that run and shows authored state again.
Pause preserves world/progress while clearing gameplay input. Neither operation implicitly saves or applies runtime
changes back to authoring. Game progress is in-memory state, not a disk-save feature.

Prepare candidates before activation. Candidate construction may validate immutable configuration and allocate
scene-local resources, but must not tick gameplay, mutate installed progress or publish gameplay effects. Preparation
failure preserves the installed run. Before activation, the host completes CPU/GPU preparation and quiesces outgoing GPU
work; the backend checks candidate owner identity and expected scene generation. No gameplay executes on an uninstalled
candidate.

Ordinary activation preserves current progress and pause state, clears input and advances scene generation. New Game
prepares fresh factory-defined defaults and a fresh scene first. Successful commit resets progress, rotates run identity
and resets scene generation to zero; failed preparation or stale/foreign candidate rejection leaves the installed run
intact. Tokens from the previous identity cannot address the new run.

## Current implementation

- `crates/engine-core/src/editor/mod.rs`: `EditorSession` owns authored/saved state and optional `GameSession`;
  `render_document()` selects runtime data while authored snapshots remain authored. Start rejects open transactions.
  Document-cost caching is editor infrastructure, not game progress.
- `crates/engine-core/src/game_session.rs`: shared `Arc<ProjectData>`, `ProgressState`, `prepare_scene`,
  `prepare_new_game` and `commit_scene`; `PreparedScene` holds owner/generation and optional replacement progress.
  Ordinary preparation never calls `create_progress`; New Game does.
- `crates/runtime/src/scene_editor.rs`: native adapter exposes preparation/commit and typed progress access through the
  core session.
- `apps/game/src/main.rs`: Player F8 cycles scenes and F10 starts New Game through CPU/render candidate preparation, GPU
  idle, backend commit and installation of the corresponding rendered scene. This path remains synchronous.
  Development-mode restoration across runtime scene transitions is outside this feature.

Preparation is a trusted-native contract: immutable borrows cannot prevent factory side effects through interior
mutability or external I/O. There is no sandbox or universal resource rollback implied by the candidate type.

## Rationale and remaining proposals

Separate owners prevent saves, undo and UI preferences from accidentally capturing gameplay state. Prepared activation
retains the old playable scene until replacement is valid, while shared project data avoids copying configuration for
every candidate.
The [original rationale and alternatives](../0004-authoring-play-and-editor-state.md#accepted-core-decisions) remain
available.

Apply selected runtime changes, generalized runtime inspectors, preference reset/migration extensions, live authored
editing, disk game saves and asynchronous streaming remain open. Broader component/asset/prefab models require their own
decisions. See [resolved ADR-0008](0008-native-modules-and-typed-progress.md) for native state contracts
and [resolved ADR-0005](0005-serial-scheduling-and-rollback.md) for supported failure scope.

## Evidence and limits

The prior documentation checkpoint records a newer TEST/root report of 555 workspace tests passed, zero failed and two
ignored, including progress atomicity, tick rollback, scene retention, New Game reset and candidate invalidation
regressions. This is attributed evidence, not a new run by Project5 or documentation maintenance. At inspection, the
core progress/session/simulation/editor/world/project files, native adapter, game progress module and focused
progress/session tests match that checkpoint's hashes; `apps/game/src/main.rs` differs. Those results therefore do not
certify the current full Player path or staged-only snapshot.

Prior renderer smoke evidence covers its exercised resource lifecycle, not interactive F7/F8/F10 behavior. Visible
Play/Stop restoration, scene transition appearance, all device/failure cases and future proposal acceptance remain
unverified here. The checkpoint records documentation-site results separately. Hash matching compares documentation
inventories; it is not an independently pinned test execution.
