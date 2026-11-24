# Architecture implementation record

The relevance check used `ad909d9` as the starting point. The working changes extend existing commands, the
fixed-step simulation, and the structured scene format. [Milestones and acceptance](../roadmaps/adr-implementation-203-222.md).

## 203–204 · Export ownership and status

The native build state owns the destination reservation through prepare, finish, and retry. A failed finish retains
the prepared job. The Angular export control reads the scoped status after a reconnect. This is exercised by the
host build command and Angular project-build suites.

```rust
pub(crate) fn readback(&self, workspace_generation: u64) -> ProjectBuildReadback
```

Sources [Native reservation](../../apps/editor-host/src-tauri/src/state/project_build.rs) ·
[Export control](../../apps/editor/src/app/modules/editor/workspace/project-build/project-build.ts) ·
[Command pipeline decision](../adr/0001-editor-command-pipeline.md).

## 205–206 · Editor command transport and catch-up

The client queues commands inside a captured workspace generation and keeps bounded command outcomes.
A Rust and Angular fixture checks the journal vocabulary. After listening for notifications, the client
asks for changes since its last known cursor. An expired cursor requests an authoritative snapshot.

```typescript
const page = readEditorChangePage(await transport.invoke('editor_changes', {cursor: after}), after);
```

A local Node.js probe ran 60 measurements of each synthetic payload after ten parse warmups. The full case
contains 12,000 object records with a mesh reference and transform. The edit case contains one upsert. The
journal case is the two-event contract fixture. These timings cover JSON in the process.

| Payload | UTF-8 bytes | Encode p95 | Decode p95 |
| --- | ---: | ---: | ---: |
| Full document | 2,233,665 | 2.132 ms | 3.612 ms |
| Single-object update | 257 | 0.001 ms | 0.001 ms |
| Two journal events | 243 | 0.001 ms | 0.001 ms |

Keep JSON for ordinary scoped edits. Capture native end-to-end IPC timings before making a wire-format decision.
Sources [Editor service](../../apps/editor/src/app/core/editor/editor.service.ts) ·
[Contract test](../../crates/bridge/tests/editor_change_page.rs) · [Bridge decision](../adr/0003-rust-typescript-bridge.md).

## 207–209 · Object selection, activation and lights

Editing an axis on a multi-selection uses one authored transform command. **Active in Play** is a saved local
object flag. Play projects inherited inactivity into its own document, suppressing rendering, collision,
scripts, controllers, animation, audio and particles for that subtree. Runtime actions respect the effective
state. Both old scene documents and older Prop sources default to active.

```json
{"type":"set_object_active","key":"assembly-root","active":false}
```

Point and directional lights have their own component and normal hierarchy identity. Existing records are
converted on Editor open with the same keys. The renderer computes light pose from the object world transform.
Preflight normalizes equivalent legacy scene records before comparing against the retained scene.

Sources [Inspector](../../apps/editor/src/app/modules/editor/workspace/inspector-panel/inspector-panel.html) ·
[Activation](../../crates/engine-core/src/scene_data/activation.rs) ·
[Light component](../../crates/engine-core/src/components/light.rs) · [Composition decision](../adr/0002-object-composition-and-components.md).

## 210–211 · Reviewed Play edits and workspace layout

The editor captures before and after local transforms for existing objects in a paused Play run. Selected
keys pass the same native revision and run checks, and one authored commit stops Play and records Undo.

```typescript
{type: 'apply_play_transforms', run_id: review.run_id, scene_revision: review.scene_revision, keys}
```

Layout preferences use the canonical project scope. Personal settings retain their global scope and old global
layout preferences serve as a fallback. Explicit layout import and export remain open.

Sources [Play review](../../crates/engine-core/src/editor/play_review.rs) ·
[Layout preferences](../../apps/editor/src/app/modules/editor/workspace/workspace-layout-preferences.ts) ·
[State lifetimes decision](../adr/0004-authoring-play-and-editor-state.md).

## 212–213 · Runtime hierarchy, emitters and root motion

Typed runtime actions reparent live handles, preserve the selected transform space, and validate the resulting
tree before commit. Particle Emitter enable changes are validated and transactional. Broader add/remove
component operations remain separate.

```rust
actions.reparent_runtime(child, parent, RuntimeReparentMode::KeepWorld)?;
actions.set_particle_emitter_enabled(emitter, component_id, false)?;
```

Imported Animator graphs can opt into planar root motion on a compatible rig node and Character Controller.
The motor consumes the blended contribution once per fixed step. The visual pose removes the corresponding
planar translation. The existing two-joint sample clips have no positional channels. The dedicated fixture
drives a two-unit-per-second clip, hits a wall, and checks deterministic blending and rollback.

Sources [Runtime action writer](../../crates/engine-core/src/simulation_session/gameplay.rs) ·
[Root motion](../../crates/engine-core/src/animation/root_motion.rs) ·
[Runtime lifecycle decision](../adr/0005-runtime-lifecycle-and-scheduling.md).

## 214–216 · Asset identity and cache evidence

Managed models publish stable mesh IDs. The source can supply `extras.game_engine_mesh_id`, and the importer
can use decoded geometry identity where unambiguous. Legacy numeric bindings remain supported. Reimport checks
the current scenes, saved drafts, Undo and linked Prop dependencies before it can change a numeric slot.

```text
Mesh Renderer model reference → asset_id + mesh_id → current catalog mesh → renderer mesh
```

Stable clip and material identities remain open. The cache measurement found retained metadata was fast and
that a large model could justify disk storage if real repeat misses become common. The in-memory cache remains
the active implementation.

Sources [Catalog](../../crates/engine-core/src/asset_catalog.rs) ·
[Reimport](../../crates/runtime/src/assets/project_assets.rs) ·
[Derived cache measurements](milestone-216-derived-cache.md) · [Asset identity decision](../adr/0006-asset-identity-and-import-pipeline.md).

## 217–218 · Material graph compilation and authoring

A scene mesh can save a bounded Color, Value, Multiply and Output graph. Compilation validates types,
connections and cycles and then produces constant RGB for the existing material path. Opacity remains a
mesh property. The export regression checks the authored graph and compiled color in an installed package
fixture. Reusable graph assets and dynamic shader pipelines remain open.

```rust
let compiled_color = source_graph.compile_base_color()?;
```

Sources [Native compiler](../../crates/engine-core/src/material_graph.rs) ·
[Editor dialog](../../apps/editor/src/app/modules/editor/workspace/material-graph/material-graph-dialog.ts) ·
[Graph execution decision](../adr/0007-graph-assets-and-execution.md).

## 219 · Project-authored rotation routines

The shared gameplay catalog registers a bounded rotation routine. Its source is saved with the Script
attachment. The same hooks run in editor simulation and packaged projects.

```json
[{"during":"start","degrees":35},{"during":"fixed_update","degrees":60}]
```

It can write the owner's local yaw. It runs at most six rules from 256 bytes of JSON and rejects unsupported
configuration versions. A general scripting language, wider effect APIs and a dedicated script asset remain open.

Sources [Rotation routine](../../crates/gameplay-catalog/src/rotation_routine.rs) ·
[Package parity test](../../compiler/tests/object_routine_package.rs) ·
[Game modules decision](../adr/0008-game-modules-and-scripting.md).

## 220 · Linked Prop presence overrides

A linked placement can remove a direct inherited leaf. The persisted **Presence** override keeps its scene key
reserved. Undo and Restore Object recover the leaf. A compatible source refresh preserves the override.
If the source adds descendants, **Use Source** restores the branch. Instance-owned child insertion, nested
leaf removal and per-instance reparenting remain open.

```json
{"source_entity":"lamp-head","property":{"kind":"presence"}}
```

Sources [Native linked edit](../../crates/engine-core/src/editor/linked_prefab.rs) ·
[Inspector](../../apps/editor/src/app/modules/editor/workspace/prefabs/linked-prefab-inspector.html) ·
[Linked Prop decision](../adr/0009-prefabs-and-instance-overrides.md).

## 221–222 · Runtime possession and ordered input

A typed runtime action can transfer the one local input stream to a valid character. Pending movement
and older held input are cleared and the new character receives input in the new epoch. The headless replay
checks deterministic handoff and restart. Authored player assignments remain available when Play stops.

```rust
actions.set_local_possession(Some(target))?;
```

The installed Windows Editor and Player still need a focus, controller reconnect and input latency run.
Device pairing for multiple local players remains open.

Sources [Possession](../../crates/engine-core/src/simulation_session/possession.rs) ·
[Replay](../../apps/headless/tests/possession_replay.rs) · [Input selection decision](../adr/0010-input-actions-and-possession.md).

## Integrated verification

The offline Rust workspace run passed 1,372 tests and skipped 24 opt-in checks. The Angular Editor passed
777 tests and its production bundle check. The Windows Editor built with the compiled frontend, along with
the Player and Agent worker. Workspace-wide strict Clippy and formatting checks passed. The documentation site built and link-checked 28 pages across
its two spaces. All eight site tests passed, including archive exclusion from published search.

The Agent worker test retrieved a citation from the current object authoring guide in a staged help layout.
The paired Dashboard job fixtures use their own project targets so they cannot replace binaries in the
workspace's shared Cargo target.

The native automated results cover component behavior, packaged data, light pose, source reimport, runtime
rollback and replay. Visible rendering, physical controller qualification and installer acceptance have
separate checks.
