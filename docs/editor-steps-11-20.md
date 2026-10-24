# Editor and rendering changes: steps 11–20

**Historical milestone:** normal game/editor ownership, schema and rendering contracts have advanced
in [steps 21–35](editor-steps-21-35.md). The native UI and metrics below describe that earlier implementation, not the
current Angular product direction.

This Engine Developer report describes implemented steps 11–20 against Git baseline `b8c2301` and the current
staged/unstaged working tree. **Verification is automated only.** The user chose to finish without interactive
UI/gameplay or rendered-image acceptance checks when native launch access expired. Implementation and tests are complete
for this sequence; panel appearance, dragging feel, lighting appearance, and GPU performance are not certified.

The work extends the native `game` application with editing tools. It does not add those tools to the Angular/Tauri
editor. Game App Developer remains empty. The saved `robots.json`, including the user's large Left Robot scale, is
preserved rather than reset to an example scene.

## Architecture and file map

```text
GameApp: native events, Edit/Play lifetime, scene I/O, draw submission
  SceneEditor: generic selection, picking, edit commands and history
    Scene / EntitySnapshot / EntityKey: identity, content and restoration
    bounds + picking + gizmo + visibility: CPU geometry/math
  RobotGame: optional robot simulation during Play
  EditorTools: dirty state, drag lifecycle, catalog placement, CPU metrics
  EditorPanel / SelectionOverlay / GizmoOverlay: native UI/draw resources
  AssetManager / Model: imports, bounds, cached GPU meshes and textures
  WindowRenderer: GPU uploads, synchronized dynamic writes, drawing/resize
```

| Source                                                                                                     | Responsibility                                                          |
|------------------------------------------------------------------------------------------------------------|-------------------------------------------------------------------------|
| `crates/runtime/src/scene_editor.rs`                                                                       | Reusable edit API without robot rules                                   |
| `crates/runtime/src/commands.rs`                                                                           | Mixed transform/rename/entity history, 256 undo records                 |
| `crates/runtime/src/scene.rs`                                                                              | Snapshots, restoration, identity/bindings, visible draw collection      |
| `crates/runtime/src/bounds.rs`, `picking.rs`, `gizmo.rs`, `visibility.rs`                                  | Local bounds, ray selection, drag math, frustum/statistics              |
| `crates/runtime/src/assets/catalog.rs`, `asset_manager.rs`, `model.rs`, `mesh_data.rs`, `gltf_loader.rs`   | Discovery, shared models, cached bounds, normals                        |
| `apps/game/src/main.rs`, `game.rs`                                                                         | Application callbacks/Edit/Play and optional robot simulation           |
| `apps/game/src/editor_tools.rs`, `editor_panel.rs`                                                         | Editing integration, hierarchy list, fields, asset browser, dirty state |
| `apps/game/src/selection_overlay.rs`, `gizmo_overlay.rs`                                                   | Cached selection box and batched transform handles                      |
| `crates/renderer/src/window_renderer.rs`, `vulkan/mesh.rs`, `frame.rs`                                     | Dynamic mesh allocation/update and synchronization                      |
| `crates/renderer/src/vulkan/vertex.rs`, `push_constants.rs`, `render_object.rs`, `pipeline.rs` and shaders | Normal/lighting attributes, draw flags, depth-free overlays             |

## Step 11: generic SceneEditor

**Responsibility and reason.** Generic selection, movement, commands, and editing no longer belong to RobotGame.
`runtime::SceneEditor` can work with an arbitrary Scene. RobotGame retains only robot-specific simulation, so a generic
scene does not need fake robot roles to open in Edit.

**Files:** `runtime/src/scene_editor.rs`, `commands.rs`, `selection.rs`; `game/src/main.rs` and `game.rs`. The full
crate paths are listed above.

**Example:** construct `SceneEditor::from_scene(&scene)`, select an entity, then call
`set_selected_transform(&mut scene, transform)` to record an edit. Application input calls
`editor.update(&mut scene, &input, delta_seconds)` in Edit; it does not also run robot animation there.

**Reading:** [Rust enums and associated data](https://doc.rust-lang.org/book/ch06-01-defining-an-enum.html) explains the
representation used by typed edit/history variants. Compare the runtime editor API with
`crates/runtime/tests/editor_api.rs`.

## Step 12: temporary Play state

**Responsibility and reason.** Development starts in **Edit**. F6 clones the authored Scene and camera for Play.
Model/mesh/texture resources remain shared through Arc ownership rather than reimported. Stop restores authored content,
preventing simulated changes from silently becoming edits.

**Files:** `apps/game/src/main.rs` (`start_play`, `stop_play`, `save_scene`, `reload_scene`), `runtime/src/scene.rs`,
`model_instance.rs`, and `camera.rs`.

**Example:** move an entity in Edit, enter Play with F6, simulate, and stop with F6. The authored transform and camera
are restored. F5 during Play saves the authored snapshot, not simulated state. Failed reload keeps the live state
available. Mode changes cancel interaction, suppress held input, and reset timing.

Generic scenes can enter Play without robot bindings; robot simulation is optional. For version-1 migration only, the
application adapts the first two entities into roles when at least two exist. Version-2 roles use explicit bindings.

**Reading:** [Rust Arc shared ownership](https://doc.rust-lang.org/std/sync/struct.Arc.html) helps distinguish cloned
scene state from shared model allocations. Sharing a handle is not copying a GPU resource.

## Step 13: structural history

**Responsibility and reason.** CommandHistory now mixes transform edits, rename, spawn, duplicate, and delete.
EntitySnapshot retains ID, persistent key, scene order, bindings, and shared model ownership so undo restores the actual
entity rather than an approximate replacement.

**Files:** `runtime/src/commands.rs`, `scene.rs`, and `scene_editor.rs`; `crates/runtime/tests/scene_history_api.rs`.

**Example:** duplicate with C, delete it, then Ctrl+Z. Deletion is undone with its prior identity/order/model sharing.
Another undo reverses duplication. Rename and transform edits participate in the same 256-entry undo stack. Changed
edits invalidate redo; no-ops and errors preserve it. Undo/redo only move records after successful application.

Use `undo_scene`/`redo_scene` for mixed history. The legacy transform-only `undo`/`redo` wrappers reject a structural
record. History is session state and is not stored in scene JSON. Saving is not an undo operation.

**Reading:** [Rust enums](https://doc.rust-lang.org/book/ch06-01-defining-an-enum.html) and the
author's [Command / undo-and-redo chapter](https://gameprogrammingpatterns.com/command.html#undo-and-redo). The engine's
concrete snapshot/restore rules are defined by its source and tests, not by the external pattern description.

## Step 14: model bounds and selection box

**Responsibility and reason.** Imported CPU vertex positions produce model-local axis-aligned bounds once during model
preparation. Selection can display model extent without reading GPU buffers or rebuilding highlight geometry every
frame.

**Files:** `runtime/src/bounds.rs`, `assets/model.rs`, `asset_manager.rs`; `apps/game/src/selection_overlay.rs`.

**Example:** selecting a bounded entity transforms a cached unit wire box by its local bounds and instance transform.
The shared selection mesh is uploaded under `builtin:selection-box` once; its yellow material is unlit. Bounds are
padded slightly for visibility. Models without bounds produce no selection box.

This represents the whole model's box, not exact visible surfaces or glTF node hierarchy. Source transforms and the
saved large robot scale are respected.

**Reading:** [PBRT bounding boxes](https://www.pbr-book.org/4ed/Geometry_and_Transformations/Bounding_Boxes), especially
the distinction between geometry and enclosing axis-aligned bounds.

## Step 15: screen-ray selection

**Responsibility and reason.** Mouse selection uses the same Vulkan camera/viewport convention as drawing.
`CameraRay::from_screen` unprojects near/far clip points (depth 0 and 1). Picking transforms that ray into each model's
local coordinates, intersects its box using slabs, and returns the nearest hit.

**Files:** `runtime/src/picking.rs`, `bounds.rs`; `SceneEditor::pick`; native cursor mapping in
`apps/game/src/main.rs` / `editor_tools.rs`; `crates/runtime/tests/picking_api.rs`.

**Example:** click an object in Edit while the mouse is not captured. Screen coordinates are mapped from physical window
pixels to the actual prepared surface extent. An empty click clears selection. Exact hit ties retain scene order.
Singular transforms and models without bounds are skipped.

The local ray direction is not renormalized after inverse transformation, preserving comparable distance under scaling.
This is nearest box picking within the camera segment, not triangle-accurate selection or an ID-buffer pass.

**Reading:
** [Vulkan depth conventions](https://docs.vulkan.org/guide/latest/depth.html), [PBRT boxes](https://www.pbr-book.org/4ed/Geometry_and_Transformations/Bounding_Boxes),
and [glam Mat4](https://docs.rs/glam/0.33.6/glam/f32/struct.Mat4.html) for matrix inversion/point-versus-vector
transforms.

## Step 16: native hierarchy list, properties, dirty state

**Responsibility and reason.** A native game panel exposes scene entities, selection, name, position, rotation, scale,
mode, assets, and statistics. It reuses SceneEditor commands instead of bypassing history. This is a flat entity list
called the hierarchy panel; no parent-child scene graph was added.

**Files:** `apps/game/src/editor_panel.rs`, `editor_tools.rs`; `Scene::rename`, content comparison, and command history.

**Example:** click an entity, edit its Name field, press Enter, then Ctrl+Z. Rotation fields display degrees and convert
to radians on application. Escape cancels typing; typed field input suppresses movement shortcuts. Invalid values report
an error rather than becoming valid commands.

Dirty state compares the authored scene and camera against saved content. It includes persistent identity, names, paths,
transforms, bindings, and camera; undo returning exactly to saved content clears dirty. During Play the comparison uses
authored state, not simulation. A title marker and panel report it.

The native panel uses an ASCII bitmap font; Unicode names remain data but unsupported characters render as fallback
glyphs. Panels require at least 800×400 logical pixels; smaller layouts hide panels and short layouts can truncate lower
properties. No image-based appearance check was run.

**Reading:** [Rust enums](https://doc.rust-lang.org/book/ch06-01-defining-an-enum.html) for typed Property/PanelAction
values; inspect the panel's `begin_edit`, field conversion, and hit regions alongside callback tests.

## Step 17: mouse transform gizmos

**Responsibility and reason.** Gizmo math is runtime code; event/capture/cancellation belongs to the game. Preview is
derived from the original transform, and release commits one history record. This avoids making every cursor event a
separate undo entry.

**Files:** `runtime/src/gizmo.rs`; `apps/game/src/editor_tools.rs`, `gizmo_overlay.rs`; dynamic mesh methods in
WindowRenderer and VulkanMesh; `crates/runtime/tests/gizmo_api.rs`.

**Example:** press 1 for Translate, drag a visible axis, and release. Ctrl+Z reverses the whole drag. 2 selects Rotate
and 3 selects Scale. Translate uses world axes and projected screen-axis mapping; rotation uses world-axis
quaternion/ray-plane math; scale follows the selected local axis.

Escape, focus loss, resize, or a mode change cancels preview back to its starting value. Camera-parallel/invalid handles
are rejected; non-finite drag inputs are tested. The overlay batches handles in a preallocated mesh and synchronizes
buffer writes with the renderer fence. It is unlit and depth-free. Native handle hit feel and appearance remain
unverified.

**Reading:
** [glam matrices](https://docs.rs/glam/0.33.6/glam/f32/struct.Mat4.html), [Vulkan fence waits](https://docs.vulkan.org/refpages/latest/refpages/source/vkWaitForFences.html),
and [Vulkan depth](https://docs.vulkan.org/guide/latest/depth.html).

## Step 18: model catalog and placement

**Responsibility and reason.** Asset discovery is separated from loading and from the demo robot shortcut. The panel can
place any supported model found under the asset root through the same undoable scene API.

**Files:** `runtime/src/assets/catalog.rs`; `apps/game/src/editor_tools.rs`, `editor_panel.rs`;
`crates/runtime/tests/catalog_api.rs`.

**Example:** Refresh the asset catalog, scroll to a GLB/glTF entry, and Place it. The new entity appears three units
along camera forward and Ctrl+Z removes that placement. Repeated placements reuse a cached template's Arc<Model>. The N
robot shortcut works without a role binding: it reuses an existing robot instance when possible or loads
`props/robot.glb` even from an empty scene, provided the asset exists.

Scanning canonicalizes the root/entries, skips links, checks root containment, accepts case-insensitive .glb/.gltf
extensions, sorts paths, and limits traversal to 10,000 visited entries and 32 directory levels. Importing independently
checks containment. Catalog membership is discovery, not proof a model imports. There is no preview thumbnail, search,
dependency editor, or general glTF scene-node import.

**Reading:** [Rust filesystem APIs and TOCTOU notes](https://doc.rust-lang.org/std/fs/)
and [glTF meshes/primitives](https://github.khronos.org/glTF-Tutorials/gltfTutorial/gltfTutorial_009_Meshes.html).

## Step 19: normals and directional light

**Responsibility and reason.** Geometry now carries normals so scene objects can show a basic directional response,
while editor overlays stay readable independently of lighting.

**Files:** `renderer/src/vulkan/vertex.rs`, `push_constants.rs`, `render_object.rs`, `pipeline.rs`, `frame.rs`;
`renderer/shaders/triangle.vert` / `triangle.frag`; runtime `assets/mesh_data.rs` and `gltf_loader.rs`.

**Example:** glTF NORMAL values must be finite and nonzero and are normalized. Missing attributes use area-weighted
triangle normals; vertices with no usable result fall back to +Z. Split cube vertices preserve hard edges. The shader
transforms normal directions by inverse transpose for nonsingular model transforms, then applies ambient 0.25 plus
diffuse 0.75 from normalized direction `[-0.4, 0.8, 0.6]` to textured base color.

Vertex stride is now 32 bytes: position at byte 0/location 0, UV at byte 12/location 1, normal at byte 20/location 2.
Push constants are 96 bytes: model at 0, factor at 64, flags at 80. Flags distinguish unlit, screen-space font,
vertex-color, and depth-free overlay draws. The panel, selection, and gizmos use appropriate unlit paths. A depth-free
pipeline supports UI/handles without treating them as normal scene geometry.

This is not PBR, editable light entities, shadows, tone mapping, or a visually verified material result. Flags also
reuse the normal attribute channel for panel/handle color in specialized draws; source layouts and shaders must change
together.

**Reading:** [glTF mesh attributes](https://github.khronos.org/glTF-Tutorials/gltfTutorial/gltfTutorial_009_Meshes.html)
and [Vulkan depth/pipeline behavior](https://docs.vulkan.org/guide/latest/depth.html).

## Step 20: frustum culling and frame statistics

**Responsibility and reason.** CPU visibility uses cached model bounds to avoid submitting wholly outside instances.
Statistics expose what is submitted without pretending to be GPU profiling.

**Files:** `runtime/src/visibility.rs`, `bounds.rs`, `Scene::render_visible`; `apps/game/src/main.rs` and
`editor_tools.rs`; `crates/runtime/tests/render_data_api.rs`.

**Example:** F7 toggles culling. Six normalized planes are extracted from camera matrix rows using ±X/±Y against W, near
Z, and far W−Z. An instance's transformed world AABB is tested conservatively. Unknown bounds or invalid bound
transforms remain drawn rather than disappearing. Scene statistics count total/visible/culled entities, draw calls, and
triangles; render integration adds selection, gizmo, and panel draws.

Panel FRAME is wall-clock frame interval and UPDATE is measured update duration, averaged over roughly 0.5 seconds. Long
focus/minimize stalls above 0.25 seconds reset sampling. These numbers are not GPU timestamps, full CPU render time, or
a frame-time budget guarantee. Culling is instance-level box testing, not occlusion, batching, meshlet, or triangle
culling. FIFO remains the presentation policy.

The panel uses one font atlas plus one batched host-coherent mesh; gizmos use one batched mesh, and selection uses one
cached box. Buffers are reused, with fence completion before writes; GPU buffers/textures are not allocated each normal
frame. CPU vectors/text and temporary draw requests are not claimed allocation-free.

**Reading:
** [PBRT bounds](https://www.pbr-book.org/4ed/Geometry_and_Transformations/Bounding_Boxes), [Vulkan depth](https://docs.vulkan.org/guide/latest/depth.html), [fence waits](https://docs.vulkan.org/refpages/latest/refpages/source/vkWaitForFences.html),
and [device idle](https://docs.vulkan.org/refpages/latest/refpages/source/vkDeviceWaitIdle.html).

## Controls and mode boundaries

| Input                | Current development behavior                                                |
|----------------------|-----------------------------------------------------------------------------|
| F1                   | Show/hide native panels; compact windows can still hide them                |
| F5 / F9              | Save authored scene / reload from disk                                      |
| F6                   | Enter Play / Stop and restore Edit                                          |
| F7                   | Toggle frustum culling                                                      |
| Left click           | Panel action, gizmo handle, or ray selection in Edit                        |
| 1 / 2 / 3            | Translate / rotate / scale gizmo in Edit                                    |
| Tab                  | Cycle Edit selection                                                        |
| WASD                 | Move Edit selection; in robot Play move robot_a                             |
| Q/E, R/F             | Edit rotation / scale steps                                                 |
| N / C / Delete       | Edit spawn robot / duplicate / delete; structural history is undoable       |
| Ctrl+Z / Ctrl+Y      | Mixed Edit undo / redo                                                      |
| Space                | Pause/resume robot simulation during Play (rotation and WASD)               |
| Arrow keys           | Camera movement                                                             |
| Right mouse / Escape | Capture for look / release; Escape also cancels text or drag in its context |
| Enter                | Apply active panel text field                                               |

Allowed panel mouse commands reset FrameClock, suppress held input, and end the movement gesture before dispatch,
matching keyboard command timing boundaries even when I/O/loading fails. Typing blocks movement, dragging blocks
conflicting input, and mode/focus/resize paths cancel previews. Structural Edit operations no longer protect robot
bindings just to satisfy simulation; generic scenes can remain editable. Installed/name-file mode excludes the
development editing panels/capture/update behavior described here.

## Verification evidence and limits

The implementation/tester handoff reports **183 passed, 0 failed, 1 ignored** across runtime/game/renderer all-target
suites with offline locked dependencies, after the non-finite drag correction, callback regressions, and final
panel-command timing correction (the game suite reports 41 passes). This total supersedes earlier intermediate counts.
The ignored runtime initialization test requires native Vulkan/GPU support.

The handoff also reports successful debug game builds, full-workspace all-target compile, and focused Clippy for
runtime/game libraries, binaries, and examples. A later ordinary-target rebuild encountered a transient Windows
access/file-lock error replacing game.exe; this alone does not establish which process held it. A second ordinary-target
attempt also failed with Windows error 5; the latest default-target game.exe is not certified as replaced. No process
was terminated. Final source built successfully with
`cargo build -p game --locked --offline --target-dir target/steps-11-20-verification`, producing
`target/steps-11-20-verification/debug/game.exe`. Clippy completed with existing argument-count, glTF modulo, example
collapsible-if, and cache-hardlink warnings; this is not a zero-warning claim. Documentation-site checks and final
checkpoint are recorded separately in `docs-site/docs-state.json`.

Supplementary automated native checks also passed: the explicitly enabled Vulkan initialization test (one pass), hidden
`renderer_smoke` (textured rendering, resize, suspension, resource cleanup), and hidden `scene_io_smoke` (save/reload,
rollback, GPU lifecycle, unchanged bundled scene). These results supplement the standard 183-pass/one-ignored run; the
ignored test passes when explicitly enabled.

No UI screenshot, visible interaction, lighting comparison, or GPU-performance measurement was performed for steps
11–20. Earlier user-confirmed renderer extraction results remain historical evidence only. The user explicitly chose
automated-only verification. Authored/play separation and callbacks have automated tests, not visual acceptance.

Outstanding unrelated limits remain: admin source-generation JSON asset packaging, installed release verification,
Jenkins execution evidence, full glTF scene import, and backend-independent resource ownership. Steps beyond 20 are
future planning, not implementation in this report.
See [testing](testing.md), [scene editing](scene-editing.md), [scene persistence](scene-persistence.md),
and [pipeline](pipeline.md).

For a local launch using the verified alternate output location:
`cargo run -p game --locked --offline --target-dir target/steps-11-20-verification`. This command is provided for
follow-up interaction; no interactive acceptance run is claimed here.
