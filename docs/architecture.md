# Workspace architecture

This repository contains a native Vulkan demo, an Angular editor with a Tauri host, an independent administration application, and shared Rust crates. See [setup and workflows](usage.md) before running applications.

## Projects and responsibilities

| Source directory | Responsibility | Guide |
| --- | --- | --- |
| `apps/editor` | Angular project setup and GPU status | [Editor](apps/editor/README.md) |
| `apps/editor-host/src-tauri` | Desktop host, engine state, native commands; Cargo package `app` | [Editor host](apps/editor-host/README.md) |
| `apps/game` | Native window, demo scene, assets, drawing loop | [Game](apps/game/README.md) |
| `apps/admin-dashboard` | Independent logs, processes, jobs, board, source generation; Rust package `engine-admin` | [Engine Admin](apps/admin-dashboard/README.md) |
| `compiler` | Build and install the starter executable | [Compiler](compiler/README.md) |
| `crates/runtime` | Renderer initialization and engine status | [Runtime](crates/runtime/README.md) |
| `crates/renderer` | Vulkan initialization, GPU resources, frame drawing | [Renderer](crates/renderer/README.md) |
| `crates/bridge` | Serializable engine contracts | [Bridge](crates/bridge/README.md) |
| `crates/logger` | Diagnostics and bounded persistent storage | [Logger](crates/logger/README.md) |

## Editor command flow

The Angular `EngineService` invokes `engine_status`. The Tauri host locks `EditorState`, calls `runtime::Engine::status`, and serializes `bridge::EngineStatusDto` as camelCase JSON. The runtime owns a `renderer::Renderer`, which initializes a Vulkan instance/device and reports GPU information.

For project creation, the frontend invokes `create_project`; the host runs `compiler::install_project` on a blocking worker, then spawns the installed game as a separate process. Installation and process-spawn results are reported separately. Spawn success does not prove the child later initialized its GPU.

The host initializes its engine before showing the window, so Vulkan initialization failure stops startup. The editor currently provides project setup rather than an editable scene or embedded graphics viewport.

## Native rendering and generation

The game uses low-level `renderer::vulkan` types directly and owns its winit loop, surface, presentation device, swapchain, depth, camera, pipeline, synchronization, and assets. The runtime status wrapper does not drive its drawing loop. Development mode displays two rotating model instances containing every primitive of the first mesh from `props/robot.glb`; a readable `project-name.txt` selects executable-relative assets and stationary instances. The installer still omits the required robot model, so its current output fails model import; see the [game guide](apps/game/README.md). See [graphics and assets](graphics-and-assets.md).

| Workflow | Destination | Output |
| --- | --- | --- |
| Compiler CLI / editor | Existing absolute parent directory | Named child folder with release executable, textures, name file, and README |
| Admin Project Builder | New absolute directory outside source checkout | Independent Rust workspace validated in staging before publication |

The compiler embeds the source workspace location and builds from that checkout. Its output has no editable source project. The admin builder copies allowlisted inputs and retains failed staging. See [usage](usage.md) for destination rules.

## Diagnostics and ownership

Each executable initializes the shared logger once and keeps its guard alive; libraries emit through that logger. Applications exchange diagnostics through per-session JSONL files rather than shared memory. Engine Admin collects those files and maintains separate administration history.

Dashboard controls apply to its owned processes. Python launcher terminals have a separate lifetime. The editor does not explicitly stop the game child when it closes. See the [admin](apps/admin-dashboard/README.md) and [logger](crates/logger/README.md) guides.

The game has an in-memory scene with entity IDs, spawn/remove, and transform mutation. Editor entity editing, scene persistence, and a general runtime simulation loop are not established features. glTF/GLB import and textured rendering support all indexed triangle primitives of the first mesh with distinct materials. Models are shared between game-local scene instances; full glTF scene traversal, persistence, runtime/editor integration, and PBR remain outside that scope. PRD requirements include future work; consult implementation status and [testing limits](testing.md).
