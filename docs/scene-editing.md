# Editor commands and document ownership

The current editor direction is **Angular controls plus a game pane**, with runtime logic behind a typed bridge. Steps
21–35 implement the integration contracts and engine foundations; no editor layout, mock screen, or composed template
was added. Read the [learning report](editor-steps-21-35.md) for each step's rationale, file map, example and resources.

## Authoritative document API

`engine_core::editor::EditorSession` (re-exported by runtime) owns validated SceneData, saved authored state, a
temporary Play copy, persistent-key
selection, snapping, transactions and bounded patch history. `EditorCommandRequest` carries expected_revision. A
rejected/stale command is not silently retried. Snapshot revision is command/document state; scene_revision
independently invalidates rendered Play content. Workspace generation distinguishes project replacement.

`bridge::editor` defines input-independent commands, capabilities, snapshots and viewport DTOs. Angular's EditorService
queues requests, publishes authoritative read-only signals, and reports/reconciles errors. Widgets and user-configured
bindings choose command intent; runtime does not assign physical keys. No UI should mutate snapshot arrays in place or
reimplement domain transforms.

## Selection, snapping, hierarchy, and history

Selection is a validated list of up to 4096 distinct persistent keys. Commands support local/world transforms,
frame/orbit/pan/dolly camera, translation/rotation/scale snapping, reparenting and grouped edits. World hierarchy is
derived from local TRS and parent keys; missing/cyclic parents or unsupported shear/singular keep-world conversions
fail.

Patch history stores changed entity/camera/light/binding/selection data, bounded at 128 records and 16 MiB.
Begin/commit/cancel transactions group a gesture. Saving/reloading while a transaction is open requires an explicit
commit/cancel first. Authored save state is separate from Play simulation and history is not serialized.

Prefab export/instantiation remaps internal identities and parent references. Importing a static glTF scene creates
entities/transform nodes for its selected scene and reuses mesh resources. Per-entity tint, lights and colliders are
document data and use the same validated command path.

## Frontend and viewport integration

The files under `apps/editor/src/app/core/editor` provide typed models, EditorService, and EngineViewportDirective. The
directive attaches one enabled Angular DOM pane to an on-demand native viewport, observes layout/visibility and emits
status/raw input/errors. It has not been applied by newly composed app layout in this batch.

The Windows child viewport has native airspace and lifecycle/DPI/focus constraints; ordinary HTML is not a free overlay
atop it. The UI owner must surface platform capability limits and input/error states. Raw mouse/key notifications are
not document edits until mapped by the frontend owner. No finished inspector, multi-selection gesture UI, snapping
toolbar, or prefab browser is claimed.

## Multi-scene documents and native adapter

Project schema 2 retains independent authored/saved scene drafts and transient per-scene selections. Switching requires
Edit/no transaction, keeps unsaved drafts and the outgoing selection, and restores the incoming scene's selection after
validating it. Cross-scene undo/redo can activate the scene owning an earlier edit and restores that history record's
selection; it validates the restored selection before changing history or activation. Structural catalog changes share
history. Navigation adds no undo entry. Freshly created/loaded drafts and reopened workspaces start with empty
selection;
selection is not saved in scene/project files and does not affect dirty flags or saved baselines. Snapshots expose
active_scene_id, project_dirty and per-scene dirty state.
Read [project lifecycle](project-lifecycle.md) for the exact commands and persistence limits.

The native SceneEditor now delegates to the core session rather than maintaining a competing active history pipeline.
Source game execution defaults to Development and preserves panels/gizmos/learning controls; installed projects default
to Player. Legacy CommandHistory/RobotGame APIs remain compatibility leftovers. Angular/Tauri owns future product
presentation and bindings.

## Verification boundaries

Final implementation/review/test results for steps 21–35 are recorded in [testing](testing.md) and
docs-site/docs-state.json. Automated checks and hidden native examples are distinct from interactive Angular pane
acceptance. No visual completion, full physics, animated importer, PBR, GPU-utilization percentage, or measured
instancing speedup is inferred from the code alone.
