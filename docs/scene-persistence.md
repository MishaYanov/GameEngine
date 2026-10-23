# Scene data, save, and reload — in progress

**Status: implemented prototype; automated verification, native visual acceptance pending.** The current working tree
introduces `runtime::scene_data`, `runtime::scene_io`, application save/reload, and the bundled
`apps/game/assets/scenes/robots.json`. This is a first scene format and demo integration, not a completed editor
authoring workflow.

## Schema version 2 and persistent identity

Current saves use schema version **2**: `schema_version`, `bindings`, ordered `entities`, and `camera`. Each entity now
has a persistent `key` plus name, asset-relative model_path, and position/rotation/scale. Camera fields remain
position/yaw/pitch/vertical_fov/near/far in radians where applicable. Names are labels and can repeat; keys must be
unique within a scene.

`EntityKey::new` requires a nonempty string without whitespace; `generate()` creates a UUID v4 string. Keys serialize as
strings. `EntityId` remains a transient scene lookup value, not a persistent reference. `Scene::entity_by_key` resolves
a key after loading; never reuse an old ID as a cross-scene reference.

`scene_data::parse_json` returns validated data and its original `source_schema_version`. Version-1 JSON is migrated in
memory to version 2: keys become `legacy-entity-{index}` and bindings start empty. `SceneData::from_json` uses this
migration path; `to_json` writes validated version 2. Other versions fail. Reading alone does not rewrite the original
file.

The game supplies a version-1 compatibility adapter: its first two loaded IDs get `robot_a`/`robot_b` bindings.
Version-2 games require those bindings explicitly and do not infer roles from order or names. Robot simulation requires
distinct roles when both are present; generic Edit scenes and Play without robot simulation do not require them. The
application only adds version-1 compatibility roles when at least two entities exist. Saving then persists the migrated
keys/bindings as version 2.

Validation rejects unknown fields, blank names/paths, duplicate or whitespace-containing keys, blank/dangling bindings,
non-finite transform/camera values, invalid camera projection, and pitch beyond ±89 degrees. Model paths use '/' and
reject absolute/drive/backslash/parent traversal syntax; asset loading checks actual containment. Scale validation
checks finiteness, not positivity or nonsingularity.

No GPU handles, Arc values, runtime IDs, current selection, history, animation pause, or controller settings are
serialized. Persistent keys are scene identity, not a global cross-project reference system.

## Scene metadata and snapshots

`spawn_keyed_model` accepts a validated unique key. `spawn_persistent_model` generates a fresh key; `duplicate` clones
the source's shared model and transform, adds an offset and a ' Copy' label, and assigns a new key. `spawn_model` still
records no persistent key, so it is insufficient for saving on its own. `spawn`/`spawn_named` also lack source paths.

`Scene::to_data(&camera)` requires both a model path and key for every entity, preserves bindings, and validates the
snapshot. `key`, `entity_by_key`, `set_binding`, and `binding` expose identity/role lookup. Removing an entity removes
bindings that target its key. See [editing and history](scene-editing.md).

## Load and save APIs

`scene_io::load(path, asset_root, &renderer)` reads/validates JSON, loads models through a new AssetManager, and returns
`LoadedScene { scene, assets, camera, entity_ids, source_schema_version }`. Identical model-path strings share one
Arc<Model> within that load. `entity_ids` follows file order. This does not implement a global model cache or glTF node
traversal.

`scene_io::save(path, &scene, &camera)` serializes first, creates an exclusive sibling temporary file, writes/syncs/closes it, then renames it over the destination. Failures clean up the temporary file where possible; the previous file is not deliberately deleted before replacement. The parent must exist. This replacement pattern is not a universal crash-durability guarantee or a multi-writer conflict policy.

## Game integration and controls

Startup reads `<asset root>/scenes/robots.json` into Edit mode and a generic SceneEditor. Robot simulation is optional
and runs on a temporary cloned scene/camera during Play (F6). Model resources are shared through Arc. Stop restores
authored state. Generic scenes need no robot bindings; a version-1 compatibility adapter uses the first two IDs only
when available. Version-2 roles resolve through keys, not file order or names.

F5 snapshots authored content even during Play. F9 reloads disk; success replaces scene/cache/camera/editor after GPU
idle and exits Play, while failure preserves live/authored state. Command priority is **F9, F5, F6, Delete, C, N**.
Save, reload, and mode transitions reset timing and suppress conflicting input. Their file operations are not undoable,
while transforms, rename, spawn, duplicate, and delete now share a mixed undo history.

The native game panel provides editable name/transform fields, a flat entity list, an asset catalog, and dirty state
derived from exact authored content comparison. Current selection/history and Play state are not persisted. Development
saves overwrite the checkout's scene asset file; no separate user-save folder or overwrite prompt exists. Installed mode
loads its packaged scene but excludes development editing tools. See [editing and controls](scene-editing.md)
and [steps 11–20](editor-steps-11-20.md).

## Distribution limitation

The compiler recursively copies the asset tree and its copy test now checks `scenes/robots.json` as well as the model/textures. The admin Project Builder is different: its asset extension allowlist currently omits JSON, so a generated workspace can compile while missing the scene required at startup. Do not treat successful generated-project compilation as runtime validation. Fix that allowlist in implementation before relying on generated output with the new scene-based startup.

## Checks

```powershell
cargo test -p runtime -p game -p compiler --offline --locked
cargo check -p game --all-targets --offline --locked
cargo run -p game --example scene_io_smoke --offline --locked
```

Schema tests, filesystem save/replacement tests, game selection tests, and app error/command tests cover focused behavior. The new native `scene_io_smoke` example uses a hidden GPU window and temporary scene files. It is separate from `renderer_smoke` and is not automatically added to the current `test_all.py --native` list. Native save/reload, visible controls, generated output, and installed execution require their own results. Earlier renderer extraction checks do not certify this new feature.
