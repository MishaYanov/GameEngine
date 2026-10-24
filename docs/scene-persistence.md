# Scene data, save, and reload — in progress

**Status: implemented prototype; automated verification, native visual acceptance pending.** The current working tree
introduces `runtime::scene_data`, `runtime::scene_io`, application save/reload, and the bundled
`apps/game/assets/scenes/robots.json`. This is a first scene format and demo integration, not a completed editor
authoring workflow.

## Schema version 3 and persistent identity

Current saves use version **3**, retaining entity keys/bindings/transforms/camera and adding optional parent keys,
renderable state, selected mesh_index, tint material override, collider data, and a bounded scene light list.
Nonrenderable entities can preserve imported transform-only nodes. Names are labels; keys provide document identity.
Runtime EntityIds and GPU handles are not serialized.

Version-1 and version-2 input migrates in memory to v3. V1 receives deterministic legacy keys; V2 preserves
keys/bindings. New fields receive compatible defaults and older scenes receive a migrated default light. Reading alone
does not rewrite files. Unknown future versions fail; the current to_json writes v3.

Validation rejects malformed identities/paths, duplicates, missing/cyclic parents, invalid world transforms, non-finite
values, projection errors, and invalid materials/colliders/lights. The ceiling is 32 MiB scene JSON, 100,000 entities
and 16 lights, not a tested real-time scene size. Local TRS remains the format: keep-world reparent can fail when a
required transform contains shear or cannot be safely inverted.

## Document history and prefabs

The authoritative Angular path edits CPU SceneData through runtime::editor::EditorSession and expected-revision
commands. It provides multi-key selection, grouped transactions and bounded patch history (128 records / 16 MiB).
Save/reload reject an active transaction; commit/cancel is explicit. Authored content is separate from the temporary
Play copy and saving never implicitly adopts simulated changes.

PrefabData has its own schema version 1 and bounded isolated entity subtrees. Export detaches external roots while
preserving placement; instantiation creates new keys and remaps internal parents as one edit. Prefabs are not
live-linked overrides or copies of every referenced asset. See [steps 21–35](editor-steps-21-35.md).

The older GPU SceneEditor/Scene snapshot APIs remain for legacy demo compatibility, with a different history model. Do
not carry their transient IDs/history across document replacement or treat them as the new Angular transport.

## Load and save APIs

`scene_io::load(path, asset_root, &renderer)` reads/validates JSON, loads models through a new AssetManager, and returns
`LoadedScene { scene, assets, camera, entity_ids, source_schema_version }`. Identical model-path strings share one
Arc<Model> within that load. `entity_ids` follows file order. This does not implement a global model cache or glTF node
traversal.

`scene_io::save(path, &scene, &camera)` serializes first, creates an exclusive sibling temporary file, writes/syncs/closes it, then renames it over the destination. Failures clean up the temporary file where possible; the previous file is not deliberately deleted before replacement. The parent must exist. This replacement pattern is not a universal crash-durability guarantee or a multi-writer conflict policy.

## Application ownership and packaging

Angular owns editor controls and the chosen game pane. Its service supplies open/execute/save/reload and prefab/import
operations; no new layout or hardcoded keys were added. The host's EditorWorkspace resolves scoped project paths and
owns CPU session data, while the Windows viewport worker owns render resources.

The standalone game now normally runs without editor overlays. Historical native-demo F5/F6/F9 controls require the
explicit legacy demo environment variable and are not the product editor contract. See [game modes](apps/game/README.md)
and [Angular integration](apps/editor/README.md).

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
