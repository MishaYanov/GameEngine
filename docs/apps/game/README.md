# Native game demo

Source: `apps/game`. Cargo package: `game`. This application owns a winit window/event loop and directly uses `renderer::vulkan` for drawing.

## Run

From the repository root, after [setup](../../usage.md):

```powershell
cargo run -p game
.\dev.ps1 game
cargo run -p game --release
```

Requires Vulkan 1.3 and a working GPU driver; builds require `glslc`, and debug startup requires the Khronos validation layer.

## Scene modes

| Mode | Selection | Geometry | Asset root |
| --- | --- | --- | --- |
| Development | No readable `project-name.txt` beside executable | Two rotating instances of the first robot primitive | `apps/game/assets` from the compiled manifest directory |
| Installed starter | Readable name file beside executable | Two stationary instances of the imported primitive, if its asset is present | `assets` beside executable |

The installed name supplies the title. Both modes now import `props/robot.glb` once during `GameApp::resumed`, upload its first mesh/primitive, and draw two instances sharing the same mesh and base-color GPU texture. Two materials reference that texture. Development rotates the objects; a name file disables rotation.

The loader returns mesh data, optional base-color image, color factor, and a double-sided flag. The game requires an image and exits on import/image/upload failures. The factor is logged but is not applied by the shader. The loader currently returns `double_sided: true`; the pipeline disables culling for that setting.

**Installed starter limitation:** the compiler still copies only `crate.png` and `bricks.jpg`, not `props/robot.glb`. A freshly installed starter therefore cannot render with the current game code: import reports the missing model and exits. Copying the required model to `<installed folder>/assets/props/robot.glb` is a manual workaround; fixing the installer is separate implementation work. Preserve any external files referenced by other glTF models.

The admin builder includes `.glb`/`.gltf` assets and adjusts asset lookup in generated workspaces. Preserve the generated layout. See [model import details](../../graphics-and-assets.md).

## Lifecycle and extension

`GameApp` owns instance, surface, presentation device, swapchain, depth buffer, assets, camera, pipeline, and frame resources. Redraw updates the perspective camera/transforms and calls `VulkanFrame::draw`; each successful iteration requests another redraw. Heartbeats are emitted on redraw roughly every five seconds.

Resize and `FrameStatus::SwapchainNeedsRebuild` mark presentation dirty. Zero-sized windows defer rendering. Rebuild waits for idle and recreates presentation resources. Drawing/rebuild errors are logged and exit. Closing the window exits normally; `GameApp::drop` waits for GPU work before fields are destroyed.

Change scene setup in `src/main.rs` and assets under `src/assets`. `MeshData` provides procedural geometry and `AssetManager` caches GPU resources. See [graphics and assets](../../graphics-and-assets.md) before changing shader layouts or ownership.

The importer handles one indexed triangle primitive, not a complete glTF scene. Node transforms, additional primitives, animation, and full PBR are not implemented. The game currently has no Rust behavioral tests; verify rendering, resize/minimize, and shutdown manually. See [testing](../../testing.md).
