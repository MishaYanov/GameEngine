# Editor commands and document ownership

The desktop Editor combines Angular authoring controls with a native scene and game viewport. Hierarchy,
Inspector, Asset and specialized pane actions submit typed commands to the current workspace.
[Objects, components and Props](authoring-objects.md) walks through the editing workflow.

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
workspace's SceneViewport applies that directive to one persistent Scene/Game pane. It attaches an on-demand native
viewport, observes layout/visibility and emits status/raw input/errors.

The Windows child viewport has native airspace and lifecycle/DPI/focus constraints; ordinary HTML is not a free overlay
atop it. Angular keeps Inspector and toolbar controls beside the child window. Native selection, marquee, transform
gestures and snapping share Rust command/history semantics with numeric edits. The Component Inspector edits typed
mesh, collider and native behavior records. Assets exposes copied placement, linked source creation/editing and
explicit Link/Update workflows. Open a nested source in Prop Workspace to edit it.

## Colliders and interaction settings

The selected entity supports None, Box and Sphere, local offset, positive local half extents/radius, Blocking/Sensor
mode, membership layers and detection masks. Numeric drafts keep incomplete text local, coalesce valid previews,
commit on Enter/blur and cancel on Escape or an invalid final value. Type, mode and filter changes are atomic edits;
queued filter intents rebase the latest collider so they preserve dimensions and other bits. Selection/scene/project
changes invalidate drafts, and Play disables authoring.

`ColliderData` and `ColliderInteraction` live in `engine-core/src/collider.rs` and remain re-exported by `scene_data`.
`set_collider` validates geometry and uses ordinary history/delta/snapshot handling. Absent interaction defaults to
`sensor: false`, `layers: 1`, `mask: 4294967295`; partial objects default individual fields and fully default settings
are omitted on serialization. Filters accept all u32 values, including zero and bit 31. Both participants' masks must
accept the other's membership. Sensors produce overlaps but never block kinematic motion on either side.

The optional Colliders Scene overlay shows selected effective shapes, using dashed pink sensor outlines and continuous
cyan blocking outlines. It is editor-only, cached and bounded; errors and omitted outlines are reported without
failing scene rendering. Boxes use transformed world AABBs, and spheres conservatively enclose hierarchy scale/shear.
This is not an oriented-box/rigid-body solver. Sensor contacts feed fixed-step Enter/Stay/Exit trigger delivery and
native component behavior. Sensor enter and exit edges use the native trigger lifecycle.

## Multi-scene documents and native adapter

Project schema 3 retains independent authored/saved scene drafts and transient per-scene selections. Switching requires
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
to Player. Angular and Tauri own workspace presentation and device routing.

## Check an editor change

Use [Testing and validation](testing.md) for command, workspace and native viewport checks. A revision or history
regression belongs in the command tests. Validate window focus, gestures and visible frames in a desktop run.
