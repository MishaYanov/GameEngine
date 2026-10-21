# Graphics and asset development

This guide describes the current demo and renderer contracts. See [renderer](crates/renderer/README.md) for prerequisites and [game](apps/game/README.md) for execution and scene modes.

## Asset loading and caches

The asset layer lives in `crates/runtime/src/assets`. `ImageAsset::load` opens/decodes an image, converts it to RGBA8, and flips it vertically once to match the demo UV convention. The manifest enables PNG and JPEG decoding. `ImageAsset::from_rgba8(width, height, pixels)` accepts already decoded data, preserves row order, and rejects zero dimensions, size overflow, or a byte count other than `width * height * 4`. glTF images use this constructor without the standalone-file vertical flip.

`AssetManager::new(root)` canonicalizes an existing directory. Texture lookup joins and canonicalizes the path, rejects resolved paths outside the root, and caches `Arc<VulkanTexture>` by canonical filename. Files must exist; symlinks escaping the root are rejected.

`load_mesh(renderer, id, data)` uploads `MeshData` and caches `Arc<VulkanMesh>` by caller-supplied string ID. Reusing an ID returns the cached mesh even if new data differs. Use distinct stable IDs for distinct geometry. Count and membership methods expose cache state.

`clear()` releases cache references, but materials and other `Arc` holders can keep resources alive. Clearing does not guarantee immediate GPU destruction. Automatic invalidation and hot reload are not implemented.

## Geometry and shader contracts

`MeshData` stores `Vec<Vertex>` and `Vec<u32>` indices, with procedural triangle, cube, plane, polygon, sphere, and capsule constructors. `Vertex` has C layout: position `[f32; 3]` at location 0 and UV `[f32; 2]` at location 1. Binding/attribute descriptions derive stride/offsets from this Rust layout.

The vertex shader computes `camera.viewProjection * model * position`; the fragment shader multiplies the sampled texture by the material base-color factor.

| Shader input | Binding |
| --- | --- |
| Position / UV | Locations 0 / 1 |
| Camera view-projection | Descriptor set 0, binding 0 |
| Sampled texture | Descriptor set 1, binding 0 |
| Model matrix and base-color factor | 80-byte push constant block shared by vertex/fragment stages |

`CameraUniform::perspective` creates camera data. `ModelPushConstants::from_3d` accepts translation, Euler rotation in radians, and scale. The block contains the matrix at byte 0 and material factor at byte 64. `VulkanMaterial::new(texture, double_sided, base_color_factor)` owns a shared texture and material properties. `VulkanMaterialPipelines` supplies matching single/double-sided variants; frame drawing chooses a variant per material.

A `WindowRenderer`-owned `VulkanMaterialLayout` is shared by uploaded textures and pipelines. Runtime asset methods receive `&WindowRenderer` and call `upload_mesh` / `upload_rgba8`; they no longer receive a raw Vulkan instance/device or own a separate material-layout reference. Pipeline setup does not depend on a scene object, permitting an empty scene. Preserve the layout and logical device until dependent descriptor resources are released.

Change Rust layout, pipeline descriptors, and GLSL together. The shaders apply base-color factors, but provide no normal lighting, full PBR, or transparent-material blending/sorting. Shader sources remain `crates/renderer/shaders/triangle.vert` and `triangle.frag`; rebuilding compiles and embeds SPIR-V.

## Procedural primitives — in progress

The working tree extracts shape builders into private modules under `crates/runtime/src/assets/primitives`. Use the public `runtime::assets::MeshData` constructors, rather than importing those implementation modules. These builders create CPU vertex/index data; GPU upload still goes through `AssetManager::load_mesh(&renderer, id, &data)`.

| Constructor | Current geometry and dimensions |
| --- | --- |
| `MeshData::triangle()` | XY triangle at Z=0; three vertices and two opposite-winding index triangles, preserving the existing two-sided geometry |
| `MeshData::cube()` | Unit cube centered at the origin, spanning ±0.5 on each axis; separate face vertices/UVs |
| `MeshData::plane()` | Unit XZ plane at Y=0, spanning ±0.5 in X/Z; four vertices, two triangles facing +Y |
| `MeshData::polygon(sides)` | Regular XY polygon at Z=0 with radius 0.5, center-fan triangulation, facing +Z; requires at least three sides |
| `MeshData::sphere()` | Radius-0.5 rounded mesh centered at the origin, with Y-axis poles |
| `MeshData::capsule()` | Y-aligned capsule with radius 0.5 and cylinder half-height 0.5: total height 2, width/depth 1 |

`polygon(sides)` panics when `sides < 3`; it does not return a validation error. Sphere and capsule currently have fixed resolution: 32 angular segments and eight rings per hemisphere. Their shared `rounded.rs` builder duplicates the UV seam, makes pole positions exact, and skips degenerate pole triangles. Radius, capsule height, and tessellation are not public constructor parameters yet.

```rust
use runtime::assets::MeshData;
let ground = MeshData::plane();
let marker = MeshData::polygon(6);
let ball = MeshData::sphere();
let character_shape = MeshData::capsule();
```

Vertices currently contain position and UV only: normals, tangents, lighting-ready attributes, collision shapes, and mesh LOD are not supplied. Each constructor creates fresh CPU data; use distinct stable upload IDs for different shapes/parameters because an existing cache ID returns the old GPU mesh.

To render, upload the data, pair it with a material in a ModelPart/Model, and spawn a ModelInstance. This does not automatically add primitive entities to the robot demo or give them serialized identifiers. Current scene persistence expects a source `model_path`; procedural meshes have no primitive-specific JSON schema or save/load workflow yet.

The focused primitive test checks cube, plane, polygon(5), sphere, and capsule for finite positions, bounded UVs, and nondegenerate triangles; rounded shapes additionally check outward orientation. A separate should-panic test covers invalid polygon side count. The geometry loop does not include the legacy opposite-winding triangle. Native visual verification of the new shapes has not been performed in this docs review.

## Add a texture or procedural mesh

1. Put a PNG/JPEG under `apps/game/assets` and load it with an asset-root-relative path.
2. Create a material from the texture. Supply `MeshData` and a distinct cache ID to `load_mesh` for new geometry.
3. Put geometry/material parts in a runtime `Model`, spawn a `ModelInstance` in `Scene`, and let `Scene::render_objects()` create borrowed draw requests.
4. Installed starters receive the complete asset tree through recursive copying in `compiler/src/lib.rs`. For generated workspaces, verify admin builder preview includes the asset.
5. Check rendering, texture orientation, depth, resize/minimize, shutdown, and validation logs.

The compiler copies nested game assets, including `props/robot.glb`; keep referenced resources in that tree. Admin source generation still uses an allowlist, so inspect its preview separately.

## GPU lifetime and presentation changes

Keep the instance alive until devices/surfaces are destroyed, and the logical device alive until buffers, textures, descriptors, camera, pipeline, frame, and swapchain resources are destroyed. `ash::Device` clones are handles, not ownership of device lifetime. Release remaining mesh/texture references while the device remains valid.

The game declares scene/cache before `WindowRenderer`, waits for idle before releasing external uploads, and then drops the renderer. The owner groups internal render targets ahead of device/instance/window dependencies and also waits during destruction. Preserve both external and internal lifetime ordering; the types do not enforce every misuse.

Extent/format changes can invalidate depth, pipeline, and frame resources. `WindowRenderer::resize` marks targets dirty; `prepare` waits for idle, takes old targets out, and rebuilds replacements. A zero dimension returns `None`. `draw` exposes `DrawStatus::{Rendered, Suspended, NeedsRedraw}`; the application no longer recreates the swapchain itself.

## glTF and GLB import

`load_first_mesh(path) -> Result<Vec<ImportedPrimitive>, GltfLoaderError>` imports every primitive of the first mesh through `gltf::import`. Any unsupported primitive rejects the import rather than being silently skipped. Returned geometry stays in mesh-local coordinates: nodes/transforms, other meshes, animations, and skinning are not traversed.

Each `ImportedPrimitive` contains `MeshData`, mesh/primitive indices, an optional source image index, and public `MaterialData`. Material data contains an optional base-color image, RGBA factor, and the actual glTF `doubleSided` flag.

Positions and indices are required. The loader rejects non-triangle modes, empty geometry, invalid index count/bounds, and mismatched position/UV counts. Textured materials require the referenced UV set; untextured materials can use zero UVs. Images support decoded RGB8 (expanded to RGBA8) and RGBA8; other formats are rejected. Imported row order is preserved. Import/image errors expose underlying causes.

### Models, instances, and GPU caches

`AssetManager::load_model` resolves a path inside its canonical root, reimports CPU data, and returns a `Model` containing one `ModelPart` per primitive. GPU meshes are cached by canonical path plus mesh/primitive indices; imported images are cached by canonical path plus image index. Repeated model loads reuse uploads, not the CPU import or the returned model object.

`load_image_texture` caches decoded image uploads by caller ID. `load_material` uses the imported image or shared `builtin:white` 1×1 texture, enabling factor-only materials. Its `VulkanMaterial` stores factor/sidedness independently even when textures are shared. `loaded_texture_count` includes both file and imported-image caches. Reused IDs keep existing uploads; there is no hot reload/invalidation.

`ModelPart` owns an `Arc<VulkanMesh>` and material; `Model` groups parts. `ModelInstance` owns `Arc<Model>` and independent position/rotation/scale. `Scene` owns instances under increasing entity IDs, with spawn, mutable lookup, removal, and iteration. These types now live in `runtime`; `Scene::render_objects()` handles instance-to-part draw conversion. Requests borrow existing resources, so release them before mutating the scene. Models still hold Vulkan-specific resources, so the API is not backend-independent.

The game loads the robot model once, shares it between two instances, and asks the runtime scene for `RenderObject` inputs per frame. `clear()` releases cache references; scene/model/material owners can still retain GPU resources. Release those owners before destroying their device. No complete glTF scene import, ECS, or editor entity commands are provided by this model layer. Initial JSON scene persistence now records model paths, names, transforms, and camera via runtime scene_data/scene_io; see [scene persistence](scene-persistence.md).

### Packaging

The game resolves `props/robot.glb` below its selected asset root. The compiler now recursively copies the full asset tree, fixing the previous missing-model issue. The admin builder allowlist includes `.glb`, `.gltf`, and `.bin` assets; inspect preview for all external resources a model needs. See [game modes and packaging](apps/game/README.md) and [compiler limits](compiler/README.md).

## Troubleshooting and manual checks

Check development robot rendering and installed mode separately with its packaged asset tree. Inspect texture orientation, depth ordering, resize, minimize/restore, and clean closure. Debug validation messages use the shared logger. For startup failures, check Vulkan 1.3 driver support and validation layers; for shader failures, check `glslc`; for missing assets, check mode-specific roots and the install copy list.

A native window/GPU is required to validate visual correctness. See [testing](testing.md).
