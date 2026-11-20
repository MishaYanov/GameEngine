# Scene data, save, and reload

Scene documents are owned by engine-core and exposed through runtime adapters. Current authoring retains multiple drafts
under project schema 3, while scene saves use schema 4. Read [project lifecycle](project-lifecycle.md) for scene IDs,
project opening, per-scene dirty state, Save As, register, reload policies and catalog-wide build checks. Native visual
acceptance remains separate.

## Schema version 4 and persistent identity

Current saves use version **4**, retaining entity keys/bindings/transforms/camera, optional parent keys and scene lights.
Objects store typed component arrays for mesh rendering, material overrides, colliders and native behaviors. Existing
flat internal adapters and computed transport views preserve the runtime/editor boundary.
Nonrenderable entities can preserve imported transform-only nodes. Names are labels; keys provide document identity.
Runtime EntityIds and GPU handles are not serialized.

Box/Sphere colliders may contain `interaction` with sensor state and unsigned membership/detection masks. Legacy
absence defaults to a blocking collider on layer bit 0 accepting all masks. Partial objects default fields; fully
default settings serialize omitted. Both filters must match for detection, and sensor pairs do not block movement.
These remain authored component fields, participate in ordinary undo/save/reopen and are separate from editor-only
outline visibility. Sensor edges drive the implemented trigger lifecycle. [Collider reference](scene-editing.md#colliders-and-interaction-settings).

Version-1, version-2 and version-3 input migrates in memory to v4. V1 receives deterministic legacy keys; V2 preserves
keys/bindings. New fields receive compatible defaults and older scenes receive a migrated default light. Reading alone
does not rewrite files. Unknown future versions fail; the current to_json writes v4.

Validation rejects malformed identities/paths, duplicates, missing/cyclic parents, invalid world transforms, non-finite
values, projection errors, and invalid materials/colliders/lights. The ceiling is 32 MiB scene JSON, 100,000 entities
and 16 lights, not a tested real-time scene size. Local TRS remains the format: keep-world reparent can fail when a
required transform contains shear or cannot be safely inverted.

## Document history and prefabs

The authoritative Angular path edits CPU SceneData through runtime::editor::EditorSession and expected-revision
commands. It provides multi-key selection, grouped transactions and bounded patch history (128 records / 16 MiB).
Save/reload reject an active transaction; commit/cancel is explicit. Authored content is separate from the temporary
Play copy and saving never implicitly adopts simulated changes.

Canonical prefab files use schema 3, migrating legacy versions 1/2. They retain bounded entity subtrees, typed
components, optional legacy gameplay data and explicit external reference slots. Copy export detaches external
roots; copy instantiation creates new keys and remaps parents as one edit. Linked instances separately retain a
source identity/version and typed overrides with explicit update/revert/unpack. See the
[current prefab workflow](milestones-4-6-review.md#milestone-5-linked-prefab-authoring); the
[steps 21–35](editor-steps-21-35.md) remain the historical introduction.

Native SceneEditor delegates authored edits/history to the core session. Scene/EntityId and GPU snapshots remain
resource adapters; transient IDs are not persistent scene/entity identities.

## Load and save APIs

`scene_io::load(path, asset_root, &renderer)` reads/validates JSON, loads models through a new AssetManager, and returns
`LoadedScene { scene, assets, camera, entity_ids, source_schema_version }`. Identical model-path strings share one
Arc<Model> within that load. `entity_ids` follows file order. AssetManager also retains bounded static-scene metadata
and selected-mesh resource snapshots; this is scoped cache reuse, not a global hot-reload system. Static scene import
can traverse glTF hierarchy; animation and live-linked prefab behavior remain separate.

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

Schema tests, filesystem save/replacement tests, game selection tests, and app error/command tests cover focused behavior. The new native `scene_io_smoke` example uses a hidden GPU window and temporary scene files. It is separate from `renderer_smoke` and is not automatically added to the current `test_all.py --native` list. Native save/reload, visible controls, generated output, and installed execution require their own results. Earlier renderer extraction checks do not certify this new feature.
