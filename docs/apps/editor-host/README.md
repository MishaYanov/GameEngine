# Editor desktop host

Source: `apps/editor-host/src-tauri`. Cargo package `app`, library `app_lib`. Tauri hosts the Angular app and exposes
project-scoped CPU editing plus an on-demand native game pane. It does not own frontend layout or hardcode editor
keyboard bindings.

## Responsibilities

- `editor.rs`: EditorWorkspace, project-root paths, scene load/save, asset metadata, model imports, prefabs and revision
  checks.
- `commands/editor.rs`: typed editor IPC and trusted main-window checks; blocking CPU/filesystem work uses async
  spawn_blocking.
- `viewport.rs`: one Windows native child, render-worker ownership, bounds/visibility/DPI, invalidation, status and
  raw-input events.
- `state.rs`: shared editor workspace, cached engine-status result and viewport state.
- `lib.rs`: command registration and parent lifecycle, including renderer shutdown before final parent close.

Startup initializes logging and CPU state. The cached engine_status probe queries Vulkan instance/physical-device
information without creating a logical device or claiming runtime health. The actual viewport owns presentation GPU
resources on demand. Either operation can report a renderer error without making eager GPU initialization a prerequisite
for opening the shell.

## Editor IPC

| Commands                                                                                          | Scope                                                              |
|---------------------------------------------------------------------------------------------------|--------------------------------------------------------------------|
| editor_capabilities / editor_open / editor_snapshot                                               | Protocol/schema descriptors and authoritative document             |
| editor_execute / editor_save / editor_reload                                                      | Revision-aware command dispatch, authored persistence, replacement |
| editor_assets / editor_import_model                                                               | Catalog and static glTF scene import                               |
| editor_export_prefab / editor_instantiate_prefab / editor_save_prefab / editor_load_prefab        | Prefab data and scoped files                                       |
| editor_collisions                                                                                 | Validated collision snapshot                                       |
| editor_viewport_attach / editor_viewport_bounds / editor_viewport_detach / editor_viewport_status | Single native-pane lifecycle                                       |

DTOs live in bridge::editor; EditorSession owns mutations and validation. Open accepts an asset root and a scene path
resolved within it. Workspace/project switches advance revision identity so stale commands cannot accidentally apply to
a replacement project. Save/reload enforce revision and grouped-edit state. Raw native input is emitted for the Angular
owner to interpret; it is not a domain command by itself.

The existing engine_status/create_project commands remain for status and the setup installer; they are separate from the
new editor document API.

## Windows viewport boundary

A worker owns native child/Vulkan state and responds to document/render invalidation. Idle Edit does not require a
perpetual render loop. Parent/DOM visibility, bounds, minimization, attach/detach and shutdown are explicit. Native
prepared pixel extents drive pointer/status coordinates and renderer resizing under DPI changes. Lifecycle transitions
clear held input; a static Play session with no movement also waits for invalidation. Angular owns the pane and UI.
Unsupported platforms report capability limitations. Native airspace prevents treating HTML as an unrestricted overlay
atop the child; UI composition is still future integration work.

## Development and packaging

From the root:

```powershell
.\dev.ps1 editor
cargo check -p app --all-targets --locked --offline
```

Tauri configuration uses the editor server on port 4200 and production `apps/editor/dist/editor/browser`. Build a bundle
from apps/editor-host using the editor's installed tauri CLI. Windows prerequisites and the SDK apply when exercising
native rendering.

Read [steps 21–35](../../editor-steps-21-35.md) for examples and learning resources, and [testing](../../testing.md) for
exact verification. No composed Angular editor layout, interactive child-window acceptance, or installer execution is
implied by IPC implementation.
