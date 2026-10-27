# Shared engine contracts

Source: `crates/bridge`. The crate defines serializable engine/editor data; it owns no UI layout, native input bindings,
scene mutation, filesystem access, or GPU lifecycle.

## Editor protocol

`bridge::editor` adds typed command requests, snapshots, capabilities, transforms, snapping, prefab/collision and
viewport status/input DTOs. Current capability negotiation is protocol version 2 with scene schema 3; project manifests
use schema 2. Core command/snapshot DTOs are re-exported from engine-core, while viewport DTOs remain bridge-owned. The
Angular
mirrors live under apps/editor/src/app/core/editor/editor.models.ts; update both sides together.

EditorCommandRequest carries expected_revision plus a tagged snake_case EditorCommandDto. A snapshot includes
revision/scene_revision, document, persistent-key selection, mode/paused/dirty, undo/redo availability, transaction
label and snapping settings. Expanded snapshots also carry active_scene_id, project, project_dirty and per-scene
loaded/dirty states. Typed commands add catalog creation/removal/path changes, scene switching and startup selection.
See [project lifecycle](../../project-lifecycle.md) for incompatible project-v1 rejection and the save/reload contract.
Command descriptors report categories, undoability and Play availability so UI owners need
not hardcode backend assumptions.

The optional `incremental_commands` capability defaults to false when absent. Supporting hosts expose
`EditorCommandUpdateDto`, tagged by `kind`: `snapshot` carries a receipt and full snapshot; `delta` carries the receipt,
`EditorStateDto`, changed entity records and optional camera/lights. State excludes document/project copies. Structural,
binding, entity-order and project changes use the full snapshot fallback. The response is captured atomically with
command execution; it is not a later journal query. Angular validates and merges it before publishing accepted state.

Viewport status includes draw/instance/triangle counts and optional asynchronous GPU time with a support flag.
Unsupported/unavailable timing is not zero GPU cost. Native input/status/error events support the Angular game-pane
owner. No shortcuts or mock document state are defined by this transport.

## Engine status compatibility

EngineStatusDto remains camelCase JSON: initialized, renderer, gpuName, vulkanApi. Its naming differs from the
snake_case editor DTO contract; do not blindly rename one to match the other. The status wrapper is separate from
presentation/document editing.

## Checks

```powershell
cargo test -p bridge --locked --offline
```

Serialization and contract tests help keep Rust/TypeScript in agreement; integration and visual acceptance remain
separate. See [Angular editor](../../apps/editor/README.md), [runtime](../runtime/README.md),
and [steps 21–35](../../editor-steps-21-35.md).
