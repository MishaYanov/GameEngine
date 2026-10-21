# Scene data, save, and reload — in progress

**Status: In progress.** The current working tree introduces `runtime::scene_data`, `runtime::scene_io`, application save/reload, and the bundled `apps/game/assets/scenes/robots.json`. This is a first scene format and demo integration, not a completed editor authoring workflow.

## Schema version 1

`SceneData` contains `schema_version`, ordered `entities`, and `camera`. Each entity stores `name`, asset-relative `model_path`, and a transform with position/rotation/scale arrays. Camera stores position, yaw, pitch, vertical_fov, and JSON `near`/`far` fields. Rotations/FOV are in radians. Only version 1 is accepted; there is no version migration yet.

`from_json` parses and validates; `to_json` validates and pretty-prints. Unknown fields are rejected. Validation rejects blank names/paths, non-finite transforms/camera values, invalid projection planes/FOV, and camera pitch outside ±89 degrees. Paths use forward slashes and cannot have a leading slash, backslash, colon, or `..` component. Asset resolution subsequently checks the actual root.

The format stores no Vulkan handles, Arc references, runtime entity IDs, gameplay selection, animation state, or controller settings. Names are labels, not persistent identity. IDs are newly allocated per loaded scene; do not keep old IDs as references into a replacement scene.

## Scene metadata and snapshots

`Scene::spawn_model(name, model_path, instance)` records the source needed for saving. `spawn_named` stores a name without a source; plain `spawn` uses the label `entity`. `name(id)` reads a label. `Scene::to_data(&camera)` snapshots transforms/camera and fails if any entity lacks a source model path; use `spawn_model` for serializable model entities. Validation occurs on JSON serialization.

## Load and save APIs

`scene_io::load(path, asset_root, &renderer)` reads/validates JSON, loads models through a new AssetManager, and returns `LoadedScene { scene, assets, camera, entity_ids }`. Identical model-path strings share one Arc<Model> within that load. `entity_ids` follows file order. This does not implement a global model cache or glTF node traversal.

`scene_io::save(path, &scene, &camera)` serializes first, creates an exclusive sibling temporary file, writes/syncs/closes it, then renames it over the destination. Failures clean up the temporary file where possible; the previous file is not deliberately deleted before replacement. The parent must exist. This replacement pattern is not a universal crash-durability guarantee or a multi-writer conflict policy.

## Game integration and controls

Startup reads `<asset root>/scenes/robots.json` rather than hardcoding scene construction. `RobotGame::from_entities` requires at least two distinct returned IDs: file order assigns left/right gameplay roles, names remain labels, and the first robot is initially selected. More scene entities can render, but only the first two get this demo's robot behavior.

- **Tab:** toggle selection between the two gameplay robots.
- **WASD:** move the selected robot in world XY; both robots retain automatic rotation.
- **F5:** save current transforms and camera to the active scene file.
- **F9:** reload that file, discarding unsaved state and resetting gameplay selection.

If F5/F9 both arrive in one frame, reload wins. Each operation resets the frame clock and returns before movement/rotation for that update. Errors are logged. Reload loads and checks a replacement first, waits for GPU idle, then swaps scene/cache/camera/gameplay roles. Failed load/role validation leaves the active scene in place; this is not a general transaction framework.

Development saves overwrite the checkout's `apps/game/assets/scenes/robots.json`; there is no undo, separate user-save folder, or overwrite prompt. Installed/name-file mode loads its packaged scene but skips update commands, so F5/F9 and gameplay movement are disabled there.

## Distribution limitation

The compiler recursively copies the asset tree and its copy test now checks `scenes/robots.json` as well as the model/textures. The admin Project Builder is different: its asset extension allowlist currently omits JSON, so a generated workspace can compile while missing the scene required at startup. Do not treat successful generated-project compilation as runtime validation. Fix that allowlist in implementation before relying on generated output with the new scene-based startup.

## Checks

```powershell
cargo test -p runtime -p game -p compiler --offline --locked
cargo check -p game --all-targets --offline --locked
cargo run -p game --example scene_io_smoke --offline --locked
```

Schema tests, filesystem save/replacement tests, game selection tests, and app error/command tests cover focused behavior. The new native `scene_io_smoke` example uses a hidden GPU window and temporary scene files. It is separate from `renderer_smoke` and is not automatically added to the current `test_all.py --native` list. Native save/reload, visible controls, generated output, and installed execution require their own results. Earlier renderer extraction checks do not certify this new feature.
