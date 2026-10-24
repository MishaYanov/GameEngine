# Angular editor integration

Source: `apps/editor`. The Angular 22 app owns editor UI, input bindings and the chosen game pane. Runtime supplies
editor behavior through a typed Tauri bridge. **This batch adds integration APIs, not a new UI layout, mock screen, or
composed editor template.** The current setup/status screen has not been replaced with a finished scene editor.

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
| `engine-viewport.directive.ts` | Attach a native child to one existing Angular DOM pane; observe bounds/visibility and emit input/status/errors |

EditorService exposes document, selection, dirty, playing, pending/busy, capabilities, assets, collisions and viewport
signals. Open a project, load capabilities, execute typed intent, and accept the returned authoritative snapshot.
Commands use expected_revision; errors are reported and recover the in-memory snapshot where possible, with no silent
replay or disk reload. The document queue is bounded at 256 pending operations; continuous input should be coalesced by
its owner.

The core service does not assign shortcuts. Future UI controls and key bindings use the same execute path. Do not mutate
signal snapshots or duplicate runtime scene semantics in components.

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
instancing. These are APIs for the Angular editor plus game pane, not a second native authoring product. The earlier
native game UI remains an explicitly opt-in legacy demo.

## Build and checks

From apps/editor:

```powershell
npm.cmd run build
npm.cmd test -- --watch=false
```

Production output remains `dist/editor/browser`. Consult [testing](../../testing.md) for exact evidence; successful
TypeScript/build checks do not establish a finished UI layout or visible embedded-viewport acceptance.
See [host](../editor-host/README.md) and [bridge](../../crates/bridge/README.md).
