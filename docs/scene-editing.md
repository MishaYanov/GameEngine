# Native scene editing

Steps 11–20 implement shared SceneEditor APIs and a native editing interface in the game. Automated verification is
available; visible acceptance and GPU-performance appearance checks remain unverified. This is separate from the
Angular/Tauri editor, which still provides its setup/status shell. See
the [step-by-step change report](editor-steps-11-20.md) for rationale, files, examples, and resources.

## SceneEditor and Edit/Play

`runtime::SceneEditor` owns generic EntitySelection and CommandHistory. It exposes selection, picking, keyboard
transform edits, rename, spawn, duplicate, deletion, and drag commits. RobotGame no longer owns editing: it runs
optional robot simulation during Play.

Development starts in Edit. F6 runs a temporary cloned Scene/camera with shared Arc<Model> resources; Stop restores
authored state. Generic scenes need no robot bindings to open or enter Play. F5 during Play saves authored data. Failed
reload preserves active state; successful reload installs a fresh editor and clears obsolete history. Installed mode
does not expose these development tools.

## Selection, bounds, and picking

EntitySelection validates live IDs and cycles all scene entities. Persistent EntityKey values belong to saved identity;
runtime EntityIds remain scene lookup values. Picking uses model-local bounds, inverse transforms, a Vulkan near/far
screen ray, and nearest slab intersection. Exact ties keep scene order; empty clicks clear selection; singular or
unbounded models are skipped. Coordinates are physical pixels remapped to the actual surface extent.

A selected bounded model displays one cached unit-box overlay transformed by its bounds/instance. Selection is
box-based, not triangle-accurate. The native hierarchy panel is a flat entity list, not a new parent-child scene graph.

## Mixed undo/redo

`SetTransform.apply` validates finite transforms. CommandHistory now records Transform, Rename, Spawn, Duplicate (as a
spawn snapshot), and Delete, with 256 undo entries. EntitySnapshot restoration preserves IDs, keys, order, role
bindings, and model sharing. Failed restoration leaves its history record in place; no-ops/errors do not discard redo.
Fresh changed edits clear redo. Use `undo_scene`/`redo_scene` for mixed history; transform-only wrappers reject
structural records.

Structural edits no longer clear the entire history. Generic Edit deletion does not protect demo roles merely for
simulation; removing an entity clears targeting bindings, and undo restores them. Camera motion, save/reload, and Play
simulation are not authored history commands. History is not serialized and cannot be carried into a replacement scene.

## Native controls

| Input                | Behavior                                                                 |
|----------------------|--------------------------------------------------------------------------|
| F1                   | Toggle native panels, subject to compact-window layout limits            |
| F5 / F9              | Save authored content / reload                                           |
| F6 / F7              | Toggle Edit/Play / toggle frustum culling                                |
| Left click           | Panel action, gizmo handle, or object box selection in Edit              |
| 1 / 2 / 3            | Translate / rotate / scale gizmo                                         |
| Tab                  | Cycle current Edit entities                                              |
| WASD                 | Move Edit selection in world XY; move robot_a in robot Play              |
| Q/E                  | Z rotation steps of -/+5 degrees in Edit                                 |
| R/F                  | Uniform scale ×1.1 / ÷1.1 in Edit                                        |
| N / C / Delete       | Robot shortcut spawn / duplicate / delete in Edit                        |
| Ctrl+Z / Ctrl+Y      | Mixed undo / redo                                                        |
| Space                | Toggle robot simulation pause during Play (rotation and WASD)            |
| Right mouse / Escape | Capture for camera look / release; Escape cancels active drag/text first |
| Enter                | Apply active property text field                                         |

Panel fields edit names and numeric transforms through history; rotation is shown in degrees and stored in radians.
Typing blocks movement. Allowed panel commands also reset timing, suppress held input, and end gestures before
execution, preventing file/model-loading time from becoming the next movement delta even on failure. The dirty marker
compares authored scene and camera with saved content, including identity/bindings; undo exactly to saved content clears
it. It does not compare simulated Play state.

## Mouse gizmos and assets

Translate uses world-axis projected screen mapping, rotate uses world-axis quaternion/ray-plane math, and scale follows
local axes. A drag previews from the original transform; release commits one undo entry. Escape, focus loss, resize, and
mode changes cancel back to the original state. Invalid/non-finite and camera-parallel handle cases are rejected.

The asset panel scans supported GLB/glTF paths under the root, skips links, enforces limits, and sorts paths. Refresh
rescans; Place creates an undoable entity three units along camera forward. Cached templates share models across
placements. There are no thumbnails, search, general glTF scene-node import, or material/light inspectors.

Panels use a bitmap ASCII font; Unicode names remain data, with fallback glyphs for unsupported display characters.
Panels hide below 800×400 logical pixels and short windows can truncate lower fields. No screenshot or native
interaction review was done for this batch.
See [verification evidence](editor-steps-11-20.md#verification-evidence-and-limits).
