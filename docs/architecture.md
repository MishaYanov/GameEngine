# Workspace architecture

The intended product is one Angular editor with controls and a game pane. Runtime exposes editing/game logic, bridge
defines transport, and Tauri supplies project/native ownership. This batch implements integration APIs and engine
foundations, not a finished editor layout. Start with [usage](usage.md) and
the [steps 21–35 learning report](editor-steps-21-35.md).

## Projects and boundaries

| Source                     | Responsibility                                                                                                | Guide                                   |
|----------------------------|---------------------------------------------------------------------------------------------------------------|-----------------------------------------|
| apps/editor                | Angular22 models/service/directive; UI layout and key bindings belong here                                    | [Frontend](apps/editor/README.md)       |
| apps/editor-host/src-tauri | Project CPU workspace, async IPC, on-demand Windows child viewport                                            | [Host](apps/editor-host/README.md)      |
| crates/bridge              | Editor/engine DTOs, capabilities, input-independent command contracts                                         | [Bridge](crates/bridge/README.md)       |
| crates/runtime             | EditorSession, schema3, history/transactions, hierarchy/prefabs, camera, static imports, simulation/collision | [Runtime](crates/runtime/README.md)     |
| crates/renderer            | WindowRenderer, lights, actual GPU queries, batching/instancing, device/presentation lifecycle                | [Renderer](crates/renderer/README.md)   |
| apps/game                  | Normal standalone player; native editor demo requires explicit opt-in                                         | [Game](apps/game/README.md)             |
| apps/admin-dashboard       | Separate logs/process/jobs/board/source-generation tool                                                       | [Admin](apps/admin-dashboard/README.md) |
| compiler                   | Builds/installs executable plus validated asset tree                                                          | [Compiler](compiler/README.md)          |
| crates/logger              | Process diagnostics and bounded persistent records                                                            | [Logger](crates/logger/README.md)       |

## Editor data flow

Angular EditorService publishes authoritative signals and queues typed operations. EditorCommandRequest includes
expected_revision; native EditorWorkspace delegates to runtime::editor::EditorSession for validation/mutation.
Snapshot/capabilities tell widgets what is available. Input notifications do not become edits unless the frontend owner
maps them into intent. Stale requests fail; recovery refreshes memory rather than reloading disk or silently replaying
an edit.

EditorSession owns CPU authored data and temporary Play state. Patch history/transactions belong there, not in DOM
components or raw Vulkan objects. Command revision, rendered scene revision and workspace generation serve different
purposes and are not a single interchangeable counter.

The host initializes CPU state and logging without eager GPU construction. A cached EngineStatusDto is filled on demand
by Engine::probe_status and Renderer::probe_info, which inspect a Vulkan instance and physical-device information
without creating a logical device. This reports probe information, not runtime health. The viewport worker owns the
actual presentation GPU lifetime rather than retaining a second permanent headless engine device.

## Game pane and ownership

EngineViewportDirective can attach an existing Angular element, observe bounds/visibility, and detach on destruction. No
new pane layout/template was composed. The eventual UI owner chooses editor controls and game pane in the same Angular
app.

Embedding currently uses a Windows native child with airspace constraints; unsupported platforms report capabilities.
The render worker owns native/Vulkan state and reconciles scene/cache changes. Idle Edit waits for invalidation;
inactive Play input need not force continuous work. Parent close, minimization, bounds, DPI, attach/detach and worker
shutdown are explicit lifecycle concerns, not proof of interactive correctness.

The older native SceneEditor/panels are a legacy example under GAME_ENGINE_NATIVE_EDITOR_DEMO=1. Normal game execution
is overlay-free and does not create another editor UI fork. The Angular/Tauri integration and that historical demo have
different command/history ownership.

## Scene, simulation, and rendering

Schema3 extends persistent keyed scenes with parenting, transform-only nodes, selected mesh, tint, colliders and lights.
V1/V2 migrate in memory; prefabs remap isolated subtree identities. Static glTF import traverses a selected scene and
mesh nodes while rejecting unsupported dynamic behavior.

Default fixed steps are 60 Hz with bounded catch-up. Conservative collision queries and swept AABB movement are a
foundation, not rigid-body physics. Renderer world transforms, material tints and light uniforms feed compatible opaque
instancing. Optional asynchronous GPU time belongs to a completed command range; unsupported/no sample is not zero GPU
cost, and no speedup/utilization claim is made.

## Installation and diagnostics

Compiler and admin generation now include bounded validated asset JSON alongside model/image dependencies, fixing the
earlier scene allowlist omission. Generation remains a separate source-workspace flow from installing a release
executable. Successful copying/compilation does not prove installed runtime acceptance.

Executables initialize logger guards; libraries emit through them. Separate processes communicate diagnostics through
persisted session records, and admin owns its managed process lifecycle. Keep authored saves separate from simulation
state. See [persistence](scene-persistence.md), [testing](testing.md), and [pipeline](pipeline.md) for current limits.
