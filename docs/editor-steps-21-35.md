# Angular editor integration and engine foundations: steps 21–35

This is a separate learning report for the steps after [11–20](editor-steps-11-20.md). It reviews HEAD `a48ff6c` and
current staged/unstaged/untracked implementation, not a new commit authored by DOC_MANAGER. Verification results and
remaining acceptance limits are recorded below.

**The product direction is one Angular editor containing editor controls and a game pane.** This batch provides typed
Angular 22 integration APIs and a Windows native viewport boundary. It adds no editor layout, mock screen, template
composition, or second UI application. The directive is available for the pane the UI owner chooses later; its presence
is not evidence that such a pane is already composed in the current setup screen. Runtime exposes logic; the frontend
owns layout, presentation, and input bindings.

The native game normally runs without editor overlays. Its prior native editor demo is now explicitly opt-in through
`GAME_ENGINE_NATIVE_EDITOR_DEMO=1`; that legacy demonstration is not the editor product or a new UI fork. Game App
Developer documentation remains empty.

## Architecture and exact source map

```text
Angular editor owner (layout/bindings still to be composed)
  EditorService signals + typed EditorCommand/EditorSnapshot models
  EngineViewportDirective on one chosen DOM pane
        Tauri async IPC / status + raw-input events
  EditorWorkspace (project CPU state / paths / save-import-prefab services)
        runtime::editor::EditorSession
          authored document, Play copy, revisions, selection
          validated command dispatch, transactions, bounded patch history
          schema3, hierarchy, prefabs, camera, fixed step, collision
  Windows child viewport (native airspace / render thread)
        scene assets + WindowRenderer
          lights, timestamps, instancing, resource reuse
```

| Source path                                                                                                                              | Responsibility                                                                                                                 |
|------------------------------------------------------------------------------------------------------------------------------------------|--------------------------------------------------------------------------------------------------------------------------------|
| `apps/editor/src/app/core/editor/editor.models.ts`                                                                                       | Typed frontend document, commands, capabilities, metrics, and input contract                                                   |
| `apps/editor/src/app/core/editor/editor.service.ts`                                                                                      | Queued native operations, authoritative snapshot signals, errors/recovery, viewport reservation/lifecycle                      |
| `apps/editor/src/app/core/editor/engine-viewport.directive.ts`                                                                           | Attach an existing Angular element to the viewport; observe bounds/visibility, emit raw input/status/errors, detach on destroy |
| `apps/editor-host/src-tauri/src/commands/editor.rs`                                                                                      | Trusted-window IPC validation and async CPU operations                                                                         |
| `apps/editor-host/src-tauri/src/editor.rs`                                                                                               | Project-scoped EditorWorkspace, bounded filesystem/import/prefab access                                                        |
| `apps/editor-host/src-tauri/src/viewport.rs`                                                                                             | Windows native child/window-event/render lifecycle and status metrics                                                          |
| `crates/bridge/src/editor.rs`                                                                                                            | Input-independent DTOs, protocol/capabilities, command descriptors                                                             |
| `crates/runtime/src/editor/mod.rs`, `operations.rs`, `history.rs`, `prefab.rs`                                                           | EditorSession, command semantics, transactions, patches, prefab data                                                           |
| `crates/runtime/src/scene_data.rs`, `scene.rs`, `scene_io.rs`                                                                            | Schema3 document, hierarchy/world transforms, CPU data and GPU scene loading/saving                                            |
| `crates/runtime/src/assets/scene_import.rs`, `gltf_loader.rs`, `asset_manager.rs`, `model.rs`                                            | Static glTF scene traversal, mesh metadata/cache, selected mesh GPU resources                                                  |
| `crates/runtime/src/simulation.rs`, `collision.rs`                                                                                       | Bounded fixed steps and conservative overlap/movement queries                                                                  |
| `crates/renderer/src/vulkan/lighting.rs`, `camera.rs` and shaders                                                                        | Directional/point light uniforms and shading                                                                                   |
| `crates/renderer/src/vulkan/instancing.rs`, `frame.rs`, `buffer.rs`, `device.rs`, `pipeline.rs`, `render_object.rs`, `push_constants.rs` | Instance data, async GPU timing, submitted metrics, batch submission and reuse                                                 |
| `compiler/src/lib.rs`, `apps/admin-dashboard/src-tauri/src/builder.rs`                                                                   | Asset JSON inclusion, bounded copying/validation and source generation                                                         |

The new CPU document EditorSession is distinct from the older SceneEditor operating on GPU-backed ModelInstances and the
status-only Engine/Renderer wrapper. Do not assume their history or IDs are interchangeable. Persistent keys cross the
transport; document commands do not carry raw GPU handles.

## Design patterns in this implementation

The Command pattern uses the closed, serializable `EditorCommandDto` enum, validated dispatch and reversible history
patches. It does not require a boxed executable trait object for every edit. A DTO is explicit at the Tauri boundary and
can be inspected, versioned and tested independently of DOM events or keyboard choices. `CommandFactory::capabilities()`
creates capability descriptors; it does not construct executable command objects.

`EditorService` is an application facade: it centralizes IPC, authoritative signals and a serialized document queue.
TypeScript `CommandBuilder` supplies ergonomic intent helpers, for example
`await editor.execute(CommandBuilder.select([selectedKey]))`. Rust
`EntityData::new(...).with_transform(...).with_parent(...)` is a fluent entity builder. `scene_io::instantiate` acts as
the GPU scene factory, validating document construction and acquiring shared resources through AssetManager rather than
duplicating upload logic in UI handlers. These patterns keep command intent reusable across buttons, menus and later
configurable shortcuts.

## Step 21: editor usability through typed Angular integration

**What and why.** Instead of expanding a native demo UI, EditorService/DTOs provide one command path shared by future
widgets, toolbars, menus, and key bindings. EditorSession owns command semantics, not DOM events. Tauri's
EditorWorkspace holds CPU state; blocking filesystem/import work executes off the async thread. A child viewport renders
inside the chosen pane on Windows.

**Files:** Angular core/editor files, host editor/viewport/commands modules, bridge/editor.rs, and runtime/editor. No
app layout/template was added.

**Example:** after `await editor.open(assetRoot, scenePath)`, use
`await editor.execute({type: 'select', keys: [entityKey]})`. Read `editor.selection()`, `editor.document()`,
`editor.dirty()`, and `editor.busy()` for UI state. Check `loadCapabilities()` before offering commands. Attaching
`[engineViewport]` is a future pane integration decision, not a template installed by this report.

Requests carry expected_revision. Stale edits fail rather than silently mutating a newer document. The service refreshes
the authoritative in-memory snapshot on recoverable rejection; it does not auto-retry or reload disk and discard unsaved
changes. Document and viewport queues are separate. One Angular element reserves the embedded viewport. Destroy
detaches/releases listeners; bounds and visibility observers govern native child placement. Raw viewport input is
emitted for a UI owner to map; there are no hardcoded frontend keyboard policies.

Windows native airspace means ordinary HTML cannot simply paint over the child viewport. Unsupported platforms report
capability limits. Interactive child placement/focus/DPI behavior remains a verification gate.

**Reading:
** [Angular signals](https://angular.dev/guide/signals), [Tauri frontend-to-Rust commands](https://v2.tauri.app/develop/calling-rust/),
and `bridge::editor::EditorCapabilitiesDto` in source.

## Step 22: JSON-aware packaging

**What and why.** Scene JSON now travels with model/image assets in both compiler installation and admin source
generation. This resolves the old builder allowlist omission that could compile successfully but fail startup because
robots.json was missing.

**Files:** `compiler/src/lib.rs`, `apps/admin-dashboard/src-tauri/src/builder.rs`.

**Example:** preview a generated native workspace and confirm `apps/game/assets/scenes/robots.json` appears alongside
`props/robot.glb`. This is a source preview check; a subsequent generated executable still needs its own runtime
acceptance.

Traversal is bounded and ordered. Compiler copying collects/checks entries before output, rejects links/unsupported
entries, rechecks containment, and checks copied length. Builder allows asset JSON, validates JSON, and rejects
secret-like metadata rather than broadly copying every JSON file in the checkout. Asset JSON is capped at16MiB even when
a valid scene can fit the32MiB parser ceiling. These checks reduce accidental leakage and malformed output; they do not
prove an installed game has been launched outside the checkout.

**Reading:** [Rust filesystem operations and TOCTOU](https://doc.rust-lang.org/std/fs/) and the compiler/builder tests
in the repository.

## Step 23: frame, orbit, pan, and dolly camera commands

**What and why.** Camera intent is part of the runtime editor contract. Angular tools can frame selection or move the
camera without duplicating transform math or owning the domain document.

**Files:** `runtime/src/editor/operations.rs`, bridge editor command enum, and EditorService.execute.

**Example:** `await editor.execute({type: 'frame_selection'})`; orbit takes yaw/pitch radians, pan takes right/up
distances, and dolly takes a distance. UI input decides when to send these commands. Frame selection uses selected
bounds; operations preserve valid camera projection/pitch and reject non-finite values.

The camera pivot derives from selection/bounds rather than a fixed robot. Bounds metadata is cached by model path/mesh.
This does not add mouse bindings, an orbit toolbar, or a composed Angular camera inspector.

**Reading:
** [PBRT transformations](https://www.pbr-book.org/4ed/Geometry_and_Transformations/Transformations), [Vulkan depth conventions](https://docs.vulkan.org/guide/latest/depth.html).

## Step 24: snapping settings and commands

**What and why.** Runtime SnapSettingsDto and typed transform intent make snapping consistent across UI controls.
Snapping is not implemented by rounding a displayed input alone.

**Files:** bridge/editor.rs, runtime/editor/mod.rs and operations.rs.

**Example:** send `set_snap` with `{enabled: true, translation: 0.25, rotation_radians: Math.PI / 12, scale: 0.1}`, then
a transform command with `snap: true`. A command request and configured enablement determine use. Translation, rotation
and scale increments are finite/positive validated settings; rotations remain radians in the contract.

Snapping applies to supported edit operations. A UI must display the chosen policy and pass intent; no new snapping
controls or key bindings were installed into app layout.

**Reading:** [Rust typed enums](https://doc.rust-lang.org/book/ch06-01-defining-an-enum.html) and the runtime
snap/transform tests.

## Step 25: multi-selection and grouped undo

**What and why.** EditorSession uses a validated list of persistent entity keys rather than a single transient ID.
Multi-entity transforms are one domain operation, and explicit transactions make a long drag/gesture one undo step.

**Files:** runtime/editor/mod.rs, operations.rs, history.rs; bridge editor selection/request DTOs.

**Example:** select two keys; send begin_transaction with a label, issue validated edits as the gesture changes, then
commit_transaction. Cancel restores the transaction's prior document/selection. Save/reload reject an active
transaction; callers must choose commit or cancel.

The session caps selection at 4096 keys and patch history at 128 records / 16 MiB, accounting for owned data rather than
JSON length alone. A patch records changed entity/camera/light/binding/selection values, not every GPU resource or a
copy of the entire document per normal edit. Invalid/no-op changes do not become successful new edits. World-space
selection roots avoid transforming a selected child's inherited motion twice.

This new CPU document history is separate from the 256-record legacy SceneEditor history described in the steps 11–20
report.

**Reading:
** [Rust enums](https://doc.rust-lang.org/book/ch06-01-defining-an-enum.html), [Arc ownership](https://doc.rust-lang.org/std/sync/struct.Arc.html),
and runtime editor transaction/history tests.

## Step 26: parenting and world transforms

**What and why.** Schema entities may reference a parent key. World matrices are resolved from validated local TRS data
and parent chains; rendering, picking, selection bounds, collision, and editor transforms use the resulting world
placement.

**Files:** runtime/scene_data.rs and scene.rs, editor/operations.rs, picking.rs.

**Example:** `reparent` with `{key: child, parent: parentKey, keep_world: true}` keeps visible placement by calculating
a new local transform. `parent: null` makes a root. Missing parents/cycles and unsuitable singular/sheared
decompositions fail rather than silently changing shape. A keep-world reparent may be impossible in a TRS-only
representation and is not guaranteed to succeed.

Traversal is iterative/indexed to avoid recursive stack growth. Parenting establishes transform hierarchy, not an ECS,
skeleton animation system, or a fully composed hierarchy tree UI.

**Reading:** [PBRT transformations](https://www.pbr-book.org/4ed/Geometry_and_Transformations/Transformations) and the
scene-data world-matrix/reparent tests.

## Step 27: schema version 3

**What and why.** Current saves use version 3, adding parent, renderable, mesh_index, material override, collider, and
scene lights to keyed entities/bindings/camera. A nonrenderable entity can represent an imported transform node without
pretending it has drawable geometry.

**Files:** runtime/scene_data.rs and scene_io.rs; typed bridge/frontend models.

**Example:** load a v2 scene through parse_json and save through the current API: existing keys/bindings are preserved,
new fields gain compatible defaults, and output is v3. Version 1 still receives generated legacy keys and both older
versions receive a migrated default light. Reading does not rewrite disk by itself. Future versions fail explicitly.

Scene JSON is bounded to 32 MiB, 100,000 entities, and 16 lights. Validation includes finite values, identity
uniqueness, paths, hierarchy references/cycles, projection, materials/colliders/lights, and resolved transforms. The
limits are acceptance ceilings, not measured real-time capacity.

**Reading:** [Serde data model](https://serde.rs/data-model.html) and the repository's schema validation/migration
tests.

## Step 28: prefab documents and instantiation

**What and why.** PrefabData is a bounded reusable subtree document distinct from a full scene save. Instantiation
creates fresh persistent identities and remaps internal parent references instead of aliasing another scene's keys.

**Files:** runtime/editor/prefab.rs; host editor workspace and prefab IPC; frontend service export/instantiate/save/load
methods.

**Example:** select a subtree, `exportPrefab()`, then `instantiatePrefab(prefab, position)` to add it at a placement
point as one undoable edit. Export includes selected descendant content, detaches external root parents while preserving
placement, and stores relative root positions. A prefab cannot keep an unresolved external parent.

Prefab schema is version 1, limited to 1–4096 entities and 16 MiB. Host prefab files are scoped to project paths and
validated on load/save. There is no linked/live prefab update system, override inheritance UI, or cross-project asset
copying implied by this format.

**Reading:** [Rust HashMap](https://doc.rust-lang.org/std/collections/struct.HashMap.html) for key remapping and the
prefab validation/history tests.

## Step 29: static glTF scenes, nodes, and meshes

**What and why.** The importer now distinguishes mesh import from full **static** scene traversal. It chooses the glTF
default scene or first available scene, visits node hierarchy, retains local/world transforms and mesh selection, and
reuses imported mesh resources rather than drawing only mesh zero at the origin.

**Files:** runtime/assets/scene_import.rs, gltf_loader.rs, asset_manager.rs, model.rs; host
EditorWorkspace.import_model.

**Example:** `importModel('models/static-building.glb', [0, 0, 0])` creates keyed entities for imported nodes and remaps
parent references; transform-only nodes are nonrenderable. Scene references can select mesh_index from the source asset.
Repeated references reuse mesh/material resources.

Traversal/import budgets reject excessive graphs, cycles and multiple-parent node misuse. Selected unique meshes have a
256 MiB aggregate decoded-geometry preflight budget, including expanded indices. Transform-only imported groups do not
create phantom mesh-zero picking or framing targets. Static scene import rejects skins/morph behavior, and animated
behavior is not implemented. This is not animation/skinning support, every glTF material extension, or unrestricted
arbitrary file access. Mesh-only legacy imports remain a distinct API path.

**Reading:
** [Khronos glTF scenes and nodes](https://github.khronos.org/glTF-Tutorials/gltfTutorial/gltfTutorial_004_ScenesNodes.html), [mesh primitives](https://github.khronos.org/glTF-Tutorials/gltfTutorial/gltfTutorial_009_Meshes.html).

## Step 30: material tint overrides

**What and why.** An entity can store an optional base-color factor override and edit it through the common
command/history path. Shared source materials/textures stay reusable rather than being destructively changed for every
tinted instance.

**Files:** runtime/scene_data.rs, editor/operations.rs, scene.rs; renderer RenderObject/shader instance data; bridge
material command.

**Example:** `set_material` with `{key, material: {base_color_factor: [0.8, 0.3, 0.2, 1]}}` adds a tint;
`material: null` removes the override. The draw path combines entity tint with source material color. Persistence and
undo use scene data, not GPU pointer identity.

A tint override is not a full material graph, texture-slot editor, PBR override system, transparency solver, or a newly
composed Angular inspector.

**Reading:
** [Khronos glTF materials](https://github.khronos.org/glTF-Tutorials/gltfTutorial/gltfTutorial_010_Materials.html) and
renderer tint/instance-data tests.

## Step 31: directional and point lights

**What and why.** Scene lights and renderer light uniforms replace a single hardcoded directional source. Typed editing
supports insertion/update/removal, while the renderer validates and uploads a bounded light list.

**Files:** runtime/scene_data.rs, editor operations, renderer/vulkan/lighting.rs and camera.rs, triangle shaders.

**Example:** upsert a directional light with key/name/direction/color/intensity, or a point light with position/range.
Up to 16 lights are validated; colors/intensity must be finite/nonnegative and directions/ranges valid. Directional
direction describes where rays travel. Point lighting includes distance/range response. Editor overlays remain unlit.

This is basic lighting with ambient contribution, not shadows, PBR, image-based light, light gizmos, or
performance/appearance acceptance. Existing v1/v2 data receives a compatibility default source during migration.

**Reading:
** [glTF material overview](https://github.khronos.org/glTF-Tutorials/gltfTutorial/gltfTutorial_010_Materials.html), [Vulkan shader interface](https://docs.vulkan.org/guide/latest/shader_memory_layout.html),
and LightingUniform layout tests.

## Step 32: fixed 60 Hz simulation

**What and why.** FixedStepper decouples simulation update size from rendering frequency. Both runtime editor Play and
standalone behavior can use bounded catch-up work rather than feeding a long stalled frame directly into movement.

**Files:** runtime/simulation.rs, editor/mod.rs tick, game/game.rs and main.rs integration.

**Example:** `FixedStepper::default().advance(1.0 / 30.0)` reports two 1/60-second steps. Default rate is 60 Hz with at
most eight updates per call. At most 0.25 seconds enters the accumulator; excess time and catch-up work beyond the cap
are reported as dropped_seconds. Interpolation stays in [0,1).

Pause resets the accumulator. Session Play movement uses typed movement intent and current selection, not hardcoded
keyboard mapping inside runtime. Error paths can pause/reset simulation rather than repeatedly applying invalid
movement. The standalone game separately uses WaitUntil deadlines at update-start plus approximately 16.667 ms for
continuous redraw, including draw time rather than adding another full delay afterward. Input/resize can wake it
immediately; paused/static, unfocused, occluded and minimized states wait. Hardware cadence has not been measured. A
fixed step is not rollback networking, strict cross-platform determinism, or interpolation already composed into Angular
display.

**Reading:** [Glenn Fiedler: Fix Your Timestep](https://gafferongames.com/post/fix_your_timestep/) and FixedStepper
boundary tests.

## Step 33: conservative collision foundation

**What and why.** Schema box/sphere colliders support bounded overlap queries, debug data, and simple kinematic
translation checks. Movement can stop when swept bounds may touch an obstacle instead of ignoring all scene collision
data.

**Files:** runtime/collision.rs, scene_data.rs, editor command/collision IPC and Play tick.

**Example:** assign `set_collider` box half_extents/offset or sphere radius/offset; `queryCollisions()` obtains a
CollisionSnapshot. `collision::move_entity` translates an entity subtree in world space and preserves local
rotation/scale. A blocked move can succeed without changing position; invalid input/budget exhaustion errors before
mutation.

Rotated/scaled colliders use conservative enclosing bounds, including sphere approximation where needed. Work/contact
budgets protect CPU use. False-positive blocking is possible. There is no rigid-body solver, gravity/inertia, contact
impulses, sliding/stepping, triangle mesh collision, or certified continuous physics accuracy.

**Reading:** [PBRT bounding boxes](https://www.pbr-book.org/4ed/Geometry_and_Transformations/Bounding_Boxes) and
collision swept-movement/property tests.

## Step 34: asynchronous GPU timestamps and submission metrics

**What and why.** The renderer now measures an actual completed GPU command range rather than relabeling CPU update
duration. Capability and optional values expose unsupported/not-yet-ready cases honestly.

**Files:** renderer/vulkan/frame.rs, device.rs, instancing.rs; WindowRenderer.last_metrics; viewport status DTO/event
and Angular service models.

**Example:** after frames execute, read `gpu_timestamps_supported` and optional `gpu_time_ms` from viewport status. The
value belongs to a previously completed frame, not the command request currently in flight. DrawMetrics includes
submitted draws, instances, triangles and instanced draw groups.

Two timestamps use device timestamp period and valid-bit wrapping. Availability is checked after the existing frame
fence with no query WAIT flag added. Measurements exclude CPU work, image acquisition, presentation/display pacing and
are not GPU utilization. A null value is not zero cost. No speedup, utilization percentage, or frame budget is claimed
without workload/machine measurements.

**Reading:
** [Vulkan vkGetQueryPoolResults](https://docs.vulkan.org/refpages/latest/refpages/source/vkGetQueryPoolResults.html), [vkCmdWriteTimestamp](https://docs.vulkan.org/refpages/latest/refpages/source/vkCmdWriteTimestamp.html),
and timestamp conversion/layout tests.

## Step 35: Vulkan instancing and reuse

**What and why.** Compatible opaque RenderObjects can share one indexed instanced draw. Per-instance model matrices and
tint live in a reusable host-visible vertex buffer, avoiding one otherwise identical submission per object.

**Files:** renderer/vulkan/instancing.rs, frame.rs, buffer.rs, pipeline.rs, shaders and push constants; asset/model
reuse in runtime.

**Example:** duplicate a static model several times with compatible mesh/material state. The renderer groups compatible
opaque runs and calls indexed drawing with instanceCount. InstanceData is an 80-byte matrix+tint record at binding
1/locations 3–7; the base Vertex stays 32 bytes at binding 0. Tint multiplication preserves per-instance color.

Overlay, unlit and alpha/nonbatchable draws form ordering barriers. The renderer must not sort UI through scene geometry
or assume all shared-looking materials are interchangeable. The instance buffer starts with capacity and grows as
necessary up to a bounded maximum; normal reuse is not a claim of no allocations during growth. Frame statistics report
actual batches, not a pre-batch estimate. Cached resources and scene reconciliation preserve reusable uploads.
AssetManager limits logical GPU resources to 512 MiB and 8,192 resources; its weak-reference ledger continues accounting
for externally held model references after clear/prune. Unused cached resources are pruned after GPU idle during
structural reconciliation and failure cleanup. These logical limits are not a measurement of total driver allocation.
Scene draw expansion rejects more than 65,536 render objects before growing the output vector; production callers
propagate the fallible conversion error.

This is CPU-built instance batching, not GPU-driven indirect rendering, occlusion culling, every transparent draw
optimized, or a measured throughput gain.

**Reading:
** [Vulkan vkCmdDrawIndexed](https://docs.vulkan.org/refpages/latest/refpages/source/vkCmdDrawIndexed.html), [vertex input rate](https://docs.vulkan.org/refpages/latest/refpages/source/VkVertexInputRate.html),
and batching/order-barrier tests.

## Integration example without adding a UI

The frontend owner can use the existing service inside a future component/controller:

```typescript
await editor.loadCapabilities();
await editor.open(assetRoot, scenePath);
await editor.execute({type: 'select', keys: [selectedKey]});
await editor.execute({type: 'begin_transaction', label: 'Move selection'});
await editor.execute({type: 'translate_selection', delta: [0.25, 0, 0], snap: true});
await editor.execute({type: 'commit_transaction'});
await editor.save();
```

This is documentation of command intent, not a new component or composed template. UI shortcuts, selection gestures,
dialogs, pane layout, toolbar state and accessibility presentation belong to the Angular app. The runtime contract does
not assign key bindings. A single enabled EngineViewportDirective owns the native child; status/input/error outputs let
that owner decide interaction. Windows native airspace and unsupported-platform status must be reflected by the eventual
UI.

## Operational logging

The host logs stable operation categories for editor commands and viewport failures. A transition into failed viewport
state emits one ERROR; thread/event-loop/cancel and idle-wait failures are explicit boundaries. Gated DEBUG records
describe cold GPU creation counts and teardown. Input, redraw, bounds and polling do not generate per-event log spam.
This targeted instrumentation does not certify a full logger infrastructure audit.

## Verification and remaining work

The following engine results are attributed to implementation/TESTER/reviewer handoffs. Documentation maintenance
rebuilt and checked the site separately.

| Check                                                                    | Result and scope                                                                                                                                                                                    |
|--------------------------------------------------------------------------|-----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `cargo test --workspace --all-targets --offline --locked --no-fail-fast` | **294 passed, zero failed, two ignored**; TESTER added 62 Rust tests and updated schema/Windows Temp fixtures                                                                                       |
| Angular editor suite                                                     | **13 passed**, including 10 new service tests for revisions, conflict recovery, bounded queues, viewport ownership/coalescing and cleanup                                                           |
| Explicit ignored native Vulkan initialization                            | Passed separately; the copied-template native test also passed in a separate explicit run                                                                                                           |
| Explicit copied-template native validation                               | One passed; generated workspace manifest/artifacts validated. This does not establish installed release execution                                                                                   |
| Hidden `renderer_smoke`                                                  | Passed: 257 compatible instances in one draw, completed asynchronous timestamps, tint/light/resource mapping, retained GPU accounting/pruning, render-object budget, resize/suspension and shutdown |
| Hidden `scene_io_smoke`                                                  | Passed load/save/reload, shared resources, candidate rendering, invalid/partial-load rollback, retirement and shutdown                                                                              |
| Workspace all-target offline/locked check                                | Passed                                                                                                                                                                                              |
| Final `cargo build -p game -p app --offline --locked`                    | Passed in the ordinary target directory                                                                                                                                                             |
| Workspace Clippy                                                         | Production bins/lib and TESTER all-target checks passed; inherited argument-count/modulo/example-style warnings and cache hardlink fallback remain                                                  |
| Angular production build / strict TypeScript                             | Passed, including final lifecycle/input type changes                                                                                                                                                |
| SECURITY static re-review                                                | Both resource P2 findings and failed-candidate cleanup concern closed; this is scoped review evidence                                                                                               |
| LOGGER targeted changes                                                  | Editor/viewport operation-boundary instrumentation and host check passed; no full infrastructure audit claimed                                                                                      |
| Documentation site                                                       | 27 pages rebuilt/link-checked; all five site tests and 416 local/cross-page anchor checks passed                                                                                                    |

TESTER reports coverage for hierarchy rollback, revisions/transactions/history budgets, prefabs, collision sweeps, fixed
ticks, static glTF caches and packaging. Bundled `robots.json` remains unchanged (SHA256
`63A13AC7E1B0FEC1E00C66A85DE3169897D01BF989842F7037B523140107AA42`). The inspection baseline and uncommitted tree are
recorded in `docs-site/docs-state.json`; no files were staged or committed by documentation maintenance.

No interactive visual acceptance has been selected for this batch: hidden native examples, when supplied, are separate
execution evidence. No measured speedup or GPU-utilization claim is made. Scene hierarchy,
prefab/schema/import/lighting/collision limits above remain explicit even when automated tests pass.

Unresolved prior limits include installed release/clean-checkout/Jenkins evidence, full
material/PBR/skinning/animation/physics, backend-independent GPU handles, and composing the actual Angular
editor/game-pane UI. The earlier builder JSON omission is fixed by step 22; other historical limits are not silently
erased. Steps beyond 35 are future planning, not work implemented by this report.
