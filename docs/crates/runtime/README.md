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

`Scene::render_objects()` returns `Vec<renderer::vulkan::RenderObject<'_>>` borrowing scene resources. Each model part becomes a request with its instance transform. Drop these borrowed requests before mutating the scene. IDs increase without reuse in one scene; initial persistence stores source paths/names/transforms/camera rather than IDs; stable cross-scene references are not defined.

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

`runtime::SceneData` defines schema-version-1 entities/transforms/camera JSON with validation. `runtime::scene_io::{load, save, LoadedScene}` loads models and IDs in file order and saves snapshots via a sibling temporary file and rename. `Scene::spawn_model` records name/source path, `name` reads labels, and `to_data` creates a saveable snapshot; anonymous/source-less entries cannot be saved. No durable entity identity or version migration is established yet. See [schema, failure handling, and controls](../../scene-persistence.md).

`runtime::CameraController` now handles normalized camera movement and mouse rotation. Robot-specific rules stay in `apps/game/src/game.rs`. The existing timing/input/camera in-progress designation remains in effect.

## Status-only engine path

`Engine::new() -> Result<Engine, EngineError>` still owns a status-only `renderer::Renderer`. `Engine::status()` returns `bridge::EngineStatusDto`; initialization errors preserve their root cause. The editor host wraps this engine in a mutex. This path remains separate from `WindowRenderer`; shared scene APIs do not yet provide an editor viewport or a unified update loop.

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

Timing, key state, movement, and camera control are now implemented provisionally in the working tree and remain in progress. Initial scene persistence is now in progress. Action mapping, fixed-step updates, hierarchy, full glTF scenes, and editor scene commands remain future work.
