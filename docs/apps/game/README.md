# Native game demo

Source: `apps/game`. Cargo package: `game`. This application owns a winit window/event loop and directly uses `renderer::vulkan` for drawing.

## Run

From the repository root, after [setup](../../usage.md):

```powershell
cargo run -p game
.\dev.ps1 game
cargo run -p game --release
```

Requires Vulkan 1.3 and a working GPU driver; builds require `glslc`, and debug startup requires the Khronos validation layer. The current working tree contains an unfinished glTF module; builds including that placeholder fail until it is completed or excluded.

## Scene modes

| Mode | Selection | Geometry | Asset root |
| --- | --- | --- | --- |
| Development | No readable `project-name.txt` beside executable | Two rotating textured cubes | `apps/game/assets` from the compiled manifest directory |
| Installed starter | Readable name file beside executable | Two stationary textured triangles | `assets` beside executable |

The installed name supplies the title. Both objects share one GPU mesh and use separate materials from `textures/crate.png` and `textures/bricks.jpg`. Keep executable, name file, and assets together. The admin builder adjusts asset lookup in generated workspaces; preserve that generated layout.

## Lifecycle and extension

`GameApp` owns instance, surface, presentation device, swapchain, depth buffer, assets, camera, pipeline, and frame resources. Redraw updates the perspective camera/transforms and calls `VulkanFrame::draw`; each successful iteration requests another redraw. Heartbeats are emitted on redraw roughly every five seconds.

Resize and `FrameStatus::SwapchainNeedsRebuild` mark presentation dirty. Zero-sized windows defer rendering. Rebuild waits for idle and recreates presentation resources. Drawing/rebuild errors are logged and exit. Closing the window exits normally; `GameApp::drop` waits for GPU work before fields are destroyed.

Change scene setup in `src/main.rs` and assets under `src/assets`. `MeshData` provides procedural geometry and `AssetManager` caches GPU resources. See [graphics and assets](../../graphics-and-assets.md) before changing shader layouts or ownership.

There is no scene format, scene editor, or functional model importer. The game currently has no Rust behavioral tests; verify rendering, resize/minimize, and shutdown manually. See [testing](../../testing.md).
