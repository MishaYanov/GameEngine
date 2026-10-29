# ADR-0005: Runtime lifecycle, scheduling, and transform authority

Status: Accepted core direction for serial ordering, movement authority, bounded actions and explicit commit/failure
scope; expanded scheduling and lifecycle systems remain proposed. Existing simulation remains the implementation base.

Date: 2026-10-27

Decision update: 2026-10-29, following explicit user acceptance of the researched recommendations.

Scope: Runtime initialization/teardown, simulation phases, bounded work, movement ownership, and execution ordering.

Dependencies: [ADR-0002](0002-object-composition-and-components.md) defines composition;
[ADR-0004](0004-authoring-play-and-editor-state.md) defines world ownership. Coordinate execution with
[ADR-0007](0007-graph-assets-and-execution.md), module lifecycle with
[ADR-0008](0008-game-modules-and-scripting.md), and action sampling with
[ADR-0010](0010-input-actions-and-possession.md). No ECS, physics, or job-system library is selected here.

Authoritative accepted core: [Resolved decision](resolved/0005-serial-scheduling-and-rollback.md). This original URL
retains research, history and remaining proposals; those extensions are not resolved by the core implementation.

## Context and example

A character receives forward input while an animation supplies root motion and a wall blocks movement. If a gameplay
controller, animation graph, and physics system independently assign the transform, callback order decides whether the
character slides, clips through the wall, or snaps backward. The editor's subject order must not decide runtime
behavior.

We need one declared movement authority and an explicit schedule. The schedule must work in editor Play, an installed
player, and headless tests without Angular determining simulation timing.

## Existing implementation inspected

The current inspection baseline is `b6ec4ba5a88c5ef66eff82fa61eb5747b940400d` plus staged, unstaged, and untracked
changes on 2026-10-29. The drafting baseline was `fcb748b`. Current optimization and transition changes were inspected
again and preserved; source presence is separate from current native or behavioral verification.

`crates/engine-core/src/simulation.rs` already separates fixed stepping from rendering. Its default is 60 Hz with at
most
eight catch-up steps. Configuration validates rate/budget bounds; each advance caps accepted elapsed time at 0.25
seconds,
reports dropped time, and retains a fractional interpolation value. These are current limits, not benchmark-derived
targets.

`crates/engine-core/src/simulation_session.rs` owns the world, collision state, input, clock, module, and action buffer.
`GameModule::plan` reads a runtime world and emits bounded `Move`, `Rotate`, `SetTransform` and typed `Progress` actions
through progress-aware compatibility hooks. Built-in
behavior
currently covers controlled movement and spin, not a general physics or animation scheduler. Action, matrix, and
collision
work limits apply across the catch-up batch. On an action failure, touched transforms are restored, simulation pauses,
input is cleared, and the batch does not advance its tick.

That rollback does not prove rollback of arbitrary module interior state or external effects. `plan(&self, ...)` alone
cannot prevent interior mutation. A broader runtime must define those guarantees explicitly.

The optimized path shares `Arc<ProjectData>`, prepares resolved behaviors once for scene construction, and supplies
`PreparedModuleConfig` to `GameModuleFactory::create_prepared`. A `ChangeAccumulator` collects world changes during
the catch-up batch. `GameSession` owns the scene lifetime and rejects stale/foreign transition candidates. There is
now a typed candidate progress transaction across the aggregate tick. A general structural action buffer, animation
evaluator and expanded phase scheduler remain unimplemented.

## Research rationale

Separating fixed simulation from variable-rate presentation is an established engine pattern. Godot documents fixed
physics processing independently from frame-driven processing, supporting this separation without determining our exact
phase order or
guarantees. [Godot processing documentation](https://docs.godotengine.org/en/stable/tutorials/scripting/idle_and_physics_processing.html).

## Accepted core decisions

Use a small serial pipeline: consume input, plan bounded gameplay actions from committed read-only state, resolve
movement/collision, commit supported state changes, then publish results. One declared movement authority resolves an
entity's motion. Additional scheduling machinery follows actual systems and measured need.

Unreal tick groups/prerequisites and Bevy's deferred commands demonstrate explicit ordering and synchronization.
Deferred command application alone does not provide all-or-nothing rollback. The failure guarantee below is this
engine's chosen contract, not a property inferred from those mechanisms.
[Unreal actor ticking](https://dev.epicgames.com/documentation/en-us/unreal-engine/actor-ticking-in-unreal-engine),
[Bevy deferred commands](https://docs.rs/bevy/latest/bevy/ecs/prelude/struct.Commands.html).

### State commit and failure boundary

Preserve the current aggregate `SimulationSession::tick(elapsed)` failure boundary, which can contain several fixed
catch-up steps. Typed game progress now stages across that same call: later steps may
read earlier candidate values, but observers receive only committed results. Rejecting any covered action discards
the candidate progress and restores the covered transforms across the batch. A host/debug progress command can apply
as its own atomic batch between simulation updates; it must use the same domain validation and declared work bounds.

Emit rewards, notifications and other publishable gameplay events only after successful commit. A collected-item
operation must not commit its reward while restoring the item's covered world change. Unsupported topology changes,
arbitrary module interior mutations, filesystem/network writes and other external effects are outside the current
rollback guarantee. Stop or pause on unsupported failure according to a visible development policy.

Progress staging uses a bounded lazy candidate, retaining `ChangeAccumulator` for world changes. Progress changes have
their own result/read model and do not invent entity-transform changes. Preserve
shared project ownership and document-cost caching; avoid full project/world copies in the steady update path.
The resolved pages describe the current typed-state API and attributed evidence with snapshot limits; arbitrary native
effects remain outside rollback.

## Remaining lifecycle and scheduling proposals

The core ordering, authority and failure rules above are accepted. Detailed component callbacks, structural mutation,
animation/root-motion phases, parallel execution and new physics backends below remain proposals.

### World and component lifecycle

A runtime world progresses through **constructing, ready, running/paused, stopping, and disposed** states. Constructing
validates authored data, resolves required assets, creates component state, and establishes references before enabling
behavior. Required-resource failure prevents readiness; optional pending resources use an explicitly defined inactive or
placeholder state. A completion from an obsolete world generation cannot activate a replacement world.

Create, enable, disable, and destroy are distinct operations. Enable registers runtime participation at a defined
boundary;
disable stops participation and cancels owned transient work; destroy releases state after dependent work is quiescent.
Stopping cancels pending callbacks/jobs before releasing resources. Callbacks must not assume UI mounting or subject
order.
Module-specific hooks and reload policy belong to ADR-0008 rather than an invented universal callback ABI here.

### Fixed-step phase contract

The accepted minimal pipeline starts serially. The following expanded phase table is a future integration model for
animation and other systems, not a requirement to implement empty phases now. Explicit dependencies may refine it
without changing ownership.

| Phase                        | Responsibility                                                                                               |
|------------------------------|--------------------------------------------------------------------------------------------------------------|
| Boundary                     | Validate queued lifecycle/structural commands; publish the stable world view for this step.                  |
| Input and intent             | Consume logical input edges once, sample held values, and compute player/AI gameplay intent.                 |
| Animation motion preparation | Advance animation/state-machine time once and prepare any requested root-motion delta.                       |
| Movement resolution          | The assigned motor/body authority resolves intent, root motion, constraints, and collisions.                 |
| Transform propagation        | Publish resulting local/world transforms in valid parent order.                                              |
| Pose and dependents          | Finalize animation pose/IK, camera, and audio state using resolved movement; do not advance clip time again. |
| Commit and publication       | Commit validated changes, enqueue presentation events, and expose bounded diagnostics/read models.           |

An animation graph may have work on both sides of movement, but has one logical time advance and one event identity per
step. ADR-0007 owns graph evaluation details. Presentation interpolates committed states where supported; it cannot
write
simulation transforms or emit a gameplay event again because an extra frame was rendered.

### One transform authority

Each moving object declares one runtime movement authority: for example a character motor, a dynamic body, or scripted
kinematic movement. Static objects have no continuous writer. These are conceptual policies pending component/physics
proofs, not a new serialized enum introduced by this document.

Player and AI controllers produce intent. Root motion produces a requested delta. The assigned movement authority
decides
the actual motion and reports measured velocity/grounding for animation. A teleport is an explicit runtime operation
with
defined collision and interpolation-reset behavior; it is not a second hidden transform writer. Child transforms
propagate
after parent resolution. Competing owners are rejected when configuring/enabling the object.

### Structural mutation and failure

Spawn, despawn, reparent, and component attachment changes enter a bounded command buffer. They become visible only at
declared boundaries, never while a system iterates a live collection. Use validated, generation-aware handles so
removing
and recreating an object cannot redirect an old command. Newly created objects participate on the following eligible
step.

Preserve existing transform rollback while expanding runtime mutation. Stateful modules and graphs must use engine-owned
staged state or an explicit prepare/commit mechanism before their state joins the same atomic guarantee. Do not
advertise
whole-world rollback until that proof exists. Publish audio/event side effects after successful commit; arbitrary file
or
network I/O cannot be undone by restoring transforms. Unsupported recovery pauses the session with a diagnostic rather
than continuing from an undocumented mixture of states.

### Timing, ordering, and work budgets

Keep a configured fixed timestep and bounded catch-up. Report dropped time rather than concealing a slow frame. Preserve
button/event consumption across multiple catch-up steps and avoid replaying an edge per step. Aggregate budgets include
all work in the scheduling call, not only each individual action.

Order systems by declared dependencies and stable registration/identity rules. Hash-map iteration must not define
gameplay
order. Fixed stepping and stable ordering improve reproducibility but do not establish cross-platform floating-point
determinism, network lockstep, or replay compatibility. Random seeds, asset versions, physics behavior, and external
inputs
need separate specifications before such promises.

Idle worlds may sleep only when no active system, pending input, timer, animation, or resource completion needs
progress.
Extend the current `needs_tick` mechanism with explicit wake sources as those systems arrive. Parallel execution follows
measured need and declared data access/dependencies; it must preserve the observable phase contract.

## Alternatives, open choices, and sequence

An unrestricted per-component update callback is easy to expose but hides ordering and competing writes. A full ECS/job
framework could enforce access rules, but adopting one before workload evidence adds migration risk. Retain the current
module/action seam and introduce phases incrementally.

Open gates include the physics backend, root-motion collision policy, general post-commit event delivery API and
job-system thresholds. Typed native progress and its supported aggregate failure boundary are now implemented. Add
structural/animation phases only with their corresponding features,
then measure before parallelizing. Existing optimized construction and change collection remain the base.

## Acceptance and verification limits

- Equivalent fixed-step inputs produce equivalent results across supported render-frame chunking when no time is
  dropped.
- Catch-up limits report dropped time; button edges and animation events are consumed once at their intended step.
- A blocked root-motion character cannot bypass the movement authority; competing transform owners fail validation.
- Spawn/despawn/reparent during execution respect boundaries and invalidate stale handles.
- Fault injection verifies the promised rollback scope, clears input, and avoids publishing effects from rejected work.
- A failed later catch-up step discards covered progress and world changes from earlier steps in the same update;
  host/debug batches are atomic, and no reward or notification escapes rejected work.
- Stop/restart releases resources and rejects old completions; headless and native hosts use the same simulation phases.

These gates cover their respective scopes, including proposed structural/animation features. The native typed-state
transaction is present; this documentation resolution does not execute it or establish root-motion, benchmark or
determinism acceptance. Documentation-site checks establish publication
only; runtime and native verification remain separate.
