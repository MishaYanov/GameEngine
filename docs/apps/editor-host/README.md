# Editor desktop host

Source: `apps/editor-host/src-tauri`. Cargo package `app`, library `app_lib`. Tauri hosts the Angular app and exposes
project-scoped CPU editing plus an on-demand native scene/game pane. It owns native spatial input bindings and tool state; Angular owns frontend layout and adjacent controls.

## Responsibilities

- `project_location.rs`: canonical project root, project.json and contained assets ownership; typed ProjectOpenError.
- `editor.rs`: EditorWorkspace, project-root paths, scene load/save, asset metadata, model imports, prefabs and revision
  checks.
- `commands/editor.rs`: typed editor IPC and trusted main-window checks; blocking CPU/filesystem work uses async
  spawn_blocking.
- `viewport.rs`: one Windows native child, render-worker ownership, bounds/visibility/DPI, invalidation, status and
  raw-input events, applied view-action replies and editor-state notifications.
- `viewport_interaction.rs`: editor camera, navigation, picking, marquee, handle transactions, placement, import ownership and caches.
- `viewport_tools.rs`: window-independent projection/ray/grid/gizmo/transform math.
- `state.rs`: shared editor workspace, cached engine-status result and viewport state.
- `lib.rs`: command registration and parent lifecycle, including renderer shutdown before final parent close.

Startup initializes logging and CPU state. The cached engine_status probe queries Vulkan instance/physical-device
information without creating a logical device or claiming runtime health. The actual viewport owns presentation GPU
resources on demand. Either operation can report a renderer error without making eager GPU initialization a prerequisite
for opening the shell.

## Editor IPC

`editor_capabilities` includes the native gameplay-module catalog and module-owned role hints. The Angular selector
uses this data rather than a hardcoded Courtyard list. `set_collider` continues through the ordinary core command path;
collider presence changes use full snapshots and interaction/dimension changes use authoritative entity deltas.
`collider_visuals.rs` adds cached selected-shape outlines with bounded line/hierarchy work and nonfatal diagnostics;
continuous cyan blocking outlines and dashed pink sensor outlines remain Scene-only. [Implementation review](../../building-blocks-review.md).

| Commands                                                                                          | Scope                                                                                |
|---------------------------------------------------------------------------------------------------|--------------------------------------------------------------------------------------|
| editor_capabilities / editor_open / editor_snapshot                                               | Protocol/schema descriptors and authoritative document                               |
| editor_execute / editor_save / editor_reload                                                      | Revision-aware command dispatch, authored persistence, replacement                   |
| editor_save_all                                                                                   | Persist dirty retained scene documents and project settings without switching scenes |
| editor_execute_incremental                                                                        | Atomic receipt plus changed UI data or full snapshot fallback                        |
| editor_assets / editor_import_model                                                               | Catalog and static glTF scene import                                                 |
| editor_export_prefab / editor_instantiate_prefab / editor_save_prefab / editor_load_prefab        | Prefab data and scoped files                                                         |
| editor_collisions                                                                                 | Validated collision snapshot                                                         |
| editor_viewport_attach / editor_viewport_bounds / editor_viewport_detach / editor_viewport_status | Single native-pane lifecycle                                                         |
| editor_viewport_action / editor_viewport_editor_state                                             | View/tool actions, Restart Run, applied native state and Game status                 |
| editor_prepare_project_build / editor_finish_project_build / editor_run_build                     | Staged export, background compilation, retry and explicit player launch              |

Document DTOs live in bridge::editor; view/tool DTOs live in bridge::viewport. EditorSession owns mutations and validation. Compatibility Open accepts an asset root and a scene path
resolved within it. Workspace/project switches advance revision identity so stale commands cannot accidentally apply to
a replacement project. Save/reload enforce revision and grouped-edit state. Raw native input is interpreted by ViewportInteraction and remains available as an observation event; it is not a domain command by itself.

`engine_status` remains a status probe. `create_project` creates a validated authoring manifest/assets tree without compiling a runtime or spawning a game process. It returns the project path and manifestPath for setup's guarded Open in Editor flow.

Setup also exposes `choose_project_destination` to the trusted main window. It opens a parented native folder dialog
on a blocking worker and returns a selected path or null on cancellation. An existing absolute initial directory can
seed the dialog. Choosing a folder does not install anything; the compiler still validates the final destination.

`editor_execute_incremental` executes and captures its response under the same workspace lock. It returns
`EditorCommandUpdateDto::Delta` with lightweight editor state, changed authored entities and optional camera/lights.
Full resync, topology, project, binding or entity-order changes return `Snapshot` instead. Capability negotiation
advertises `incremental_commands: true`; the legacy full-snapshot command remains available. The handler emits
`editor://changes-ready` with the receipt cursor after execution. This reduces returned data for compatible edits;
command/history processing may still clone full scene data.

The native worker publishes `game_status` in applied viewport state as its fields change. The status includes
Play session and run IDs, current runtime scene, pause and pending destination, and optional objective/detail/score
and completion from the module. Angular matches the Play session ID to its authored snapshot. Progress-only
updates use this viewport channel without refreshing the authored document.

Runtime requests prepare the retained destination and stage renderer resources before committing. Workspace
generation and candidate checks guard adoption. `restart_run` also validates the current run ID and expected
authored revision, and restarts at the retained startup scene. A successful restart clears input and changes the
run ID within the same Play session. Preparation failure leaves the previous scene and progress installed and
reports an error in
Game. [Runtime and authoring identity](../../project-lifecycle.md#editor-play-and-simulation-scene-identity).

## Project lifecycle and multi-scene persistence

`editor_open_project` uses ProjectLocation and prepares every catalog scene before installing a candidate. Project
schema 3 uses stable scene IDs, scene-scoped behaviors, shared settings, and an optional `gameplay_module`.
Inline-only schema-2 JSON upgrades on load. Capabilities advertise protocol 2 / scene schema 3.
The host installs the shared project resolver when opening or reloading a workspace. `StartPlay` resolves the
current in-memory manifest before creating a runtime session. An unsupported ID rejects Play and preserves the
authored state. The existing project snapshot and `set_project` command carry the field over protocol 2.
`editor_register_scene` registers an existing file, while `editor_save_scene_as` writes a new destination and changes
the active scene path. `editor_save` saves only the active scene; `editor_save_project` saves the manifest. `editor_save_all` preflights all catalogued outputs and advances each successful saved baseline before later writes, so a partial failure can be retried accurately.

`editor_execute_receipt` and `editor_changes` expose coordinator receipts/change pages. Catalog operations travel
through typed commands. `editor_reload` accepts `require_clean` (default when omitted) or explicit `discard_unsaved`; it
validates all replacement documents and refreshes generation/jobs/caches only after success. Manifest-backed state never
silently falls back when project.json disappears. Build preparation compares every scene with disk before releasing the
workspace lock for compilation. The split export API stages verified source assets and manifest before preparation
acknowledges success. Finish verifies the staged manifest and assets, compiles outside the workspace lock, and retains
failed tokens for retry unless superseded. `editor_run_build` checks the latest completed build, verifies its data and
executable fingerprints, and launches that player. Use an output folder writable only by trusted users.
See [project lifecycle](../../project-lifecycle.md) for failure behavior and frontend
helpers.

## Guarded project replacement and native dialogs

`choose_project_manifest` opens a parented native JSON chooser and returns a path or null on cancel. `confirm_project_discard` asks whether unsaved scene/project changes may be discarded. Both enforce the trusted main-window policy and run blocking dialogs outside the WebView thread.

`editor_open_project` accepts optional expected_revision and discard_unsaved (default false). Before candidate installation, ProjectReplacement compares the reviewed revision against the current workspace, rejects Play/open transactions and requires permission for dirty replacement. It checks under the installation lock, so edits made during dialogs or preparation cannot be discarded using stale approval. Null expected revision only matches no current workspace. The older editor_open compatibility path bypasses this guard. See [editor workspace](../../editor-workspace.md) for the connected UI and its limits.

## Windows viewport boundary

A worker owns native child/Vulkan state and responds to document/render invalidation. Idle Edit does not require a
perpetual render loop. Parent/DOM visibility, bounds, minimization, attach/detach and shutdown are explicit. Native
prepared pixel extents drive pointer/status coordinates and renderer resizing under DPI changes. Lifecycle transitions
clear held input; a static Play session with no movement also waits for invalidation. The Scene camera is editor-only; navigation does not dirty scene data or history. Game uses the runtime camera and hides editor decoration. Scene stays available for read-only inspection during Play.

Native view actions are bounded and acknowledged after application. The worker publishes `editor://viewport-editor-state` for camera/tool presentation and `editor://viewport-document-changed` for authored changes. Camera-only notifications do not request full Angular document snapshots. Pick bounds and selected transform data are cached independently of camera movement.

Queued actions capture the actual workspace generation and recheck it with their expiry under the mutation lock. Native transform movement publishes compact Inspector previews instead of full-document notifications; begin/commit/cancel supply document boundaries. Captured drag decoration avoids repeated world-matrix maps. Import completion reconciliation also handles the notifier or another IPC drain consuming the result first.

The analytic grid, bounding-box selection feedback, transform handles and placement marker use editor-only Vulkan passes. The grid depth-tests without writing depth; overlay lines are expanded to clipped quads with stable pixel widths. Native airspace keeps the Angular toolstrip outside the child. Internal model dragging uses a WebView pointer-capture broker and normalized aperture coordinates, with asynchronous asset adoption. Unsupported platforms report capability limitations.

Read the [full viewport implementation](../../viewport-implementation.md) for exact gestures, transaction/capture cleanup, bounds-picking and affine-transform limits, renderer ownership and current evidence.

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
exact verification. An implemented command path or passing fake-driver test does not itself establish visible child-window/GPU acceptance or installed-player execution.
