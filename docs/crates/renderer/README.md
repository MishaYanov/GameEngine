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
| `VulkanTexture`, `VulkanMaterial`, `VulkanMaterialLayout` | Texture upload and materials with shared texture ownership and double-sided state |
| `VulkanCamera`, `CameraUniform` | Camera buffer/descriptors |
| `VulkanGraphicsPipeline`, `VulkanMaterialPipelines`, `ModelPushConstants` | Fixed pipeline and object transform |
| `RenderObject`, `VulkanFrame`, `FrameStatus` | Draw inputs, synchronization, rebuild status |

Presented applications use `VulkanInstance::for_display` and `VulkanDevice::for_surface`. The status wrapper's device creation does not replace selection of a surface-compatible device.

## Shared material layout and pipeline contracts

`VulkanMaterialLayout::new(device)` creates the shared descriptor set layout for a fragment combined image sampler at binding 0. Pass an `Arc<VulkanMaterialLayout>` to texture constructors and to the asset manager. Texture descriptor allocation and pipeline creation use this shared layout rather than deriving renderer setup from a scene object.

`VulkanMaterial::new(texture, double_sided, base_color_factor)` owns a shared texture, stores the imported sidedness and RGBA factor, and exposes both properties. `VulkanGraphicsPipeline::new` still takes its final culling boolean: true means no face culling; false means back-face culling.

`VulkanMaterialPipelines::new` creates compatible single- and double-sided pipelines. `for_material(bool)` selects one; `VulkanFrame::draw` selects and binds the matching pipeline for each object. Recreate both variants when attachment formats change. Pipeline/layout creation can occur with an empty scene.

`ModelPushConstants` is an 80-byte block: model matrix at byte 0 and base-color factor at byte 64, shared with vertex/fragment stages. The frame supplies each material's factor, and the fragment shader multiplies sampled texture color by it. Factor-only materials use a white texture supplied by the game asset manager. Full PBR, normal lighting, and transparent-material blend/sort behavior remain unimplemented.

## Shaders and ownership

`build.rs` compiles `crates/renderer/shaders/triangle.vert` and `triangle.frag` into Cargo `OUT_DIR`. It checks `VULKAN_SDK/Bin/glslc.exe` on Windows (`bin/glslc` elsewhere), then PATH. The pipeline embeds SPIR-V through `include_bytes!`; shader changes require rebuilding, with no hot reload.

Wrappers destroy handles in `Drop`, but `ash::Device` clones do not own the logical device's lifetime. Destroy dependent resources before device/instance and finish submitted work before release. See [graphics and assets](../../graphics-and-assets.md) for shader contracts and lifecycle details.

## Verification limits

Tests cover validation diagnostic behavior and the push-constant byte layout. Startup probes and manual frame checks cover different behavior. Drawing, resize/minimize, and visual correctness require a GPU/window. The current textured pipeline is not a general material/scene system. See [testing](../../testing.md) and [game](../../apps/game/README.md).
