# Normal maps · Milestones 246–256

**Status** · Implemented and verified for the Windows Castle Offense pilot on November 28, 2026. See
the [implementation results and limits](../verification/normal-maps.md). This follows the reserved world-streaming
milestones 233–245.

The first demo is a Castle Offense brick wall under a light that moves from left to right. Mortar and chipped brick
should respond to the direction of the light. The wall outline, collision, and shadow shape should match the original.
The current renderer already has camera depth testing and a directional shadow map. The new work lets a material change
the normal used for lighting at each pixel.

**Delivery order:** 246 → 247 → 248 → 249 → 250 gives a static wall pilot. Complete 251–255 before broad editor or
asset-pack rollout. Run 256 before shipping. Normal strength comes from the imported material. The first release covers
imported tangent-space normal maps.

The snippets illustrate the proposed interfaces and checks. Use the existing functions and error types where available.

## 246. Create a lighting comparison fixture

**Files** · `crates/runtime/tests/gltf_scene_api/normal_maps.rs` (new) and `apps/editor-e2e/native-viewport.test.mjs`.

1. Build a small self-contained GLB fixture with a wall, a receiver, and adjacent panels that share a base color. Give
   the panels flat and known directional normal textures. Include one material without a normal texture, one with
   strength zero, and one with a known nonzero tilt. Use tiny generated test images that can ship with the tests.
2. Fix the camera, ambient, fog, exposure, and shadow settings across captures. Capture under two opposite grazing light
   directions. Save the original renderer's baseline, along with a completed color/depth frame check that avoids editor
   overlays.

```text
flat RGB = (128, 128, 255)
tilt +X RGB ≈ (204, 128, 230)
expected test: the brighter flank switches sides when the light moves
```

**Done when** · The test identifies the same pixel regions on repeat runs and the baseline records how meshes without a
normal map render today. Record image-space thresholds and fixed lighting before changing the renderer.

**Sources
** · [glTF tangent-space convention](https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html#_material_normaltexture) · [Native viewport test](../../apps/editor-e2e/native-viewport.test.mjs)

## 247. Import normal textures as material data

**Files** · `crates/runtime/src/assets/gltf_loader.rs`, `gltf_loader/mesh.rs`, `gltf_loader/resources.rs`, and
`scene_import.rs` under `crates/runtime/src/assets/`.

1. Read `material.normalTexture`, its image identity, texture-coordinate set, sampler and strength. Reuse the existing
   decoder and contained-file checks. Decode only requested images within existing limits, and count each shared decoded
   image once.
2. Carry the optional normal input through the imported primitive and material metadata. Version the metadata only where
   the existing contract requires it. Keep an absent normal input representable. Report malformed references or a
   missing declared UV set with mesh and primitive context. Support the sampler choices the renderer can honor and
   report unsupported authored options explicitly.

```rust
struct NormalTextureInput {
    image_index: usize,
    tex_coord: u32,
    scale: f32,
    sampler: ImportedSampler,
}
```

**Done when** · Headless import tests cover embedded GLB and contained external glTF images, absent textures, strength
defaults and zero, and invalid references and UV data. Existing models reopen with the same stable mesh IDs.

**Sources
** · [Normal texture properties](https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html#_material_normaltextureinfo) · [Asset identity and import decision](../adr/0006-asset-identity-and-import-pipeline.md)

## 248. Upload normal textures as linear data

**Files** · `crates/renderer/src/vulkan/texture.rs`, `vulkan/upload_batch.rs`, `vulkan/material.rs`,
`vulkan/pipeline.rs`, `src/window_renderer.rs`, and `crates/runtime/src/assets/asset_manager.rs` and
`assets/asset_manager/upload.rs`.

1. Give textures a typed color or data role through immediate and batched uploads. Base-color textures keep
   `R8G8B8A8_SRGB`. Normal textures use `R8G8B8A8_UNORM` and linear filtering by default. Keep image identity plus
   format in the GPU image key and include sampling configuration in any cached sampler or descriptor key.
2. Give lit materials an optional normal texture using a dedicated descriptor set. Reserve set 3 after the current
   camera, base color, and skin sets. Check layout compatibility and bind a valid shared flat descriptor to every
   pipeline that statically reads the slot. Keep cutout masks on the base-color binding.

```rust
let format = match usage {
    TextureUsage::Color => vk::Format::R8G8B8A8_SRGB,
    TextureUsage::Normal => vk::Format::R8G8B8A8_UNORM,
};
```

**Done when** · A mid-channel sample remains near 0.5 in the normal shader. One image used as both base color and normal
creates distinct GPU interpretations. Vulkan validation passes on both the mapped and legacy materials. Partial upload
rollback releases all admitted resources consistently.

**Sources
** · [Vulkan formats](https://docs.vulkan.org/spec/latest/chapters/formats.html) · [Vulkan descriptor limits](https://docs.vulkan.org/spec/latest/chapters/limits.html) · [Current material layout](../../crates/renderer/src/vulkan/material_layout.rs)

## 249. Supply tangents and the correct UV set for static meshes

**Files** · `crates/runtime/src/assets/gltf_loader/mesh.rs`, `crates/runtime/src/assets/mesh_data.rs`,
`crates/renderer/src/vulkan/vertex.rs`, `vulkan/mesh.rs`, and `vulkan/pipeline.rs`.

1. Retain `TANGENT` as a four-component value, with XYZ for the tangent direction and W for handedness. Preserve the
   chosen normal UV independently from base-color UV. Validate counts and finite attributes. Use the existing base
   stream for models that can render with geometric normals. Measure the memory impact of an optional normal-map vertex
   stream against widening the shared vertex.
2. Use MikkTSpace for missing tangents on a normal-mapped primitive and generate flat normals first where the source
   lacks normals. Split vertices at incompatible UV or tangent corners. Bound the remap before allocating, keep triangle
   order and picking identities stable, and use geometric shading for undefined tangent frames.

```text
normal_uv = TEXCOORD[normalTexture.texCoord]
bitangent = cross(normal, tangent.xyz) × tangent.w
```

**Done when** · Tests cover supplied tangents, generated tangents, mirrored UV islands, degenerate UV triangles, and a
model whose base-color and normal textures use different UV sets. Any split vertex and index data count against import
and upload budgets.

**Sources
** · [glTF mesh attributes and tangent handedness](https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html#_meshes) · [MikkTSpace reference implementation](https://github.com/mmikk/MikkTSpace)

## 250. Light the static wall correctly

**Files** · `crates/renderer/shaders/triangle.vert`, `triangle.frag`, `shadow.frag`,
`crates/renderer/src/vulkan/push_constants.rs`, `vulkan/frame/recording.rs`, and `vulkan/frame/draw_plan.rs`.

1. Build a tangent frame from the same transform used for the draw or instance. Transform normals with the inverse
   transpose, transform tangents as directions, and orthogonalize before applying the map. Preserve glTF handedness on
   mirrored UVs and handle back faces according to the double-sided material convention. Finish this milestone with
   positive-determinant static transforms, including nonuniform scale.
2. Use the mapped normal for the existing light response. Use geometric depth and the geometric receiver plane for
   shadow visibility and bias. Keep existing alpha coverage, emissive, fog, Unlit, and editor overlay behavior. Batch
   only draws that share compatible normal descriptors, strength, and vertex layout.

```glsl
vec3 sampledNormal = texture(normalSampler, normalUv).rgb * 2.0 - 1.0;
sampledNormal.xy *= normalScale;
vec3 lightNormal = normalize(tangentFrame * normalize(sampledNormal));
```

**Done when** · The fixture's bright flank changes as the light moves. A strength of zero and the no-map path preserve
legacy lighting, and the flat map matches within quantization tolerance. Static and instanced views agree. The depth and
shadow silhouettes and alpha-mask coverage stay consistent in the on/off captures. Verify a single staged `wall-05.glb`
before moving to the next milestone.

**Sources
** · [glTF normal decoding and strength](https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html#_material_normaltextureinfo_scale) · [Current lit shader](../../crates/renderer/shaders/triangle.frag) · [Shadow shader](../../crates/renderer/shaders/shadow.frag)

## 251. Support reflected objects and animated characters

**Files** · `crates/renderer/src/vulkan/pipeline.rs`, `vulkan/frame/draw_plan.rs`,
`crates/renderer/shaders/skinned.vert`, `crates/bridge/src/animation_asset/mesh.rs`,
`crates/engine-core/src/animation/asset.rs`, and `crates/runtime/src/assets/asset_manager.rs`.

1. Correct culling for reflected static objects in both the lit and shadow passes. Carry instance handedness and
   separate draw batches when the front-face convention differs. Exercise a single-axis reflection and a parent that
   supplies the reflection. Use a finite geometric fallback for singular transforms.
2. Retain normal-texture bindings, UVs, and tangents through character import. Skin tangents with the same joint
   transform convention as normals, then reorthogonalize the posed frame. Preserve the existing positive-similarity
   transform checks for skinned placement. Keep joint weights and visual bounds intact when tangent seams split
   vertices.

```text
world_sign = tangent.w × sign(determinant(model_linear))
mirror draw = matching front face in the color pass and shadow pass
```

**Done when** · Mirrored single- and double-sided objects shade correctly, including on the shadow pass. A bent skinned
test mesh keeps the detail attached to the surface through animation. Existing skin, picking, and wireframe regressions
pass.

**Sources
** · [glTF skinned mesh attributes](https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html#_skinned_mesh_attributes) · [Skin import tests](../../crates/bridge/tests/animation_asset_api/decoding.rs)

## 252. Preserve normal maps through Blender export and reimport

**Files** · `integrations/blender/game_engine_export/blender_addon.py`, `static_glb.py`, and `character_glb.py` in the
same folder. Verify native adoption in `crates/runtime/src/assets/project_assets/mesh_source.rs`.

1. Accept the supported Blender Image Texture → Normal Map → Principled BSDF setup. Set the normal image to Non-Color
   data and export embedded PNG textures and tangents as appropriate for each profile. Teach the profile validator to
   accept `normalTexture`, validate the texture reference and selected UV stream, and return a clear error for
   unsupported node setups.
2. Reimport the test model with a revised normal texture. Preserve stable IDs, existing placements, and per-primitive
   material assignment. Do the same with the character profile after milestone 251.

```python
normal_texture = material.get("normalTexture")
if normal_texture is not None:
    validate_normal_texture(normal_texture, textures, primitive_attributes)
```

**Done when** · A Blender-generated static model and a skinned model retain the authored UV orientation and normal
strength in the editor. Texture-only reimport updates visible detail while the two placed instances keep their IDs and
transforms.

**Sources
** · [Blender glTF material export](https://github.com/KhronosGroup/glTF-Blender-IO/blob/main/docs/blender_docs/scene_gltf2.rst#materials) · [Blender asset pipeline decision](../adr/0013-blender-asset-pipeline.md)

## 253. Verify cache, scene reload, and standalone export

**Files** · `crates/runtime/src/assets/warm_model.rs`, `asset_manager/upload.rs`, and `asset_manager/source_cache.rs`
under `crates/runtime/src/assets/`, plus `compiler/src/dependencies.rs`, `compiler/src/preflight.rs`, and
`compiler/tests/unit/tests_dependencies.rs`.

1. Include normal images, tangent bytes, and any duplicate GPU formats in cold, warmed, and retired resource accounting.
   Reuse source fingerprints and dependency walking for both embedded and external images. Check old job results after
   reimport or scene replacement. Reject missing, changed, or out-of-project dependencies before export publication.
2. Reopen the project and export to a separate D: directory. Launch the Player without access to source assets. Compare
   the normal-mapped fixture under matched light and camera settings. Verify that failed import or export leaves the
   last usable asset and package in place.

```text
A → B → A: first load, warm revisit, normal-only reimport, clean reopen
export A: packaged images and models are the only asset inputs
```

**Done when** · Cold load, warm adoption, texture-only reimport, Save/Reopen, and standalone export all show the
expected material revision. A missing external normal fails with a useful asset path. A deliberately reused color/normal
image is charged correctly on GPU.

**Sources
** · [Verified asset reuse decision](../adr/0015-verified-asset-reuse.md) · [Compiler dependency walker](../../compiler/src/dependencies.rs)

## 254. Make normal maps inspectable in the editor

**Files** · `crates/runtime/src/assets/scene_import.rs`,
`apps/editor-host/src-tauri/src/commands/editor/asset_authoring.rs`,
`apps/editor-host/src-tauri/src/viewport/windows/prop_preview.rs`, and
`apps/editor/src/app/modules/editor/workspace/prop-workspace/`.

1. Surface whether each imported material has a normal map and show its source, UV set, and strength. Show a useful
   reason when it falls back to geometry. Keep one source of truth in the imported asset metadata.
2. Add a Prop preview comparison control for **Show normal detail**, using native preview state. This switch should
   neither dirty the scene nor affect Scene or Game view settings. Verify that reset, selection changes, and view
   closure follow existing preview lifetimes.

```ts
interface ImportedNormalInfo {
    readonly texCoord: number;
    readonly scale: number;
}
```

**Done when** · A user can select a material in Props, compare it under the same light with detail on/off, switch to
another prop, and export without the preview choice leaking into authored data. Check keyboard access and
missing-material feedback.

**Sources
** · [Prop preview service](../../apps/editor/src/app/modules/editor/workspace/prop-workspace/prop-preview.service.ts) · [Viewport state decision](../adr/0012-native-viewport-composition.md)

## 255. Apply verified source maps to Castle Offense

**Files** · `D:/Projects/Castle Offense/assets/models/wall-05.glb`,
`D:/Projects/Castle Offense/assets/asset-catalog.json`, and `D:/Projects/Castle Offense/Pack sources.csv`. Do the
conversion in a staging copy on D:.

1. Start with the single `MI_ExBrickWall` material on `wall-05.glb`. Its source Unity material
   `Art/Materials/M_ExBrickWall.mat` points to `Art/Textures/T_ExBrick_N.TGA`. The existing Castle Offense GLB has
   normals and UVs but lacks both tangents and a normal texture. Use the existing UVs. Compare the raw RGB and
   positive-Z reconstructed candidates where the source blue channel falls below glTF’s normal range. Embed the
   confirmed linear PNG and compare left/right light with a flat-map control.
2. Verify the source green-channel convention with the reference scene and preserve the source hash. Next try
   `exterior-floor-2.glb`, whose `M_Tiles_3` source binds `T_Tiles3_N.TGA`. Keep the door, fountain, and other masked
   blends pending a separate verification of both source normals and their blend mask. Check channel-packed sources
   individually before reconstructing or swizzling them. Update catalog bytes, fingerprints, and provenance together.

```json
{"normalTexture":{"index":1,"texCoord":0,"scale":1}}
```

**Done when** · The wall and a second verified stone material show relief in the editor and the copied standalone
project. The scene's routes, shadows, silhouettes, and clearance pass the Castle Offense regression suite. Keep the
prior project/build available on D: for comparison.

**Sources
** · [Castle pack material](<D:/Assets/GameDev/FantasyCastle__Allrenderpipelines/FantasyCastle/Assets/LeartesStudios/FantasyCastle/Art/Materials/M_ExBrickWall.mat>) · [Original brick normal](<D:/Assets/GameDev/FantasyCastle__Allrenderpipelines/FantasyCastle/Assets/LeartesStudios/FantasyCastle/Art/Textures/T_ExBrick_N.TGA>) · [Unity normal unpacking](https://github.com/Unity-Technologies/Graphics/blob/master/Packages/com.unity.render-pipelines.core/ShaderLibrary/Packing.hlsl) · [glTF normal channels](https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html#_material_normaltexture)

## 256. Measure cost and qualify the release

**Files** · `crates/renderer/src/vulkan/frame/timing.rs`, `crates/renderer/src/vulkan/texture.rs`,
`apps/editor-e2e/native-viewport.test.mjs`, and a new report at `history/verification/normal-maps.md`.

1. Measure baseline and mapped Castle Offense builds on the same GPU, scene, camera route, and resolution. Compare
   median and p95 frame cost, draw calls, CPU import time, and peak and settled GPU memory after repeat loads. Inspect
   distant masonry while moving the camera. Introduce normal-map mipmaps or anisotropic filtering only if the
   measurements and captures justify the cost. Keep mip sampling linear and renormalize the shading vector.
2. Run the focused Rust, Angular, Blender, shader, and native export checks. Cover shade-mode switching, resource
   retirement, and a legacy scene with masked grass, particles, fog, and directional shadows.

```powershell
cargo test --locked -p renderer -p runtime --lib
cargo test --locked -p bridge --test animation_asset_api
npm.cmd test -- --watch=false
```

Run the npm command from `apps/editor`. Keep build and test artifacts on D:. Record the exact builds, hardware, fixture,
image comparisons, and measured results in the verification report.

**Done when** · The authored material appears the same in the Lit scene, Prop preview, Editor Play, and the copied
Windows Player under matched lighting. The fixture and Castle Offense pass, with no Vulkan validation errors or
asset-lifetime regressions. Record any measured limits before claiming a performance budget.

**Sources
** · [Renderer timing code](../../crates/renderer/src/vulkan/frame/timing.rs) · [Vulkan image formats and capabilities](https://docs.vulkan.org/spec/latest/chapters/formats.html) · [Windows release check](../../apps/editor-host/build-windows-release.ps1)
