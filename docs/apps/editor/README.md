# Angular editor integration

Source: `apps/editor`. The Angular 22 app supplies project creation/opening, Scene/Game views, live Inspector editing, scene/model actions, native viewport controls, Save All and staged standalone export. Native spatial gestures and rendering stay in Rust/Vulkan; the Settings portal remains a placeholder. Automated checks and desktop/GPU acceptance are distinct.

Read [Angular editor workspace](../../editor-workspace.md) for the desktop workflow, exact connected controls, project replacement guards, component generators and remaining UI gaps. [Editor flow](../../editor-flow.md) traces the underlying methods.
The [native scene editor implementation](../../viewport-implementation.md) documents the new viewport gestures, grid/handle rendering, pointer-captured placement and current limits.

## Run

From the repository root after [setup](../../usage.md):

```powershell
npm.cmd ci --prefix apps/editor
.\dev.ps1 editor
```

For browser-only frontend work, run `npm.cmd start` from apps/editor. Native workspace/viewport operations require
Tauri; browser mode reports unavailable rather than supplying fake document data. GPU resources are now created on
demand in the host, not unconditionally before the window opens.

## Frontend modules

| File under src/app/core/editor | Role                                                                                                           |
|--------------------------------|----------------------------------------------------------------------------------------------------------------|
| `editor.models.ts`             | Typed scene3 documents, commands, snapshot/capability/error and viewport metrics/input shapes                  |
| `editor.service.ts`            | Read-only signals, native command queues, revision-aware edits, imports/prefabs/collisions, viewport lifecycle |
| `editor-updates.ts`            | Validate incremental command replies and merge authoritative deltas before publishing signals                  |
| `engine-viewport.directive.ts` | Attach a native child to one existing Angular DOM pane; observe bounds/visibility and emit input/status/errors |
| `viewport-editor.models.ts` | Editor-only camera/tool actions and native presentation-state DTOs |
| `viewport-editor.service.ts` | Bounded view-action queue, native state subscriptions and coalesced document notifications |

EditorService exposes document, selection, dirty, playing, pending/busy, capabilities, assets, collisions and viewport
signals. Open a project, load capabilities, execute typed intent, and accept the returned authoritative snapshot.
Commands use expected_revision; errors are reported and recover the in-memory snapshot where possible, with no silent
replay or disk reload. The document queue is bounded at 256 pending operations; continuous input should be coalesced by
its owner.

When capabilities advertise `incremental_commands`, `execute()` uses `editor_execute_incremental`. The host returns a
receipt and authoritative state with changed entities, optional camera/lights, or a complete snapshot for structural
changes. The client validates receipt/state consistency and entity coverage before accepting the whole response.
Unchanged document parts retain their references. Missing capability support uses the older `editor_execute`
full-snapshot route; failed incremental edits are recovered by querying state, never by replaying the command.

The viewport lifecycle uses a separate serialized queue, capped at 32 regular operations. Repeated pending bounds,
status and detach requests are coalesced; an attach creates a lifecycle boundary for subsequent bounds. A coalesced
detach can exceed the regular cap so teardown can finish. Listener cleanup disables local callbacks even if the native
window has already closed.

## Layout preferences

`LayoutPreferencesService` retains schema-1 component configs under stable keys. Load and initialization return cloned
values so a component cannot mutate retained preferences through a decoded object. `saveConfig(key, value, baseline?)`
returns the serialized comparison baseline; unchanged configs avoid another write. EditorComponentService reuses this
baseline when connecting a component. Changed configs are cloned through JSON, debounced for 300 ms and flushed in
order. Layout settings remain separate from authored project/scene state.

EditorService does not assign shortcuts. Native ViewportInteraction owns focused spatial shortcuts; Angular controls send editor-only viewport actions. Authored commands keep their existing typed execute path. Do not mutate signal snapshots or duplicate runtime scene semantics in components.

## Project and scene operations

The service requires editor protocol 2 / scene schema 3; ProjectData mirrors project schema 3, including reusable
behavior definitions and scene/entity attachments. Incremental snapshot validation accepts that schema; schema-2
project files are upgraded by the native JSON loader before reaching the frontend.
`openProject(manifestPath)`, `registerScene(path)`, `switchScene(id)`, `saveSceneAs(path)` and `saveProject()` wrap the
host APIs. Snapshots expose active scene identity, project state and per-scene dirty flags. `save()` persists only the
active scene; `saveProject()` persists only the manifest. `reload()` explicitly sends `discard_unsaved`, while
`refresh()` reads in-memory state. `saveAll()` persists all dirty retained scenes and project settings without changing the active scene. The UI must make destructive reload intent clear.
See [project lifecycle](../../project-lifecycle.md) for the data model. The workspace now exposes scene switching, startup selection, registration and Save As, plus selectable model/scene catalogue entries and origin/native-drag model placement.

`core/project/project.service.ts` creates authoring projects and chooses a destination. Setup opens the created manifest through EditorService, retaining a retry path after open cancellation/failure. `prepareBuild()` stages saved inputs inside the edit reservation; `finishBuild()` compiles outside the document queue. `runBuild(expectedPath)` explicitly launches the remembered successful output.

## Pane boundary and platform limits

`SceneViewport` in modules/editor/workspace imports `[engineViewport]` and is mounted by the main workspace when a project is open. It displays status/errors, registers the CSS aperture for asset dragging and suppresses the surface for WebView modal dialogs. Native ViewportInteraction handles viewport input; the wrapper still exposes raw input/status/error outputs for observation. The directive owns one viewport reservation, bounds/visibility and teardown; Scene/Game tabs share this surface rather than creating two renderers.

The adjacent ViewportToolbar stays outside the child bounds. Native grid, selection bounds, handles, marquee and placement markers are rendered in Vulkan. `ViewportAssetDragService` captures a model-entry pointer, forwards normalized CSS-aperture coordinates and flushes the final position before CommitPlacement. Native asset adoption is asynchronous and remains cancellable through its loading state.

Hierarchy uses virtual parent/child rows, keyboard navigation and keep-world reparent actions with an Inspector commit barrier. ViewportEditorService binds native generations to the frontend's separate local contexts, clears stale gesture/preview state and rejects queued actions after replacement. WebView blur cancels an unfinished model drag; focus transfer after submission retains the load, with Escape cancellation available.

Embedding currently uses a Windows native child window. It has native airspace: ordinary HTML is not an arbitrary
overlay above the Vulkan child. Non-Windows capability limits must be surfaced. Real cross-window capture, DPI, focus, shutdown and GPU behavior require their separately recorded desktop acceptance.

## Contracts and learning report

[Steps 21–35](../../editor-steps-21-35.md) explain the complete runtime/host/frontend split, camera commands, snapping,
multi-selection, hierarchy, scene3, prefabs, static scene import, tints/lights, simulation/collision, GPU timing, and
instancing. These are APIs for the Angular editor plus game pane, not a second native authoring product. The source game
defaults to Development mode with the existing native learning harness; installed projects default to Player. The native
harness delegates to the shared core session and is not the future Angular editor.

## Build and checks

From apps/editor:

```powershell
npm.cmd run build
npm.cmd test -- --watch=false
```

Production output remains `dist/editor/browser`. Consult [testing](../../testing.md) for exact evidence; successful
TypeScript/build checks do not establish a finished UI layout or visible embedded-viewport acceptance.
See [host](../editor-host/README.md) and [bridge](../../crates/bridge/README.md).
