# Angular editor integration

Source: `apps/editor`. The Angular 22 app owns editor UI, input bindings and the chosen game pane. Runtime supplies
editor behavior through a typed Tauri bridge. **This batch adds integration APIs, not a new UI layout, mock screen, or
composed editor template.** The current setup/status screen has not been replaced with a finished scene editor.

See [Editor flow: startup to project builds](../../editor-flow.md) for the current startup routes, setup action,
document queue, and viewport graphs, including which APIs are not yet connected to the main editor UI.

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

The core service does not assign shortcuts. Future UI controls and key bindings use the same execute path. Do not mutate
signal snapshots or duplicate runtime scene semantics in components.

## Project and scene operations

The service requires editor protocol 2 / scene schema 3; ProjectData mirrors project schema 2.
`openProject(manifestPath)`, `registerScene(path)`, `switchScene(id)`, `saveSceneAs(path)` and `saveProject()` wrap the
host APIs. Snapshots expose active scene identity, project state and per-scene dirty flags. `save()` persists only the
active scene; `saveProject()` persists only the manifest. `reload()` explicitly sends `discard_unsaved`, while
`refresh()` reads in-memory state. The UI must make destructive reload intent clear.
See [project lifecycle](../../project-lifecycle.md) for a complete example and limits. No new scene-browser template is
implied by these service changes.

## Pane boundary and platform limits

EngineViewportDirective is available as `[engineViewport]` with input/status/error outputs. It has not been applied to a
newly composed app pane here. The eventual UI owner chooses layout, imports the directive, binds controls, handles
errors, and maps raw input into commands. The directive handles one viewport reservation, resize/intersection
visibility, bounds updates and teardown.

Embedding currently uses a Windows native child window. It has native airspace: ordinary HTML is not an arbitrary
overlay above the Vulkan child. Non-Windows capability limits must be surfaced. No interactive acceptance of pane
bounds, clipping, DPI, focus, or shutdown has been performed for this batch.

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
