# Native game application

Source: `apps/game`. Cargo package: `game`. The executable owns window events and application behavior; reusable scene/model/asset APIs live in `runtime`, and Vulkan presentation resources live in `renderer::WindowRenderer`.

## Run

From the repository root after [setup](../../usage.md):

```powershell
cargo run -p game
.\dev.ps1 game
cargo run -p game --release
cargo run -p game --example renderer_smoke --offline --locked
```

Requires Vulkan 1.3, a capable driver, `glslc` for shader builds, and the Khronos validation layer in debug builds.

## Application boundary

`GameApp` owns scene/cache/presentation resources, clock/input/camera, CameraController, and an optional application RobotGame. Startup loads `scenes/robots.json` through `runtime::scene_io::load` and derives robot roles from its ordered entity IDs. Runtime owns reusable camera behavior; `apps/game/src/game.rs` owns selection, movement, and robot rotation. `render()` prepares targets and submits `scene.render_objects()`.

The new scene/controller/gameplay split is **in progress**. Tab selects a robot; WASD moves it, arrow keys move the camera, and captured mouse motion rotates the view. F5 saves and F9 reloads the active scene. Development save writes the checkout asset file; reload discards unsaved state. See [scene format and save/reload limits](../../scene-persistence.md).

The application no longer constructs GPU device/swapchain resources or recreates them on resize. Window events call `renderer.resize`; minimized zero-size frames suspend through `prepare()`. Renderer errors are logged and stop the loop. The working tree now has capped delta-time updates, keyboard movement, and mouse look. These controls are **in progress**, with provisional bindings and no claim of completed native interaction validation. See [timing, input, and camera](../../timing-input-camera.md).

## Shared scene and model APIs

Use `runtime::scene::Scene`, `runtime::model_instance::ModelInstance`, `runtime::Transform`, and `runtime::assets::AssetManager`. Scene spawn returns `EntityId`; mutable lookup changes transforms, removal returns ownership, and iteration or `render_objects()` supplies rendering. Draw requests borrow existing mesh/material resources.

Import covers all indexed triangle primitives of the first glTF mesh, with base-color images/factors and per-material sidedness. Shared image uploads and factor-only white fallback are supported. Nodes, additional meshes, animation/skinning, lighting/full PBR, full editor authoring and scene commands remain outside this scope; initial JSON persistence is now in progress.

## Modes and distribution

| Mode | Selection | Behavior / assets |
| --- | --- | --- |
| Development | No readable `project-name.txt` beside executable | Rotating model instances; assets below the compiled game manifest directory |
| Installed | Name file beside executable | Stationary instances; executable-relative `assets` and supplied window title |

The compiler now recursively copies the complete game asset directory, including `props/robot.glb` and nested resources. The previous missing-model packaging warning is resolved in source and covered by a copy test. Preserve executable, name file, and asset tree together. The generated admin workspace retains its separate source-generation workflow.

## Shutdown and verification

Loaded scene entries retain names and source paths for snapshots; runtime IDs are not serialized. Scene and cache hold uploaded GPU resources and are declared before the renderer so they drop before its device. Game shutdown waits for idle before releasing those owners. `WindowRenderer` also waits during its own destruction and keeps the window alive with an `Arc`. This lifetime rule is enforced by application structure, not automatic backend-independent handles.

The native smoke example uses runtime APIs and exercises textured robots, shared model draws, removal, cache/model release after idle, zero-size suspension, empty-scene resize, and shutdown. User-confirmed tests, smoke, and visible resize/minimize checks completed the five-step runtime migration. Hidden smoke execution checks lifecycle/assertions rather than image correctness. See [testing](../../testing.md), [runtime](../../crates/runtime/README.md), and [renderer](../../crates/renderer/README.md).
