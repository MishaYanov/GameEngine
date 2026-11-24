# Resolved ADR-0005: Serial scheduling, authority and rollback

**Decision status:** Accepted core, explicitly agreed on 2026-10-29.

**Implementation status:** A serial bounded action pipeline with aggregate world/progress failure handling is present.
Expanded component lifecycle, animation/root motion, structural mutation and parallel scheduling
in [original ADR-0005](../0005-runtime-lifecycle-and-scheduling.md) remain proposals.

**Verification status:** Project5 compile check passed with cache hard-link fallback warnings; it ran no behavioral
tests or native interactive checks. The prior attributed 555-test workspace result includes progress/world rollback
regressions against a recorded snapshot, not exhaustive acceptance of this decision or the current full working tree.
Documentation maintenance does not run Rust/application tests.

## Decision

Start serially: sample supplied input, plan bounded actions from read-only runtime context, resolve movement through
engine-owned world/collision policy, commit the supported state, then expose successful results. Modules request motion;
they do not receive mutable world access. One declared authority must resolve each motion domain. The current
module/action seam is the implementation base, not an animation/physics phase framework or a scheduler driven by
inspector order.

The atomic boundary is the complete `SimulationSession::tick(elapsed)` call, which may execute several fixed catch-up
steps. Later steps may see earlier candidate progress within that call. Covered failure discards all candidate progress
and restores touched transforms across the aggregate batch; it does not commit an earlier reward while undoing a later
covered world action. Successful completion installs progress and advances the simulation clock/tick before returning
results. A progress-only change has its own revision/result and does not invent entity-transform changes.

Between ticks, `GameSession::apply_progress_actions` uses the same domain validation as its own atomic batch.
Publishable gameplay effects must follow successful commit. The current implementation supplies outcomes/read views;
this decision does not assert a completed general gameplay event-delivery system.

## Supported failure scope

| Covered state                                                                                                          | Outside the guarantee                                                                                                                                                                            |
|------------------------------------------------------------------------------------------------------------------------|--------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| Candidate typed progress, supported Move/Rotate/SetTransform changes, clock/tick acceptance and returned batch changes | Arbitrary native module interior state, aliased progress storage, topology edits, filesystem/network effects, already-emitted notifications, panic/process failure or unbounded native execution |

`GameProgress::clone` must create an independent candidate; validate/apply must have no external side effects. A
module's `&self` method can still mutate interior state, so the backend cannot automatically roll it back. Trusted
native implementations must honor these constraints. No sandbox or arbitrary native-state transaction is claimed.

## Current pipeline and budgets

`crates/engine-core/src/simulation_session.rs` retains `ChangeAccumulator`, a reusable action buffer, touched-transform
restoration and a candidate clock. `ProgressTransaction` clones progress lazily on the first action, validates typed
actions and commits only after all covered actions succeed. Rejection on the covered action path reconstructs the
restored world/collision state, pauses simulation and clears input/actions. Invalid elapsed input and preflight errors
can return before that action-failure path; do not describe every error as pausing the run.

- Fixed stepping defaults to 60 Hz/eight catch-up steps and accepts at most 0.25 seconds per advance, reporting
  discarded time.
- World and progress actions share the 4,096 aggregate attempted-action ceiling; blocked motion still consumes work.
  Progress has an additional 64-action transaction ceiling across catch-up steps.
- Collision work has a one-million aggregate budget. Matrix updates use a one-million threshold; one action can
  overshoot by at most the supported scene size before rollback.
- Progress has a 64 KiB retained logical size cap and a 4,096-byte per-action cap, including declared owned heap
  storage. Producer accounting is trusted, not total process/driver memory measurement.

`Arc<ProjectData>`, engine-prepared resolved behavior configuration, cached world/document costs and `ChangeAccumulator`
preserve the optimized ownership path. Normal updates do not require a full project/world clone; the cold rejection path
may rebuild restored state. No measured speedup or allocation-free claim follows.

## Rationale, evidence and open work

Explicit serial ownership makes ordering and failure reviewable before introducing additional systems.
The [original research, alternatives and expanded phases](../0005-runtime-lifecycle-and-scheduling.md#research-rationale)
remain background, not adopted ECS/physics dependencies.

The prior TEST/root checkpoint reports 555 passed/zero failed/two ignored and targeted progress/world rollback
regressions. Focused core files/tests match its recorded hashes at this inspection; the Player main file has changed,
and the result is not a new whole-tree test run. Neither compile success nor site publication proves frame-partition
determinism, every rejection path, native interaction or all devices.

Component enable/disable/destroy callbacks, spawn/despawn/reparent actions, animation phases/root-motion resolution, a
general post-commit event API, parallel scheduling and new physics integration remain open. Continue through the
existing pipeline; do not add empty phases or advertise unsupported rollback. Hash matching compares documentation
inventories; it is not an independently pinned test execution.
