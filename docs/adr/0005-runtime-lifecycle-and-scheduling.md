# ADR-0005: Runtime lifecycle, scheduling, and transform authority

Status: Proposed; existing fixed-step simulation remains in place until incremental implementation is verified.

Date: 2026-10-27

Scope: Runtime initialization/teardown, simulation phases, bounded work, movement ownership, and execution ordering.

Dependencies: [ADR-0002](0002-object-composition-and-components.md) defines composition;
[ADR-0004](0004-authoring-play-and-editor-state.md) defines world ownership. Coordinate execution with
[ADR-0007](0007-graph-assets-and-execution.md), module lifecycle with
[ADR-0008](0008-game-modules-and-scripting.md), and action sampling with
[ADR-0010](0010-input-actions-and-possession.md). No ECS, physics, or job-system library is selected here.

## Context and example

A character receives forward input while an animation supplies root motion and a wall blocks movement. If a gameplay
controller, animation graph, and physics system independently assign the transform, callback order decides whether the
character slides, clips through the wall, or snaps backward. The editor's subject order must not decide runtime
behavior.

We need one declared movement authority and an explicit schedule. The schedule must work in editor Play, an installed
player, and headless tests without Angular determining simulation timing.

## Existing implementation inspected

The baseline is `fcb748b04853209b5e17e680c38c847c19f7f365` plus the working tree inspected on 2026-10-27.
Concurrent edits in `simulation_session.rs` and its tests change factory scene targeting; they were read and preserved.

`crates/engine-core/src/simulation.rs` already separates fixed stepping from rendering. Its default is 60 Hz with at
most
eight catch-up steps. Configuration validates rate/budget bounds; each advance caps accepted elapsed time at 0.25
seconds,
reports dropped time, and retains a fractional interpolation value. These are current limits, not benchmark-derived
targets.

`crates/engine-core/src/simulation_session.rs` owns the world, collision state, input, clock, module, and action buffer.
`GameModule::plan` reads a runtime world and emits bounded `Move`, `Rotate`, or `SetTransform` actions. Built-in
behavior
currently covers controlled movement and spin, not a general physics or animation scheduler. Action, matrix, and
collision
work limits apply across the catch-up batch. On an action failure, touched transforms are restored, simulation pauses,
input is cleared, and the batch does not advance its tick.

That rollback does not prove rollback of arbitrary module interior state or external effects. `plan(&self, ...)` alone
cannot prevent interior mutation. A broader runtime must define those guarantees explicitly.

## Research rationale

Separating fixed simulation from variable-rate presentation is an established engine pattern. Godot documents fixed
physics processing independently from frame-driven processing, supporting this separation without determining our exact
phase order or
guarantees. [Godot processing documentation](https://docs.godotengine.org/en/stable/tutorials/scripting/idle_and_physics_processing.html).

## Proposed decision

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

Start with serial, ordered phases. Explicit dependency declarations may refine their internals without changing
ownership.

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

Open gates include the physics backend, root-motion collision policy, state-staging cost, event delivery semantics on
failure, and job-system thresholds. Implement lifecycle/phase tracing first, then action input and movement authority,
then graph preparation/finalization, then structural commands and staged mutable state. Benchmark before parallelizing.

## Acceptance and verification limits

- Equivalent fixed-step inputs produce equivalent results across supported render-frame chunking when no time is
  dropped.
- Catch-up limits report dropped time; button edges and animation events are consumed once at their intended step.
- A blocked root-motion character cannot bypass the movement authority; competing transform owners fail validation.
- Spawn/despawn/reparent during execution respect boundaries and invalidate stale handles.
- Fault injection verifies the promised rollback scope, clears input, and avoids publishing effects from rejected work.
- Stop/restart releases resources and rejects old completions; headless and native hosts use the same simulation phases.

These are future acceptance gates. No new scheduler, root-motion implementation, native timing benchmark, or determinism
test was executed in this documentation pass.
