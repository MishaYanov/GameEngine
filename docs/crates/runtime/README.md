# Shared engine runtime

Source: `crates/runtime`. Cargo package: `runtime`. Runtime now provides the reusable assets, models, instances, transforms, and scene APIs consumed by the game, alongside the existing editor engine-status wrapper.

## Shared application APIs

| API | Responsibility |
| --- | --- |
| `runtime::Transform` or `runtime::transform::Transform` | Position, Euler rotation in radians, scale |
| `runtime::assets::AssetManager` | File import and GPU resource caches |
| `runtime::assets::{Model, MaterialData, MeshData, ImageAsset}` | Model parts, CPU material/geometry/image data |
| `runtime::model_instance::ModelInstance` | Shared `Arc<Model>` plus independent transform |
| `runtime::scene::{Scene, EntityId}` | Spawn, mutable lookup, remove, iterate, construct draw requests |

`Scene::render_objects()` returns `Vec<renderer::vulkan::RenderObject<'_>>` borrowing scene resources. Each model part
becomes a request with its instance transform. Drop these borrowed requests before mutating the scene. Fresh IDs are
allocated monotonically; history restoration deliberately restores the prior ID; persistence stores persistent keys,
bindings, source paths, names, transforms, and camera rather than transient IDs. Resolve keys after scene replacement.

A minimal scene can be constructed without GPU resources:

```rust
use std::sync::Arc;
use runtime::{Transform, assets::Model, model_instance::ModelInstance, scene::Scene};
let mut scene = Scene::default();
let id = scene.spawn(ModelInstance::new(
    Arc::new(Model::new(Vec::new())), Transform::default(),
));
assert!(scene.render_objects().is_empty());
assert!(scene.remove(id).is_some());
```

For real content, use `AssetManager::new(root)` and `load_model(&window_renderer, relative_path)`. It imports all primitives of the first glTF mesh and asks `WindowRenderer` to upload meshes/images. Models/materials still contain Vulkan-specific resources: this extraction establishes ownership and reuse, not backend independence. See [graphics and assets](../../graphics-and-assets.md).

## Procedural primitives — in progress

`assets::MeshData` now exposes `triangle()`, `cube()`, `plane()`, `polygon(sides)`, `sphere()`, and `capsule()` through private shape-builder modules. They produce position/UV CPU meshes, uploaded with `AssetManager::load_mesh`. Polygon requires at least three sides; sphere/capsule dimensions and resolution are currently fixed. See [primitive dimensions, limits, and tests](../../graphics-and-assets.md#procedural-primitives-in-progress). Procedural scene serialization and collision shapes are not implemented by these constructors.

## Timing, input, and camera — in progress

The working tree now exposes `FrameClock` (`time.rs`), `InputState`/`Key` (`input.rs`), and CPU `Camera` (`camera.rs`) from the crate root. The game uses them for capped delta-time updates, keyboard movement, and mouse-look camera control. All three APIs and their integration are **in progress**, not a completed input/gameplay milestone. See [current contracts and provisional controls](../../timing-input-camera.md).

## Scene data and I/O — in progress

`runtime::SceneData` now writes schema version 2 with persistent EntityKey values and role bindings; version-1 input
migrates in memory. `runtime::scene_io::{load, save, LoadedScene}` loads models and IDs in file order and saves
snapshots via a sibling temporary file and rename. Saving requires a model path and persistent key: use
spawn_keyed_model or spawn_persistent_model rather than plain spawn_model. Scene identity/binding lookup, duplicate,
shared EntitySelection, SetTransform, and bounded CommandHistory are now available.
See [editing and undo/redo](../../scene-editing.md).
See [schema, failure handling, and controls](../../scene-persistence.md).

`runtime::CameraController` now handles normalized camera movement and mouse rotation. Robot-specific rules stay in `apps/game/src/game.rs`. The existing timing/input/camera in-progress designation remains in effect.

## Editor and visibility APIs

`SceneEditor` now owns generic selection/edit/history independently of RobotGame. Shared bounds/picking/gizmo/visibility
modules provide model-local AABBs, near/far ray selection, transform-handle math, and conservative frustum collection.
Scene snapshots restore identity/order/bindings/shared models for mixed structural history. `assets::catalog` scans
supported model paths, while the application owns placement templates and UI.

These APIs support the native game panel, Edit/Play clone isolation, undoable name/properties/gizmos, model placement,
normals, and visibility statistics. See [steps 11–20](../../editor-steps-11-20.md)
and [native editing](../../scene-editing.md). They are not a backend-independent editor framework; model/material
resources still contain Vulkan handles.

## Status-only engine path

`Engine::new() -> Result<Engine, EngineError>` still owns a status-only `renderer::Renderer`. `Engine::status()` returns
`bridge::EngineStatusDto`; initialization errors preserve their root cause. The editor host wraps this engine in a
mutex. This path remains separate from `WindowRenderer`; the native game uses them for its Edit/Play UI, but they do not
yet provide an Angular/Tauri editor viewport or one unified status/presentation update loop.

The executable initializes the logger once. Uploaded scene/cache resources must be released after GPU idle and before their renderer/device. The game enforces that through shutdown and field order; the types do not prevent every lifetime misuse.

## Verification and current milestone

From the repository root:

```powershell
cargo test -p runtime -p renderer --offline --locked
cargo test -p runtime --test scene_api --offline --locked
cargo run -p game --example renderer_smoke --offline --locked
```

Runtime tests cover import/material data, image validation, scene removal and independent transforms, public API visibility, shared reference release, empty draw lists, and engine error chains. Native engine initialization is opt-in/ignored by default.

The five-step move is complete: temporary game library, scene-owned draw construction, Transform move, remaining reusable modules move, and validation. The temporary game library is no longer required. The user confirmed tests, native smoke, visible rendering, resize, and minimize checks after the move; see [testing](../../testing.md) for evidence boundaries.

Timing, key state, movement, camera control, and the native editing extension have implemented/tested prototype APIs.
Visual acceptance is separate. Initial scene persistence is now in progress. Action mapping, fixed-step updates,
parent-child hierarchy, full glTF scenes, and Angular/Tauri scene integration remain future work.
