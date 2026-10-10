# Graphics and asset development

This guide describes the current demo and renderer contracts. See [renderer](crates/renderer/README.md) for prerequisites and [game](apps/game/README.md) for execution and scene modes.

## Asset loading and caches

The asset layer lives in `apps/game/src/assets`. `ImageAsset::load` opens/decodes an image, converts it to RGBA8, and flips it vertically once to match the demo UV convention. The manifest enables PNG and JPEG decoding. `ImageAsset::from_rgba8(width, height, pixels)` accepts already decoded data, preserves row order, and rejects zero dimensions, size overflow, or a byte count other than `width * height * 4`. glTF images use this constructor without the standalone-file vertical flip.

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

`CameraUniform::perspective` creates camera data. `ModelPushConstants::from_3d` accepts translation, Euler rotation in radians, and scale. `VulkanMaterial::new(texture, double_sided)` owns an `Arc<VulkanTexture>` and stores a double-sided flag. Pass that flag as the final argument to `VulkanGraphicsPipeline::new`: true disables face culling; false enables back-face culling. This choice belongs to the pipeline, not to each draw object independently. `RenderObject` borrows mesh/material and carries the transform.

Change Rust layout, pipeline descriptors, and GLSL together. The current shaders sample color directly without applying imported base-color factor, normal-based lighting, alpha behavior, or full glTF PBR. Sources are `crates/renderer/shaders/triangle.vert` and `triangle.frag`; the renderer build script compiles and embeds their SPIR-V. Rebuild to apply shader changes.

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

## glTF and GLB import

`load_first_primitive(path) -> Result<ImportedPrimitive, GltfLoaderError>` uses `gltf::import` to read geometry and images, including GLB input. The game calls it once during window initialization for `assets/props/robot.glb`, then uploads its mesh and image for rendering.

`ImportedPrimitive` separates CPU geometry and material data:

| Field | Contract |
| --- | --- |
| `mesh: MeshData` | First mesh's first indexed triangle primitive |
| `base_color_image: Option<ImageAsset>` | Decoded base-color image, if referenced |
| `base_color_factor: [f32; 4]` | Imported material factor, currently logged rather than shaded |
| `double_sided: bool` | Currently hardcoded true; not read from the glTF material |

The loader requires positions and indices, rejects non-triangle modes and empty geometry, checks triangle index count and index bounds, and reads the UV set requested by the base-color texture. A textured primitive must have that UV set; an untextured primitive can use zero UVs. Position and UV counts must match.

Imported images support `R8G8B8` (expanded with alpha 255) and `R8G8B8A8`. Other decoded formats are rejected. `ImageAsset::from_rgba8` validates the dimensions/byte count and preserves imported row order. Buffer/image import errors retain their underlying causes through `GltfLoaderError::source`.

### CPU import to GPU material

1. Load the primitive and pass `&imported.mesh` to `AssetManager::load_mesh` with a distinct ID; the robot uses `props/robot.glb#mesh:0#primitive:0`.
2. Upload the optional image with `VulkanTexture::from_rgba8`. The current game requires it and exits if no base-color image exists, even though the loader allows factor-only materials.
3. Share the resulting `Arc<VulkanTexture>` across materials. The demo creates two materials referencing the same texture and two draw objects referencing the same mesh.
4. Construct the pipeline using the material's double-sided flag. The robot's imported texture is uploaded directly, outside the file-texture cache.

There is no node transform traversal, additional primitive selection, complete scene assembly, skinning, animation, normal-based shading, texture sampler import, or full PBR support. The game does not currently fall back to a solid-color texture for factor-only materials. Procedural geometry helpers and standalone texture loading remain available, but are no longer the default demo scene.

A separate `MaterialData` type is being introduced in uncommitted work under `src/assets/material_data.rs`. It currently declares private image/factor/double-sided fields and is not integrated into the importer/rendering flow reviewed here. Treat it as work in progress rather than a public replacement contract.

### Packaging

The game resolves `props/robot.glb` below its selected asset root. The compiler still copies only the old two textures, so installed starters miss the required model. The admin builder allowlist includes `.glb`, `.gltf`, and `.bin` assets; inspect preview for all external resources a model needs. See [game modes and workaround](apps/game/README.md) and [compiler limits](compiler/README.md).

## Troubleshooting and manual checks

Check development robot rendering and installed mode separately (after supplying the missing model). Inspect texture orientation, depth ordering, resize, minimize/restore, and clean closure. Debug validation messages use the shared logger. For startup failures, check Vulkan 1.3 driver support and validation layers; for shader failures, check `glslc`; for missing assets, check mode-specific roots and the install copy list.

A native window/GPU is required to validate visual correctness. See [testing](testing.md).
