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

## Presentation owner: WindowRenderer

`renderer::WindowRenderer` owns frame resources, both pipeline variants, depth buffer, swapchain, camera, material layout, device, surface, instance, and an `Arc` keeping the native window alive.

| Method | Contract |
| --- | --- |
| `new(window, width, height)` | Create resources for an `Arc` window with display/window handles |
| `resize(width, height)` | Record dimensions and mark targets dirty; no immediate rebuild |
| `prepare()` | Rebuild if needed; return actual extent or `None` when either dimension is zero |
| `draw(camera, objects)` | Submit borrowed requests; return `DrawStatus` |
| `upload_mesh(vertices, indices)` | Upload GPU geometry |
| `upload_rgba8(width, height, pixels)` | Upload a texture using the owner's shared material layout |
| `wait_idle()` | Wait before releasing external uploaded resources |

`DrawStatus` is `Rendered`, `Suspended`, or `NeedsRedraw`. Swapchain invalidation marks targets dirty for a later prepare. Internal `RenderTargets` groups frame, pipelines, depth, and swapchain. Rebuild waits for idle and takes old targets out before replacement, preventing reuse of a retired swapchain after a failure.

External meshes/textures must be released after GPU idle and before this owner/device. The renderer's destructor waits for idle but cannot enforce the lifetime of externally held resources. Runtime assets still contain Vulkan types.

## Presentation policy

The current swapchain selects Vulkan FIFO presentation rather than preferring MAILBOX. FIFO is display-refresh paced and
avoids rendering mailbox replacements that are never presented. This policy change does not establish a fixed-step
simulation or a measured frame-time guarantee; verify pacing on the target native system.

## Status and low-level APIs

The existing `Renderer::new()` / `Renderer::info()` wrapper remains for editor/runtime GPU status and has not been unified with window presentation. Public `vulkan` types remain available for lower-level work; applications should use the presentation owner for upload, draw, resize, and shutdown.

`VulkanMaterialLayout` is renderer-owned and shared with uploaded textures.
`VulkanMaterial::new(texture, double_sided, base_color_factor)` retains material properties. `VulkanMaterialPipelines`
selects compatible single/double-sided variants per draw. `ModelPushConstants` contains a matrix at byte 0, factor at
byte 64, and flags at byte 80 (96 bytes total); sampled color is multiplied by the factor. Basic directional lighting
now uses imported/generated normals with inverse-transpose transformation and a fixed ambient/diffuse shader. Full PBR,
shadows, and transparent-material blending/sorting remain future work.

## Normals, overlays, and dynamic meshes

Vertex stride is 32 bytes: position offset 0/location 0, UV offset 12/location 1, normal offset 20/location 2. Push
constants are 96 bytes: model 0, base-color factor 64, flags 80. Draw flags select
unlit/screen-space/vertex-color/depth-free overlay paths. These attributes also carry font/handle color in specialized
draws, so Rust layout and both shaders must be updated together.

The fixed light uses normalized direction [-0.4, 0.8, 0.6], ambient 0.25 and diffuse 0.75. Selection/gizmo/UI bypass
scene lighting. Depth-free overlay pipelines support panels and handles. `WindowRenderer::create_dynamic_mesh` allocates
reusable geometry capacity; `write_dynamic_mesh` waits for frame fence completion before writing host-coherent buffers.
Native panels/gizmos each batch geometry into one mesh; the selection box is cached once.

## Shaders and verification

`build.rs` compiles `crates/renderer/shaders/triangle.vert` and `triangle.frag` into `OUT_DIR`, using SDK `glslc` then PATH. The pipeline embeds SPIR-V; rebuild for shader changes.

Unit tests cover validation logging, device/surface selection rules, swapchain choices, and push-constant layout. Run the [native game smoke](../../apps/game/README.md) for upload/draw, resize, empty-scene, and shutdown behavior. The user confirmed native and visible-game checks after the runtime move; those results are distinct from automated checks performed during a docs update. See [testing](../../testing.md).

The steps 11–20 handoff includes passing explicit native initialization and two automated hidden-window smoke examples;
no visible panel/lighting acceptance or GPU-performance result is claimed.
See [the report](../../editor-steps-11-20.md).
