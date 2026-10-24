# Graphics and asset development

This guide describes the current demo and renderer contracts. See [renderer](crates/renderer/README.md) for prerequisites and [game](apps/game/README.md) for execution and scene modes.

## Asset loading and caches

The asset layer lives in `crates/runtime/src/assets`. `ImageAsset::load` opens/decodes an image, converts it to RGBA8, and flips it vertically once to match the demo UV convention. The manifest enables PNG and JPEG decoding. `ImageAsset::from_rgba8(width, height, pixels)` accepts already decoded data, preserves row order, and rejects zero dimensions, size overflow, or a byte count other than `width * height * 4`. glTF images use this constructor without the standalone-file vertical flip.

`AssetManager::new(root)` canonicalizes an existing directory. Texture lookup joins and canonicalizes the path, rejects resolved paths outside the root, and caches `Arc<VulkanTexture>` by canonical filename. Files must exist; symlinks escaping the root are rejected.

`load_mesh(renderer, id, data)` uploads `MeshData` and caches `Arc<VulkanMesh>` by caller-supplied string ID. Reusing an ID returns the cached mesh even if new data differs. Use distinct stable IDs for distinct geometry. Count and membership methods expose cache state.

`clear()` releases cache references, but materials and other `Arc` holders can keep resources alive. Clearing does not
guarantee immediate GPU destruction. Explicit cache clearing/pruning and scene reconciliation exist; this is not
automatic file hot reload.

## Geometry and shader contracts

Vertex remains a 32-byte C layout: position at offset0/location0, UV offset12/location1, normal offset20/location2.
InstanceData adds an 80-byte matrix+tint record at binding1/locations3–7. ModelPushConstants remains96bytes, with
model0, factor64 and flags80. Instanced input is a frame-batcher flag; unlit/font/vertex-color/depth-free paths remain
separate.

| Uniform/resource | Binding                                         |
|------------------|-------------------------------------------------|
| Camera matrix    | Set0/binding0                                   |
| LightingUniform  | Set0/binding1; up to16 directional/point lights |
| Material sampler | Set1/binding0                                   |

Transforms come from local/world scene hierarchy or per-instance batch data. Entity tint combines with source material
color; normals use inverse transpose with a collapsed-transform fallback. Scene lights replace the former single
hardcoded directional source, while migration supplies compatible defaults for old data. Lighting remains basic, without
PBR/shadows or all transparency/material extensions.

Rust layout, vertex attributes, descriptor interfaces and both shaders must change together. Shader sources remain
crates/renderer/shaders/triangle.vert and triangle.frag, compiled/embedded during rebuild. Overlay shader attribute
reuse does not imply every draw is a lit scene object.

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

Vertices now include normals. MeshData generates area-weighted normals for missing/zero attributes, with +Z fallback
when no usable sum exists. Split face vertices retain hard edges; imported NORMAL data is checked for finite/nonzero
values and normalized. Tangents, collision shapes, and LOD are not supplied. Each constructor creates fresh CPU data;
use distinct stable upload IDs for different shapes/parameters because an existing cache ID returns the old GPU mesh.

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

## glTF and GLB static import

The legacy load_first_mesh API remains distinct from selected static-scene import. The new scene_import path chooses a
glTF default scene or first available scene, visits nodes/parent links, preserves local transforms and selected meshes,
and represents transform-only nodes as nonrenderable scene entities. Shared meshes/materials are reused. SceneData
mesh_index distinguishes source meshes.

Static import rejects unsupported skins/morph behavior and does not provide animation playback. Geometry/image/resource
budgets, containment checks and graph validation apply. Selected unique meshes have a 256 MiB decoded-geometry preflight
limit including expanded indices. AssetManager caps logical GPU resources at 512 MiB and 8,192 resources, accounting for
externally retained references through a weak-reference ledger. Scene draw expansion is capped at 65,536 render objects
through fallible production APIs. These limits do not measure total driver memory. Full static node/mesh traversal must
not be described as full dynamic glTF support, every material extension, or unrestricted file loading.

Positions/indices, normals, UV and image validation remain part of mesh import. Imported normals are finite/nonzero and
normalized; missing normals use area-weighted generation. Source images support decoded RGB8/RGBA8 and preserve imported
row order. Material overrides are optional per-entity tint data rather than destructive edits to shared textures.
See [steps 29–31](editor-steps-21-35.md#step-29-static-gltf-scenes-nodes-and-meshes).

### Models, instances, and GPU caches

`AssetManager::load_model` resolves a path inside its canonical root, reimports CPU data, and returns a `Model`
containing one `ModelPart` per primitive. GPU meshes are cached by canonical path plus mesh/primitive indices; imported
images are cached by canonical path plus image index. AssetManager now also caches static-scene metadata/selected mesh
models and prunes/reconciles reuse. Cache identity is not a guarantee of automatic file reload; a project refresh/reload
is explicit.

`load_image_texture` caches decoded image uploads by caller ID. `load_material` uses the imported image or shared `builtin:white` 1×1 texture, enabling factor-only materials. Its `VulkanMaterial` stores factor/sidedness independently even when textures are shared. `loaded_texture_count` includes both file and imported-image caches. Reused IDs keep existing uploads; there is no hot reload/invalidation.

`ModelPart` owns an `Arc<VulkanMesh>` and material; `Model` groups parts. `ModelInstance` owns `Arc<Model>` and independent position/rotation/scale. `Scene` owns instances under increasing entity IDs, with spawn, mutable lookup, removal, and iteration. These types now live in `runtime`; `Scene::render_objects()` handles instance-to-part draw conversion. Requests borrow existing resources, so release them before mutating the scene. Models still hold Vulkan-specific resources, so the API is not backend-independent.

The game loads the robot model once, shares it between two instances, and asks the runtime scene for `RenderObject`
inputs per frame. `clear()` releases cache references; scene/model/material owners can still retain GPU resources.
Release those owners before destroying their device. No complete glTF scene import or ECS is provided by this model
layer. Generic editing now uses SceneEditor above it. Initial JSON scene persistence now records model paths, names,
transforms, and camera via runtime scene_data/scene_io; see [scene persistence](scene-persistence.md).

### Packaging

Compiler and admin source generation include validated asset JSON with supported model/image files, fixing the missing
scene JSON omission. Traversal, entry count, depth, per-file/total bytes and JSON content are checked; links and
unsafe/unsupported files are rejected. Package JSON has a16MiB ceiling even though the scene parser accepts up to32MiB,
so larger valid documents may still require splitting to meet distribution bounds.

Keep external model dependencies under the asset root and inspect source-generation preview. Successful copying or
generation compilation is not an installed-runtime acceptance test.
See [packaging step22](editor-steps-21-35.md#step-22-json-aware-packaging).

## Troubleshooting and manual checks

Check development robot rendering and installed mode separately with its packaged asset tree. Inspect texture orientation, depth ordering, resize, minimize/restore, and clean closure. Debug validation messages use the shared logger. For startup failures, check Vulkan 1.3 driver support and validation layers; for shader failures, check `glslc`; for missing assets, check mode-specific roots and the install copy list.

A native window/GPU is required to validate visual correctness. See [testing](testing.md).

## Bounds, discovery, and visibility

AssetManager computes cached Model bounds from imported CPU positions. Picking uses inverse-transform slab intersections
and a near/far Vulkan camera ray; selection boxes reuse one GPU mesh. The catalog scans sorted GLB/glTF paths with
root/link/depth/entry limits. Repeated panel placements cache and share a template model.

Scene::render_visible uses transformed model AABBs and six Vulkan clip planes, retaining unknown/invalid bounds
conservatively. F7 toggles culling. Draw and triangle totals include enabled selection/gizmo/panel overlays; wall FRAME
and CPU UPDATE averages are not GPU profiling. UI/gizmo geometry is batched into reused host-coherent buffers with fence
completion before updates; this avoids per-frame GPU allocation, not every CPU allocation. See
the [steps 11–20 report](editor-steps-11-20.md) for API/file mapping and evidence.

## Lights, instancing, and GPU metrics

Scene3 supports directional/point lights and per-entity tint. Renderer set_lights validates bounded light input before
mutation. Compatible opaque draws use reusable instance matrix/tint data at binding 1; overlays/unlit/alpha paths
preserve ordering barriers. Buffers can grow up to an explicit limit, so reuse does not mean no allocation during
capacity growth.

GPU timestamps are read asynchronously from a completed range using device period/valid bits and availability, with no
extra query WAIT. Unsupported/not-ready values remain optional. The measured range excludes CPU work, acquisition,
presentation and display pacing. No speedup or utilization claim follows from instancing alone.
See [steps 34–35](editor-steps-21-35.md#step-34-asynchronous-gpu-timestamps-and-submission-metrics).
