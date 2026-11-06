# Angular editor workspace: create, edit and preview

The main `/editor` route connects project creation/opening, entity and Inspector editing, native Vulkan tools, scene/model actions, saving, Play and staged standalone export. The [native scene editor implementation](viewport-implementation.md) gives the complete control reference, ownership model, source map and verification boundaries. The [historical reports](project3-handoff.md) describe older milestones.

## Open a desktop project

1. Start the Tauri editor using `python launcher.py --run client` or `dev.ps1 editor` from the repository root.
2. Open the `/editor` route and choose **File → Open Project…**. **New Project…** opens setup, creates authoring files without compiling a player, and opens the created manifest; cancellation/failure retains its path and an Open in Editor retry.
3. Select `project.json` in the native file chooser. Cancel leaves the installed workspace unchanged.
4. If the currently reviewed snapshot is dirty, choose **Discard and open** or **Keep current project** in the native confirmation dialog.
5. After successful candidate installation, the header shows project name, active scene ID and overall unsaved state. Scene is the default view; its native viewport remains available for inspection during Play.

Opening requires the desktop bridge and embedded-viewport capability. The component disables it while opening, busy, playing or inside a transaction. Browser mode displays an unavailable state rather than inventing project data. The chooser filters JSON, but `ProjectLocation` still validates the required manifest filename, root and contained assets; choosing any JSON file does not make it a project.

`Editor.openProject()` loads capabilities, chooses a file, captures the reviewed snapshot and obtains explicit discard intent. `EditorService.openProject(manifest, options)` captures the expected revision **before** queued operations finish. It sends `expectedRevision` and `discardUnsaved` with the manifest path. Under the installation mutex, host `ProjectReplacement::validate` checks that revision against the current workspace, rejects Play/open transactions and requires discard permission for dirty state. Edits made during dialogs or candidate preparation cannot be discarded under an older reviewed revision. On conflict the component refreshes in-memory state; it does not silently retry opening.

The expected revision is null when no workspace is open. A direct caller replacing an existing project must supply or capture its current revision. `editor_open` is an older compatibility API and does not use this new guarded project-replacement path; it should not be treated as the UI's safe replacement contract.

## Connected controls and current limits

| Surface | Current behavior |
| --- | --- |
| File menu | Guarded New Project/setup/open-created flow and guarded native Open Project flow |
| Save Scene | Calls `EditorService.save()` for the active authored scene; requires Edit, no transaction and overall dirty state. It does not save the manifest or other drafts |
| Save All | Saves dirty retained scenes and project settings without switching scenes; partial failures preserve accurate remaining dirty baselines |
| Undo / Redo | Sends typed commands; enabled from snapshot history availability and Edit/queue state |
| Play / Stop | Sends typed commands through the existing queue. Successful Play chooses the Game tab; Stop returns to Scene |
| Scene / Game tabs | Select the native view through editor-only actions, with arrow/Home/End keyboard navigation. They do not themselves start/stop Play or switch catalog scenes |
| Hierarchy | Virtual parent/child tree, expand/collapse, stable-key selection, drag reparenting and Set Parent/Move to Root alternatives |
| Inspector | Edits selected name and local position/rotation/scale with live previews, grouped history, Enter/blur commit and Escape cancellation |
| Viewport tools | Editor-only camera, grid/planes, projection/orientation, native picking and Move/Rotate/Scale handles |
| Assets | Catalogued scenes can be opened; catalogued models can be added at origin or pointer-captured into a native placement preview |
| Scene controls | Switch retained scenes, choose startup, register existing scene files and save the active scene at a new path |
| Export Game | Stage saved inputs, compile outside the document queue, retry a staged failure and explicitly run the remembered output |
| Settings portal | Placeholder component; not a completed settings editor |
| Reload | Service API exists; the workspace does not expose a destructive Reload action |

The overall dirty flag can include project settings or inactive drafts. Therefore an enabled Save Scene button does not promise that the whole workspace will become clean. Use Save All before Export Game. [Project lifecycle](project-lifecycle.md) defines the separate saved baselines.

## One native surface, two views

`EditorWorkspace` owns `WorkspaceViewKind` (`scene-viewport` or `game-preview`) and mounts one persistent `SceneViewport` after a snapshot exists. Scene remains visible during Play, using the editor camera with read-only authoring tools. Game uses the runtime camera and hides editor decoration. Game while stopped shows explanatory empty content. Switching these tabs creates neither a second renderer nor a second simulation.

`SceneViewport` imports `EngineViewportDirective`, displays status/error feedback and exposes input/status/error outputs. It hides the native child for WebView modal dialogs using a mutation observer, and disconnects that observer on destruction. `refreshBounds()` is available after dock/drag movement that changes position without resizing. Directive/host queues retain bounds, visibility and teardown ownership; the wrapper does not own GPU uploads.

Windows child-window airspace constraints remain. The native project chooser/confirmation are host dialogs; the modal observer concerns WebView `dialog:modal` elements. `ViewportInteraction` now handles native spatial input directly. The adjacent Angular toolstrip sends view/tool actions through `ViewportEditorService`; camera notifications do not refresh authored documents. Internal asset dragging captures its WebView pointer and forwards normalized aperture positions instead of relying on HTML drag events over the child HWND.

The editor camera and grid/tool settings remain outside authored scene data and history. Native transform/placement/loading states lock competing Inspector authoring; ordinary camera navigation does not. Numeric Inspector edits and native handle gestures both use the existing typed scene commands and history. Picking/placement currently approximate model bounds, and rotation/scaling that cannot be represented by the scene's TRS format can be rejected.

Hierarchy supports arrow/Home/End navigation, typeahead and keyboard parenting. Reparenting commits the current Inspector draft, requests keep-world behavior and waits for Rust's accepted state. Virtual rows bound DOM size; tree construction and core transform work still depend on scene size. Visibility/layer controls, manual ordering and prefab authoring remain separate work.

View actions are bound to their original frontend/native workspace contexts. Replacement clears stale gestures and Inspector previews; native generations and frontend counters are mapped as separate namespaces. Unfinished asset drags cancel on WebView blur, while a submitted load survives expected focus transfer to the native child and remains cancellable with Escape.

The old shared `EnginePortal`, EditorGridWrapper, EditorNavManager and separate left/right/bottom wrapper components have been removed/replaced in the current working tree. Use `SceneViewport`, `EditorWorkspace` and the generic persisted Drawer component for current integration. Existing design previews are separate from product wiring.

## Component scaffolding and design previews

From `apps/editor`, the new local schematics provide:

```powershell
npm.cmd run generate:ui -- asset-badge
npm.cmd run generate:panel -- scene-tools-panel
```

`ui-component` generates external TS/HTML/SCSS under `src/app/shared/components/<name>` with an `app-` selector. `editor-panel` generates under `src/app/modules/editor/workspace/<name>` with an `editor-` selector, an `EditorPane` import and a title input. Names must be lowercase kebab-case; reserved device names and case-insensitive destination collisions are rejected. These are fixed destinations, not arbitrary path options. The generated panel projects content; it does not add domain behavior, tests, a route or a workspace placement automatically.

`design-registry.mjs` now combines shared-component folders with named `.design.ts` previews, including preview-only entries. Workspace components can appear through those explicit previews; they are not automatically discovered by scanning the workspace tree. The current native-surface preview is `scene-viewport.design.ts`. The design registry and schematic tests are available via `npm.cmd run test:design-registry` and `npm.cmd run test:schematics`. Their existence is not a passed test result from this documentation task.

## Source map and verification

| Source | Responsibility |
| --- | --- |
| `apps/editor/src/app/modules/editor/editor.{ts,html}` | Open workflow, progress/error state and project-replacement intent |
| `modules/editor/components/editor-file-menu`, `editor-header` | File actions and project/scene/dirty header |
| `modules/editor/workspace/` | Workspace layout, toolbar, tabs, entity/assets/inspector panels and SceneViewport |
| `core/editor/editor.service.ts`, `core/editor/workspace/workspace-view.ts` | Shared queue/state, chooser/confirmation calls, expected revision and view-kind contracts |
| `core/editor/viewport-editor.{models,service}.ts` | View-only contracts/action queue, native state listeners and document-event coalescing |
| `modules/editor/workspace/viewport-toolbar`, `viewport-asset-drag.service.ts` | Adjacent controls and internal asset pointer-capture broker |
| `apps/editor-host/src-tauri/src/commands/{project,editor}.rs` | Trusted native dialogs and guarded candidate installation |
| `apps/editor-host/src-tauri/src/viewport{,_interaction,_tools}.rs` | Native window/render ownership, tool state and spatial math |
| `apps/editor/projects/editor-schematics/` | Local generator collection, name schema and external-file templates |

Frontend paths after the first row are relative to `apps/editor/src/app`. This guide comes from commit/history plus staged, unstaged and untracked source inspection on 2026-11-06. The last commit includes already documented shared behavior definitions, prepared transitions, typed progress, resource ownership and optimization work; it does not imply that the new Angular workspace is committed.

Documentation maintenance rebuilds and checks the site. The feature's implementation checks and current evidence belong in [native scene editor implementation](viewport-implementation.md) and the completion report; a site build does not establish GPU or cross-HWND acceptance. See `docs-site/docs-state.json` for the inspection baseline and verification limits. The Game App Developer space remains empty.
