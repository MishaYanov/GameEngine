# Vulkan renderer

Source: `crates/renderer`. Cargo package: `renderer`. The crate supplies a renderer initialization/status wrapper and low-level Vulkan resource types used by the game.

## Requirements and probe

The instance requests Vulkan 1.3. Install a capable GPU driver and SDK with `glslc`. Debug builds enable `VK_LAYER_KHRONOS_validation` and fail startup if it is missing; release builds disable that policy.

From the repository root:

```powershell
cargo run -p renderer --example vulkan_probe
.\dev.ps1 vulkan-probe
cargo test -p renderer --offline --locked
```

The probe exercises initialization/device information rather than a presented scene. Use the native game for manual imported-model and frame checks.

## API levels

`Renderer::new() -> Result<Renderer, RendererError>` creates an instance/device without a window surface. `Renderer::info()` returns `{ name, gpu_name, vulkan_api }`. Initialization errors preserve instance/device causes. Runtime uses this wrapper for editor status.

The public `vulkan` exports provide drawing resources:

| Types | Role |
| --- | --- |
| `VulkanInstance`, `VulkanSurface` | Loader/instance and window surface |
| `VulkanDevice`, `QueueFamilies` | Device and graphics/presentation queues |
| `VulkanSwapchain`, `VulkanDepthBuffer` | Presentation images and depth |
| `VulkanBuffer`, `VulkanMesh`, `Vertex` | Allocations and indexed geometry |
| `VulkanTexture`, `VulkanMaterial` | Texture upload and materials with shared texture ownership and double-sided state |
| `VulkanCamera`, `CameraUniform` | Camera buffer/descriptors |
| `VulkanGraphicsPipeline`, `ModelPushConstants` | Fixed pipeline and object transform |
| `RenderObject`, `VulkanFrame`, `FrameStatus` | Draw inputs, synchronization, rebuild status |

Presented applications use `VulkanInstance::for_display` and `VulkanDevice::for_surface`. The status wrapper's device creation does not replace selection of a surface-compatible device.

## Material culling contract

`VulkanMaterial::new(texture: Arc<VulkanTexture>, double_sided: bool)` stores texture ownership and exposes `double_sided()`. `VulkanGraphicsPipeline::new` now accepts a final `double_sided: bool` parameter: true selects no face culling, false selects back-face culling. This is pipeline state, not a per-object dynamic switch; build a matching pipeline for the materials being drawn.

Both demo materials share the imported robot texture. The loader currently hardcodes double-sided true rather than reading the glTF material flag. The fragment shader samples texture color directly; imported base-color factor, alpha handling, and full PBR are not implemented.

## Shaders and ownership

`build.rs` compiles `crates/renderer/shaders/triangle.vert` and `triangle.frag` into Cargo `OUT_DIR`. It checks `VULKAN_SDK/Bin/glslc.exe` on Windows (`bin/glslc` elsewhere), then PATH. The pipeline embeds SPIR-V through `include_bytes!`; shader changes require rebuilding, with no hot reload.

Wrappers destroy handles in `Drop`, but `ash::Device` clones do not own the logical device's lifetime. Destroy dependent resources before device/instance and finish submitted work before release. See [graphics and assets](../../graphics-and-assets.md) for shader contracts and lifecycle details.

## Verification limits

Tests cover validation diagnostic behavior. Startup probes and manual frame checks cover different behavior. Drawing, resize/minimize, and visual correctness require a GPU/window. The current textured pipeline is not a general material/scene system. See [testing](../../testing.md) and [game](../../apps/game/README.md).
