# ADR-0007: Graph assets and execution

- **Status:** Proposed
- **Date:** 2026-10-27
- **Scope:** Animation and shader authoring, compilation, instances, and diagnostics.

## Context

A character can use the same locomotion graph as twenty other characters while each has a different current state,
speed, and playback position. Editing its blend threshold must update an authored asset without writing one character's
playback state into every instance. A shader graph can use the same canvas interactions, but its execution produces GPU
shader code rather than animation poses.

The decision is to share graph authoring infrastructure while separating domain semantics and execution. This proposal
does not select an animation library, shader compiler frontend, or generic visual scripting runtime.

## Current implementation

Inspected against commit `fcb748b04853209b5e17e680c38c847c19f7f365` and the working tree on the date above:

| Evidence                                                                    | What exists                                                                                                                                                       |
|-----------------------------------------------------------------------------|-------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `apps/editor/src/app/shared/components/graph/graph.models.ts`               | A version-1 document, discriminated node values, animation/shader domains, typed ports, and distinct state-machine transitions.                                   |
| `apps/editor/src/app/shared/components/graph/graph.store.ts`                | Editor changes and validation; a frontend store rather than an engine execution service.                                                                          |
| `apps/editor/src/app/shared/components/graph/animation/animation.models.ts` | Locomotion slots, optional air motion, transitions, root-motion settings, and editor weight previews. The weight helper explicitly leaves pose evaluation native. |
| `crates/renderer/build.rs`                                                  | Build-time `glslc` compilation of fixed vertex and fragment shaders; no graph-to-shader compiler.                                                                 |

The canvas therefore demonstrates authoring behavior, not implemented native graph execution. Existing string clip
references also need the asset identity contract before becoming production references.

## Dependencies and assumptions

[ADR-0001](0001-editor-command-pipeline.md) supplies authoritative commands and
transactions; [ADR-0002](0002-object-composition-and-components.md) separates graph assets from controller
components; [ADR-0003](0003-rust-typescript-bridge.md) supplies generated DTOs and bounded
diagnostics. [ADR-0004](0004-authoring-play-and-editor-state.md) owns authoring versus Play
state, [ADR-0005](0005-runtime-lifecycle-and-scheduling.md) owns evaluation timing,
and [ADR-0006](0006-asset-identity-and-import-pipeline.md) owns asset identity, dependency tracking, and import caches.
Their proposed details can change before acceptance.

## Proposed decision

### Separate authoring data, compiled programs, and instances

Use stable node and port IDs; `GraphAssetId` is a typed use or wrapper of ADR-0006's `AssetId`, not a second asset
identity system. A graph asset declares its domain, document schema version, parameter schema, and node kind/version
pairs. Node positions, groups, comments, and canvas bookmarks are editor metadata. They may travel with the source
asset, but moving a node does not invalidate its semantic compilation key.

Rust becomes authoritative for production graph contracts and validation, with generated TypeScript descriptions for the
editor. Shared canvas code handles selection, connections, dragging, keyboard navigation, and diagnostics. Domain
registries define legal node kinds, port types, defaults, version migrations, and compile rules. A shared numeric node
can have separate domain implementations; matching labels do not imply interchangeable executables.

The compiled program is disposable derived data. Its cache key includes normalized semantic source, transitive
dependency revisions, node/compiler versions, target features, and relevant build settings. GPU driver/device-specific
pipeline caches remain separate from portable shader artifacts. Save source assets and explicit migrations; never
require a cached executable to recover a project.

A runtime animation instance owns current states, playback clocks, blend history, trigger consumption, and event
cursors. A material instance owns its parameter overrides and resource bindings. Both reference immutable compiled
programs. Unity similarly distinguishes graph nodes and outputs from the components receiving their results; it is
useful precedent, not a compatibility
requirement. [Unity PlayableGraph documentation](https://docs.unity3d.com/Manual/Playables-Graph.html).

### Keep execution domain-specific

Animation compilation produces a validated evaluation plan with explicit pose, root-motion, and event outputs. Pose
sampling, skeleton compatibility, blend evaluation, and state transitions execute in Rust at the phases specified by
ADR-0005. Advance animation state/clocks once per simulation step: prepare intent and root-motion delta before
movement/physics, then evaluate the final pose against resolved transforms without advancing time again. Locomotion is
an optional authored composition driven by motion state. The sole movement authority consumes root motion once; the
graph does not independently overwrite the object's transform.

Shader compilation lowers typed expressions to a shader-specific intermediate form, then to the selected compiler
frontend and target. Vulkan shader modules require SPIR-V; that requirement does not choose whether this engine emits
GLSL, another source language, or SPIR-V
directly. [Vulkan shader modules](https://docs.vulkan.org/spec/latest/chapters/shaders.html). Validate resource bindings
and target capabilities before creating GPU resources. Keep shader compilation off the UI thread and outside workspace
mutation locks.

### Validate cycles according to their meaning

Reject dangling references, unknown required versions, duplicate IDs, invalid parameter values, incompatible ports,
missing outputs, and incompatible skeletons before activation. Preserve unsupported source for diagnostics without
silently deleting nodes.

Pose and shader data dependencies are acyclic initially. A future history/delay node must explicitly define
previous-step storage and initialization before permitting feedback. State-machine transition cycles are expected:
grounded, airborne, and grounded again is valid. Specify deterministic transition priority, trigger consumption, and a
bounded number of transitions per simulation step so zero-duration loops cannot hang evaluation. Subgraph asset
dependencies must also reject recursive inclusion unless a later domain specification explicitly supports it.

### Make compilation and debugging revision-aware

Each compile request identifies the asset revision and dependencies. Cancel superseded work where practical, and reject
stale completions even when cancellation fails. Install a successfully compiled program only at a safe execution
boundary. Initially, replacement applies to an independent editor preview: Play pins its resolved program and asset
revisions until restart under ADR-0004/0006. Live Play replacement requires a later explicit reload policy. Invalid
edits retain the last valid preview, clearly labeled with its older revision; production builds fail on invalid required
graphs.

Initially reset an instance when its program changes. Preserving compatible state requires a later tested migration
contract. Diagnostics reference asset/node/port IDs and source revision. Optional debug subscriptions report active
states, weights, and selected events at a capped rate; they do not stream full skeleton poses through JSON. Closing the
graph removes its subscriptions.

## Alternatives and consequences

| Alternative                          | Tradeoff                                                                                         |
|--------------------------------------|--------------------------------------------------------------------------------------------------|
| One generic executor for every graph | Reuses machinery but obscures shader compilation, animation timing, and legal cycle differences. |
| Evaluate animation in TypeScript     | Convenient for demos, but duplicates native authority and makes runtime depend on a WebView.     |
| Entirely separate editors            | Gives each domain freedom but duplicates interaction, accessibility, and diagnostics work.       |

The proposed split adds compiler and schema work, but keeps runtime cost and correctness independent of canvas
complexity. Shared source assets reduce duplication while instances retain independent state.

## Open decisions and acceptance gates

Choose the shader frontend, animation sampling library, transition tie-breaking details, event/root-motion semantics,
and per-platform compile budgets through focused prototypes. Set measurable compile latency, allocation, graph-size, and
runtime evaluation limits before acceptance. GPU resource replacement must coordinate with ADR-0012's rendering
ownership. No editor preview currently proves native execution performance.

## Implementation sequence and verification

1. Introduce versioned Rust graph contracts and fixtures; migrate version-1 editor documents and preserve stable IDs.
   Verify malformed documents and unsupported versions produce actionable diagnostics.
2. Implement one native clip-to-output path and a two-clip blend. Verify two instances share a program while clocks and
   triggers remain independent.
3. Add the minimal locomotion state machine. Test valid transition loops, zero-time loop bounds, incompatible skeletons,
   and single application of root motion.
4. Implement a color-to-material shader path. Verify compiler errors map to nodes and an older asynchronous completion
   cannot replace a newer graph.
5. Add cache invalidation, safe replacement, and bounded debug subscriptions. Test source reload without caches,
   dependency changes, failed builds, stop/restart, and subscription cleanup.

Acceptance requires these native tests and recorded performance measurements. This ADR documents the intended boundary;
it does not claim that graph runtimes or compilation have been implemented.
