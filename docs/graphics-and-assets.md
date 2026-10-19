# Graphics and asset development

This guide describes the current demo and renderer contracts. See [renderer](crates/renderer/README.md) for prerequisites and [game](apps/game/README.md) for execution and scene modes.

## Asset loading and caches

The asset layer lives in `apps/game/src/assets`. `ImageAsset::load` opens/decodes an image, converts it to RGBA8, and flips it vertically once to match the demo UV convention. The manifest enables PNG and JPEG decoding. `ImageAsset::from_rgba8(width, height, pixels)` accepts already decoded data, preserves row order, and rejects zero dimensions, size overflow, or a byte count other than `width * height * 4`. glTF images use this constructor without the standalone-file vertical flip.

`AssetManager::new(root, Arc<VulkanMaterialLayout>)` canonicalizes an existing directory. Texture lookup joins and canonicalizes the path, rejects resolved paths outside the root, and caches `Arc<VulkanTexture>` by canonical filename. Files must exist; symlinks escaping the root are rejected.

`load_mesh(instance, device, id, data)` uploads `MeshData` and caches `Arc<VulkanMesh>` by caller-supplied string ID. Reusing an ID returns the cached mesh even if new data differs. Use distinct stable IDs for distinct geometry. Count and membership methods expose cache state.

`clear()` releases cache references, but materials and other `Arc` holders can keep resources alive. Clearing does not guarantee immediate GPU destruction. Automatic invalidation and hot reload are not implemented.

## Geometry and shader contracts

`MeshData` stores `Vec<Vertex>` and `Vec<u32>` indices, with procedural triangle/cube constructors. `Vertex` has C layout: position `[f32; 3]` at location 0 and UV `[f32; 2]` at location 1. Binding/attribute descriptions derive stride/offsets from this Rust layout.

The vertex shader computes `camera.viewProjection * model * position`; the fragment shader multiplies the sampled texture by the material base-color factor.

| Shader input | Binding |
| --- | --- |
| Position / UV | Locations 0 / 1 |
| Camera view-projection | Descriptor set 0, binding 0 |
| Sampled texture | Descriptor set 1, binding 0 |
| Model matrix and base-color factor | 80-byte push constant block shared by vertex/fragment stages |

`CameraUniform::perspective` creates camera data. `ModelPushConstants::from_3d` accepts translation, Euler rotation in radians, and scale. The block contains the matrix at byte 0 and material factor at byte 64. `VulkanMaterial::new(texture, double_sided, base_color_factor)` owns a shared texture and material properties. `VulkanMaterialPipelines` supplies matching single/double-sided variants; frame drawing chooses a variant per material.

A renderer-owned `VulkanMaterialLayout` is shared by textures, the asset manager, and pipelines. Constructors for GPU textures take that shared layout. Pipeline setup does not depend on a scene object, permitting an empty scene. Preserve the layout and logical device until dependent descriptor resources are released.

Change Rust layout, pipeline descriptors, and GLSL together. The shaders apply base-color factors, but provide no normal lighting, full PBR, or transparent-material blending/sorting. Shader sources remain `crates/renderer/shaders/triangle.vert` and `triangle.frag`; rebuilding compiles and embeds SPIR-V.

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

`load_first_mesh(path) -> Result<Vec<ImportedPrimitive>, GltfLoaderError>` imports every primitive of the first mesh through `gltf::import`. Any unsupported primitive rejects the import rather than being silently skipped. Returned geometry stays in mesh-local coordinates: nodes/transforms, other meshes, animations, and skinning are not traversed.

Each `ImportedPrimitive` contains `MeshData`, mesh/primitive indices, an optional source image index, and crate-local `MaterialData`. Material data contains an optional base-color image, RGBA factor, and the actual glTF `doubleSided` flag.

Positions and indices are required. The loader rejects non-triangle modes, empty geometry, invalid index count/bounds, and mismatched position/UV counts. Textured materials require the referenced UV set; untextured materials can use zero UVs. Images support decoded RGB8 (expanded to RGBA8) and RGBA8; other formats are rejected. Imported row order is preserved. Import/image errors expose underlying causes.

### Models, instances, and GPU caches

`AssetManager::load_model` resolves a path inside its canonical root, reimports CPU data, and returns a `Model` containing one `ModelPart` per primitive. GPU meshes are cached by canonical path plus mesh/primitive indices; imported images are cached by canonical path plus image index. Repeated model loads reuse uploads, not the CPU import or the returned model object.

`load_image_texture` caches decoded image uploads by caller ID. `load_material` uses the imported image or shared `builtin:white` 1×1 texture, enabling factor-only materials. Its `VulkanMaterial` stores factor/sidedness independently even when textures are shared. `loaded_texture_count` includes both file and imported-image caches. Reused IDs keep existing uploads; there is no hot reload/invalidation.

`ModelPart` owns an `Arc<VulkanMesh>` and material; `Model` groups parts. `ModelInstance` owns `Arc<Model>` and independent position/rotation/scale. `Scene` owns instances under increasing entity IDs, with spawn, mutable lookup, removal, and iteration. These types live in the game, not shared runtime.

The game loads the robot model once, shares it between two instances, and expands each instance's parts into `RenderObject` inputs per frame. `clear()` releases cache references; scene/model/material owners can still retain GPU resources. Release those owners before destroying their device. No complete scene import, persistence, ECS, or editor entity commands are provided by this model layer.

### Packaging

The game resolves `props/robot.glb` below its selected asset root. The compiler still copies only the old two textures, so installed starters miss the required model. The admin builder allowlist includes `.glb`, `.gltf`, and `.bin` assets; inspect preview for all external resources a model needs. See [game modes and workaround](apps/game/README.md) and [compiler limits](compiler/README.md).

## Troubleshooting and manual checks

Check development robot rendering and installed mode separately (after supplying the missing model). Inspect texture orientation, depth ordering, resize, minimize/restore, and clean closure. Debug validation messages use the shared logger. For startup failures, check Vulkan 1.3 driver support and validation layers; for shader failures, check `glslc`; for missing assets, check mode-specific roots and the install copy list.

A native window/GPU is required to validate visual correctness. See [testing](testing.md).
