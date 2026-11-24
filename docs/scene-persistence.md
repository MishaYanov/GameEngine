# Scene data, save, and reload

Scene documents are owned by engine-core and exposed through runtime adapters. Current authoring retains multiple drafts
under project schema 3, while scene saves use schema 8. Read [project lifecycle](project-lifecycle.md) for scene IDs,
project opening, per-scene dirty state, Save As, register, reload policies and catalog-wide build checks. Native visual
acceptance remains separate.

## Scene schema and persistent identity

Current saves use version **8**, retaining entity keys/bindings/transforms/camera, optional parent keys and scene
lights.
The scene format supports an optional `camera.orbit` definition. Its target is a persistent entity key. The arm's authored
distance limits, pivot offset, response times, probe radius and obstruction mask survive save/reopen. Play keeps
the changing eye position, current blocker and recovery progress in runtime state. A camera can use fixed, follow
or orbit behavior. [Orbit camera obstruction](timing-input-camera.md#orbit-camera-obstruction).

**Game Camera…** edits one complete definition. Preview samples live in an edit-only native session. Apply uses the
existing scene revision and undo history, and Save All persists the accepted
defaults. [Camera setup](timing-input-camera.md#orbit-camera-obstruction).

Objects store typed component arrays for mesh rendering, material overrides, colliders and native behaviors. Existing
flat internal adapters and computed transport views preserve the runtime/editor boundary.
Nonrenderable entities can preserve imported transform-only nodes. Names are labels; keys provide document identity.
Runtime EntityIds and GPU handles are not serialized.

Box/Sphere colliders may contain `interaction` with sensor state and unsigned membership/detection masks. Legacy
absence defaults to a blocking collider on layer bit 0 accepting all masks. Partial objects default fields; fully
default settings serialize omitted. Both filters must match for detection, and sensor pairs do not block movement.
These remain authored component fields, participate in ordinary undo/save/reopen and are separate from editor-only
outline visibility. Sensor edges drive the implemented trigger lifecycle. [Collider reference](scene-editing.md#colliders-and-interaction-settings).

Supported legacy scenes migrate in memory. Version 1 receives deterministic legacy keys. Version 2 preserves
keys and bindings. Both receive legacy lighting defaults. Loading leaves the existing file in place, and the next
save writes the current scene schema.

Validation rejects malformed identities/paths, duplicates, missing/cyclic parents, invalid world transforms, non-finite
values, projection errors, and invalid materials/colliders/lights. The ceiling is 32 MiB scene JSON, 100,000 entities
and 16 lights, not a tested real-time scene size. Local TRS remains the format: keep-world reparent can fail when a
required transform contains shear or cannot be safely inverted.

## Document history and prefabs

The authoritative Angular path edits CPU SceneData through runtime::editor::EditorSession and expected-revision
commands. It provides multi-key selection, grouped transactions and bounded patch history (128 records / 16 MiB).
Save/reload reject an active transaction; commit/cancel is explicit. Authored content is separate from the temporary
Play copy and saving never implicitly adopts simulated changes.

Canonical Prop files use schema 4 and accept older supported prefab documents. They retain bounded entity subtrees, typed
components, optional legacy gameplay data and explicit external reference slots. Copy export detaches external
roots; copy instantiation creates new keys and remaps parents as one edit. Linked instances separately retain a
source identity/version and typed overrides with explicit update/revert/unpack. See the
[linked Prop workflow](authoring-objects.md#reuse-a-group-as-a-prop).

Native SceneEditor delegates authored edits/history to the core session. Scene/EntityId and GPU snapshots remain
resource adapters; transient IDs are not persistent scene/entity identities.

## Load and save APIs

`scene_io::load(path, asset_root, &renderer)` reads/validates JSON, loads models through a new AssetManager, and returns
`LoadedScene { scene, assets, camera, entity_ids, source_schema_version }`. Identical model-path strings share one
Arc<Model> within that load. `entity_ids` follows file order. AssetManager also retains bounded static-scene metadata
and selected-mesh resource snapshots; this is scoped cache reuse, not a global hot-reload system. Static scene import
can traverse glTF hierarchy. Animation remains separate; linked prefab source changes require an explicit reviewed
update rather than automatic live propagation.

`scene_io::save(path, &scene, &camera)` serializes first, creates an exclusive sibling temporary file, writes/syncs/closes it, then renames it over the destination. Failures clean up the temporary file where possible; the previous file is not deliberately deleted before replacement. The parent must exist. This replacement pattern is not a universal crash-durability guarantee or a multi-writer conflict policy.

## Application ownership and packaging

Angular owns editor controls and the chosen game pane. Its service supplies open/execute/save/reload and prefab/import
operations. The current workspace composes those controls and the native pane. The host's EditorWorkspace resolves scoped project paths and
owns CPU session data, while the Windows viewport worker owns render resources.

Source game execution defaults to Development mode with the existing panels/gizmos; installed projects default to
Player. Both use shared core semantics. The Angular editor remains the product UI direction.
See [game modes](apps/game/README.md).

Compiler installation and admin source generation now include validated asset JSON, fixing the earlier missing-scene
packaging omission. Copying is bounded and ordered with link/containment/format checks. Generated compilation, a
successful save, or a hidden smoke run still does not prove installed execution outside the checkout or full user
interaction.

## Checks

```powershell
cargo test -p runtime -p game -p compiler --offline --locked
cargo check -p game --all-targets --offline --locked
cargo run -p game --example scene_io_smoke --offline --locked
```

The native test runner includes `scene_io_smoke`. It opens a hidden Vulkan window and exercises saving and
reopening temporary scene files. Follow [desktop acceptance](testing.md#desktop-and-release-acceptance) to
verify visible editing and installed package execution.
