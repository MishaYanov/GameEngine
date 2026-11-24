# Workspace architecture

The Angular Editor handles authoring controls and presentation. The Tauri host owns the open project and the
native child viewport. Engine Core owns authored documents, validation, command history and fixed-step simulation.
Runtime connects those CPU systems to assets, graphics and windowed apps.

Each project can retain several scene drafts. Entering Play creates a simulation from the active draft. Authored
scene data stays available during Play, and opening another project establishes a new workspace generation.
The Player and headless runner use the same simulation code. The headless runner can validate a project and
replay inputs without a window or GPU.

[Editor command flow](editor-flow.md) follows requests through these layers. [Project lifecycle](project-lifecycle.md)
describes the project, scene and editor protocol versions.

## Projects and boundaries

| Source                     | Responsibility                                                                                                               | Guide                                     |
|----------------------------|------------------------------------------------------------------------------------------------------------------------------|-------------------------------------------|
| apps/editor                | Angular22 models/service/directive; UI layout and key bindings belong here                                                   | [Frontend](apps/editor/README.md)         |
| apps/editor-host/src-tauri | Project CPU workspace, async IPC, on-demand Windows child viewport                                                           | [Host](apps/editor-host/README.md)        |
| crates/bridge              | Editor/engine DTOs, capabilities, input-independent command contracts                                                        | [Bridge](crates/bridge/README.md)         |
| crates/engine-core         | CPU coordinator, commands/history, project/scene documents, shared simulation, world/collision caches and bounded asset jobs | [Project lifecycle](project-lifecycle.md) |
| apps/headless              | CPU-only project/scene execution and bounded replay                                                                          | [Testing and replay](testing.md#headless-replay)    |
| crates/runtime             | Core re-exports, GPU scene synchronization, asset import/cache, native SceneEditor adapter and GraphicsProbe                 | [Runtime](crates/runtime/README.md)       |
| crates/renderer            | WindowRenderer, lights, actual GPU queries, batching/instancing, device/presentation lifecycle                               | [Renderer](crates/renderer/README.md)     |
| apps/game                  | Development learning harness from source; installed projects use Player                                                      | [Game](apps/game/README.md)               |
| apps/admin-dashboard       | Separate logs/process/jobs/board/source-generation tool                                                                      | [Admin](apps/admin-dashboard/README.md)   |
| compiler                   | Builds/installs executable plus validated asset tree                                                                         | [Compiler](compiler/README.md)            |
| crates/logger              | Process diagnostics and bounded persistent records                                                                           | [Logger](crates/logger/README.md)         |

## Editor data flow

Angular EditorService publishes authoritative signals and queues typed operations. EditorCommandRequest includes
expected_revision; native EditorWorkspace delegates to the engine-core Engine/EditorSession coordinator for
validation/mutation.
Snapshot/capabilities tell widgets what is available. Input notifications do not become edits unless the frontend owner
maps them into intent. Stale requests fail; recovery refreshes memory rather than reloading disk or silently replaying
an edit.

EditorSession owns CPU authored data and temporary Play state. Patch history/transactions belong there, not in DOM
components or raw Vulkan objects. Command revision, rendered scene revision and workspace generation serve different
purposes and are not a single interchangeable counter.

The host initializes CPU state and logging without eager GPU construction. A cached EngineStatusDto is filled on demand
by GraphicsProbe::probe_status and Renderer::probe_info, which inspect a Vulkan instance and physical-device information
without creating a logical device. This reports probe information, not runtime health. The viewport worker owns the
actual presentation GPU lifetime rather than retaining a second permanent headless engine device.

## Game pane and ownership

The Scene and Game views host a native Windows child viewport through the Angular viewport directive. The directive
observes bounds and visibility and releases its attachment on destruction. The render worker owns native surface
state and Vulkan resources. It reconciles scene changes, resize, minimization, DPI and focus.

The source game has a Development mode for the engine's native examples. Exported projects start in Player mode.
Read [Editor viewport](viewport-implementation.md) and [Player modes](apps/game/README.md#modes-and-authored-gameplay).

## Scene, simulation, and rendering

The [scene format](scene-persistence.md) stores object keys, hierarchy, components, the default game camera
and scene settings. Runtime uses current handles to execute this data. Props keep authored source identity and
give placed objects their own scene keys.

The shared engine advances simulation at 60 Hz with bounded catch-up and uses collision queries to constrain
movement. Runtime sends current transforms, meshes, materials and lighting to the renderer. Compatible opaque
models can share instanced draws. [Timing](timing-input-camera.md) describes the fixed step, and
[graphics](graphics-and-assets.md) covers meshes, materials and asynchronous GPU statistics.

## Installation and diagnostics

The compiler packages validated scene data and referenced model and image dependencies with the Player.
The Admin Dashboard generates editable source workspaces and manages development jobs. Use the Editor's
Export Game… action when shipping an authored project.

Executables initialize logger guards; libraries emit through them. Separate processes communicate diagnostics through
persisted session records, and admin owns its managed process lifecycle. Keep authored saves separate from simulation
state. See [persistence](scene-persistence.md), [testing](testing.md), and [pipeline](pipeline.md) for current limits.
