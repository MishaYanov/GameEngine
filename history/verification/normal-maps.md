# Normal map verification · November 28, 2026

[Implementation milestones](../roadmaps/normal-maps-246-256.md)

## Rendering and import

- Static and skinned glTF meshes use independent normal UV coordinates, authored or generated MikkTSpace tangents, and
  linear RGBA8 normal textures. Color and normal bindings may share decoded pixels. GPU uploads account for their
  distinct formats. Generated seams retain triangle order and skin influences. Missing normals generate per-face frames.
  Undefined tangent frames use the geometric normal for lighting.
- Reflected meshes choose the matching front face for the main and shadow passes. The shaders keep the geometric
  receiver normal for shadow bias and preserve the existing alpha mask and depth coverage. The Prop Workspace shows the
  selected mesh's material, model path, image and texture indices, UV set, and normal strength. **Show normal detail**
  belongs to the current preview.
- Repeat wrapping and linear filtering are supported. Assets requesting trilinear mipmaps use base-level linear
  filtering with a diagnostic. Import accepts finite nonnegative strength. The shared Vulkan vertex layout grew from 32
  to 56 bytes. With a separate tangent/UV stream, unmapped meshes would save 24 bytes per vertex, before alignment and
  binding overhead. The performance comparison below holds the shared layout constant.

## Native image comparisons

- On the previous Evening Lighting player, all four mapped/unmapped A/B pairs were pixel-identical. On the new player at
  1600×900, panel luminance was 187 for the unmapped and zero-strength controls and 186 for the flat map. Under the left
  key, the +X and mirrored +X panels measured 73 and 255. The UV1 samples measured 73 and 255 in the expected quadrants
  under both grazing light directions.
- The original Fantasy Castle brick and flagstone normals changed 3,473,440 and 1,992,572 RGB channel values
  respectively in the matched close-ups. Both source material bindings and TGA hashes are recorded. Conversion retained
  the green channel and reconstructed positive Z from XY. The two GLBs preserve the original geometry and UVs. All 18
  Castle collision probes and all 12 historic routes passed, covering 8,295 entities and 3,379 native colliders.
- The packaged Editor Play test showed the expected UV1 response, mirrored tangent handedness, and reflected instances.
  A standalone single-sided caster's reflected and unreflected shadows were pixel-identical. The skinned fixture rotated
  a joint 180° about the surface normal while keeping the plane in place. Luminance moved from 132 to 42 as the tangent
  followed the joint. The Khronos validation layer loaded on both standalone checks. Their logs report zero Vulkan
  validation errors.

[Before captures](<D:/Assets/GameDev Converted/_work/castle-offense/normal-maps/captures/before/results.json>) · [After captures](<D:/Assets/GameDev Converted/_work/castle-offense/normal-maps/captures/after/results.json>) · [Packaged Editor capture](<D:/Assets/GameDev Converted/_work/normal-shader-check/editor-e2e/native-normal-maps-y8A6l7/normal-map-fixture.png>) · [Reflected shadow](<D:/Assets/GameDev Converted/_work/normal-shader-check/reflection-caster/frame.png>) · [Pack provenance](<D:/Assets/GameDev Converted/_work/castle-offense/normal-maps/pilot-provenance.json>) · [Castle route log](<D:/Assets/GameDev Converted/_work/castle-offense/normal-maps/collision-normal-maps.log>) · [Static validation log](<D:/Assets/GameDev Converted/_work/normal-shader-check/normal-validation.log>) · [Skinned validation log](<D:/Assets/GameDev Converted/_work/normal-shader-check/skinned-validation.log>)

## Castle Offense performance

Measured on an NVIDIA GeForce RTX 4070 at 1600×900. Both variants use the new Player and the same Castle brick camera
and full scene. A/B/B/A captures supplied 20 settled intervals per variant. GPU timestamps sample a frame once per
second. CPU figures divide process time by presented frames within each sample interval. A separate run switched eight
times between duplicate scenes.

| Measurement                           |                 Castle baseline |                 Two normal maps |
|---------------------------------------|--------------------------------:|--------------------------------:|
| GPU frame median / p95                |                7.421 / 7.952 ms |                7.557 / 8.031 ms |
| CPU per present median / p95          |                1.780 / 2.648 ms |                1.854 / 2.604 ms |
| Draw calls                            |                             206 |                             206 |
| Logical GPU asset memory              |               187,762,356 bytes |               221,316,788 bytes |
| Tracked GPU allocation settled / peak | 204,083,952 / 209,265,904 bytes | 237,638,384 / 242,820,336 bytes |
| Retained decoded model-source memory  |                 3,360,052 bytes |                 3,360,052 bytes |

The increase of 32 MiB matches the two 2048×2048 RGBA8 normal textures. The GLBs grew by 17.42 MiB combined. These
figures cover two pilot materials and a fixed camera. Software measurements use logical asset bytes and do not include
all driver allocation or application memory. At four matched camera positions from 18.4 to 46.3 m, the bump response
stayed visible. The two farthest positions showed fine grain on tower and gate masonry with base-level filtering.
Camera-motion shimmer and broad asset-pack cost remain unmeasured.

The headless CPU import harness ran 16 uncached engine imports of each variant per GLB, with the files already in the
Windows filesystem cache. It measures decoding and tangent preparation before GPU uploads. The brick model changed from
1.174 / 1.509 ms to 47.213 / 48.026 ms median / p95. The flagstone changed from 1.048 / 1.251 ms to 45.222 / 46.124 ms.
All eight measured revisits used the 83 cached models with zero static import time.

[CPU import samples](<D:/Assets/GameDev Converted/_work/castle-offense/normal-maps/cold-import.csv>) · [Raw performance samples](<D:/Assets/GameDev Converted/_work/castle-offense/normal-maps/performance/normal-mapped-brick/report.json>) · [Distant comparison](<D:/Assets/GameDev Converted/_work/castle-offense/normal-maps/distance-check/comparison.png>) · [Camera positions](<D:/Assets/GameDev Converted/_work/castle-offense/normal-maps/distance-check/route.json>)

## Automated checks

| Suite                                                                   | Result                             |
|-------------------------------------------------------------------------|------------------------------------|
| Renderer Rust                                                           | 50 passed                          |
| Runtime Rust                                                            | 149 passed, 1 pre-existing ignored |
| Bridge Rust unit                                                        | 9 passed, 1 pre-existing ignored   |
| Static glTF, including embedded and external normal images              | 13 passed                          |
| Animation asset API, including generated skin tangent frames            | 12 passed                          |
| External normal-map dependency packaging and failure before publication | 1 passed                           |
| Native Prop lifecycle                                                   | 1 passed                           |
| Packaged Prop comparison                                                | Pointer and Space key passed       |
| Angular workspace                                                       | 875 passed                         |
| Blender profile                                                         | 8 passed                           |
| GLSL / SPIR-V                                                           | 8 variants validated               |

Blender 4.0.2 and 5.3.0 exported both static and skinned fixtures. Replacing only the normal texture changed its pixels
and the asset revision in the Rust reimport path. The mesh, material and animation identities and the two saved
placements were preserved. The fixtures and native harness are under the D: test
workspace. [Build and test logs](<D:/Assets/GameDev Converted/_work/castle-offense/normal-maps/build-logs>) · [Reimport harness](../../temp/normal-maps-blender/native-reimport.rs)

A real Windows Editor opened a test Prop from Assets and showed the source image, UV1, and strength. The pointer
disabled normal detail and Space on the focused button restored it. Closing the preview preserved the authored document
and
revision. [Packaged Prop evidence](<D:/Assets/GameDev Converted/_work/castle-offense/normal-maps/offense-desktop/normal-detail-prop-SZAMF1/prop-normal-detail.json>)

## Windows builds and Castle publication

- The portable Editor binary has SHA-256 8f4a6a8aceaffbcfee70954e5b619442720fb598bd8bf697b1fe6b80cc99ed79. Its bundled
  Player and the exported Player share SHA-256 26650894d7169485ee9ef7882a3b34c90f1745d88028ac8336b2d06f06d67891.
- With the staged project open in the packaged Editor, Castle Scene and Editor Play presented at 920×844 with 249 and
  247 draw calls respectively. **Save All** and **Export** packaged 87 assets. The exported GLBs matched their staged
  sources byte for byte. A new editor process reopened and presented the saved scene. The new standalone executable
  rendered from its copied package with a separate working directory and reported GAME_STARTUP_SMOKE_OK mode=player.
- Published the two converted GLBs and their catalog and provenance records. Verified that the authored Castle scene
  SHA-256 stayed at e766671565ba2454d8e6679e9fd306b0e5e1371f673fb4c84a244ec61cb684de. Prior project asset files were
  backed up on D:. The new game was installed in a separate export folder alongside the previous game build.

[Portable Editor](<D:/Builds/editor/Normal Maps/Editor/game-engine-editor.exe>) · [Castle project](<D:/Projects/Castle Offense/project.json>) · [Standalone Castle](<D:/Builds/games/Normal Maps/Castle Offense/game.exe>) · [Export log](<D:/Assets/GameDev Converted/_work/castle-offense/normal-maps/native-editor-export.log>) · [Clean reopen status](<D:/Assets/GameDev Converted/_work/castle-offense/normal-maps/offense-desktop/castle-normal-maps-reopen-cTbbvz/reopen.json>) · [Publication fingerprints and backup](<D:/Assets/GameDev Converted/_work/castle-offense/normal-maps/published.json>)

## References

[glTF normal textures](https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html#_material_normaltexture) · [MikkTSpace reference](https://github.com/mmikk/MikkTSpace) · [Vulkan texture formats](https://docs.vulkan.org/spec/latest/chapters/formats.html) · [Blender glTF material export](https://github.com/KhronosGroup/glTF-Blender-IO/blob/main/docs/blender_docs/scene_gltf2.rst#materials)
