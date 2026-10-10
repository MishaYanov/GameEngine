# Graphics and asset development

This guide describes the current demo and renderer contracts. See [renderer](crates/renderer/README.md) for prerequisites and [game](apps/game/README.md) for execution and scene modes.

## Asset loading and caches

The asset layer lives in `apps/game/src/assets`. `ImageAsset::load` opens/decodes an image, converts it to RGBA8, and flips it vertically once to match the demo UV convention. The manifest enables PNG and JPEG decoding.

`AssetManager::new(root)` canonicalizes an existing directory. Texture lookup joins and canonicalizes the path, rejects resolved paths outside the root, and caches `Arc<VulkanTexture>` by canonical filename. Files must exist; symlinks escaping the root are rejected.

`load_mesh(instance, device, id, data)` uploads `MeshData` and caches `Arc<VulkanMesh>` by caller-supplied string ID. Reusing an ID returns the cached mesh even if new data differs. Use distinct stable IDs for distinct geometry. Count and membership methods expose cache state.

`clear()` releases cache references, but materials and other `Arc` holders can keep resources alive. Clearing does not guarantee immediate GPU destruction. Automatic invalidation and hot reload are not implemented.

## Geometry and shader contracts

`MeshData` stores `Vec<Vertex>` and `Vec<u32>` indices, with procedural triangle/cube constructors. `Vertex` has C layout: position `[f32; 3]` at location 0 and UV `[f32; 2]` at location 1. Binding/attribute descriptions derive stride/offsets from this Rust layout.

The vertex shader computes `camera.viewProjection * model * position`.

| Shader input | Binding |
| --- | --- |
| Position / UV | Locations 0 / 1 |
| Camera view-projection | Descriptor set 0, binding 0 |
| Sampled texture | Descriptor set 1, binding 0 |
| Model matrix | Vertex shader push constant |

`CameraUniform::perspective` creates camera data. `ModelPushConstants::from_3d` accepts translation, Euler rotation in radians, and scale. `VulkanMaterial` owns an `Arc<VulkanTexture>`. `RenderObject` borrows mesh/material and carries the transform.

Change Rust layout, pipeline descriptors, and GLSL together. The current shaders sample color directly without normal-based lighting or a glTF material system. Sources are `crates/renderer/shaders/triangle.vert` and `triangle.frag`; the renderer build script compiles and embeds their SPIR-V. Rebuild to apply shader changes.

## Add a texture or procedural mesh

1. Put a PNG/JPEG under `apps/game/assets` and load it with an asset-root-relative path.
2. Create a material from the texture. Supply `MeshData` and a distinct cache ID to `load_mesh` for new geometry.
3. Add a `RenderObject` to drawing inputs in `apps/game/src/main.rs`.
4. For installed starters, update the explicit asset copy list in `compiler/src/lib.rs`. For generated workspaces, verify admin builder preview includes the asset.
5. Check rendering, texture orientation, depth, resize/minimize, shutdown, and validation logs.

The compiler currently copies only `crate.png` and `bricks.jpg`. Adding an asset to development does not automatically include it in installed output.

## GPU lifetime and presentation changes

Keep the instance alive until devices/surfaces are destroyed, and the logical device alive until buffers, textures, descriptors, camera, pipeline, frame, and swapchain resources are destroyed. `ash::Device` clones are handles, not ownership of device lifetime. Release remaining mesh/texture references while the device remains valid.

The game's dependent fields precede device/instance fields so ordinary field destruction respects their dependencies. Its `Drop` waits for GPU idle first. Preserve this order when adding owners; the device destructor itself does not wait for submitted work.

Extent/format changes can invalidate depth, pipeline, and frame resources. Follow `GameApp::recreate_swapchain`: wait for idle, rebuild dependent presentation resources, and defer zero-sized windows. `FrameStatus::SwapchainNeedsRebuild` signals rebuild rather than successful presentation.

## Model import status

The current working tree contains a `gltf` dependency and `gltf_loader.rs` placeholder with an unfinished return type/body. The asset module declares it, so game builds fail while that placeholder remains included. It is not a functional importer. Scenes, animations, and glTF materials are not supported merely by adding the dependency. Complete the loader/data/error contracts before introducing import workflows.

## Troubleshooting and manual checks

Check development cubes and installed triangles separately. Inspect texture orientation, depth ordering, resize, minimize/restore, and clean closure. Debug validation messages use the shared logger. For startup failures, check Vulkan 1.3 driver support and validation layers; for shader failures, check `glslc`; for missing assets, check mode-specific roots and the install copy list.

A native window/GPU is required to validate visual correctness. See [testing](testing.md).
