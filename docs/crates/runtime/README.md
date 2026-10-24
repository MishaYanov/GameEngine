# Shared runtime and editor logic

Source: `crates/runtime`. CPU documents, commands and simulation live in engine-core and are re-exported here. Runtime
supplies shared asset/model/GPU-scene adapters and GraphicsProbe; Angular owns presentation and bindings.

## EditorSession: the Angular domain boundary

`runtime::editor::EditorSession` owns schema3 authored/saved data, persistent-key multi-selection, snapping, a temporary
Play copy, revisions, transactions, and bounded patch history. `execute(EditorCommandRequest)` validates
expected_revision and returns an authoritative bridge snapshot. `CommandFactory::capabilities()` advertises
intent/categories/undoability/Play availability without assigning physical keys.

Operations cover camera frame/orbit/pan/dolly, local/world/group transforms, hierarchy parenting, identity-preserving
edits, prefab export/instantiate, tint/light/collider changes, undo/redo and Play input/pause. `tick` uses a bounded
fixed step and updates scene_revision without turning every rendered frame into a command revision conflict. Saving uses
authored content and rejects an uncommitted transaction.

The CPU history is capped at 128 records / 16 MiB, selection at 4096. Atomic validation/error behavior matters as much
as nominal command success. This is separate from the older GPU SceneEditor history; do not reuse one session's
transient IDs in another.

## Project documents

Project schema 2 adds SceneCatalog/SceneId, startup_scene_id and scene-scoped behavior entries. Core EditorSession
retains active/inactive drafts, per-scene saved baselines and scene-aware history; EngineBuilder::with_scenes prepares
all documents. Editor protocol 2 snapshots expose the catalog and dirty states.
See [project lifecycle](../../project-lifecycle.md) for caps, commands, version rejection, host paths and save/reload
semantics.

## Scene and asset modules

| Module                          | Current role                                                                            |
|---------------------------------|-----------------------------------------------------------------------------------------|
| scene_data / scene_io           | Schema3 parse/migration/validation, hierarchy/world matrices, bounded CPU/GPU load/save |
| editor/prefab                   | Isolated version-1 prefabs, subtree export, remapped instance keys/parents              |
| assets/scene_import             | Selected static glTF scene traversal and node/mesh metadata                             |
| assets/asset_manager, model     | CPU metadata plus cached selected-mesh GPU resources                                    |
| camera / editor camera commands | View state and validated frame/orbit/pan/dolly intent                                   |
| simulation / collision          | Default 60 Hz bounded steps; conservative overlap/swept movement foundation             |
| bounds / picking / visibility   | World-aware bounds, nearest-box picking, conservative frustum selection                 |

Schema3 can store transform-only nodes, parents, mesh selection, material tint, collider data and up to 16 scene lights.
Full static scene import is not skinning, morph playback or animation. Collision is not full physics. Model resources
still contain Vulkan types, so the asset/GPU-scene side is not backend-independent.

## Legacy and compatibility APIs

Transform and CPU editor/scene types are re-exported from engine-core. Scene/EntityId, ModelInstance, AssetManager and
SceneEditor adapt those documents to Vulkan resources. Native SceneEditor delegates authored edits/history to the core
session; legacy CommandHistory/RobotGame helpers remain compatibility leftovers rather than the normal GameApp pipeline.

`runtime::Engine` now refers to the CPU coordinator. Graphics availability uses `GraphicsProbe::probe_status()`; it
queries physical-device information without retaining a logical device. Do not confuse that status with the embedded
viewport's GPU ownership.

Procedural MeshData constructors (triangle/cube/plane/polygon/sphere/capsule) remain available with generated normals.
See [graphics and assets](../../graphics-and-assets.md). No primitive-specific persistence UI or collision shape is
implied by constructor availability.

## Learning and checks

[Steps 21–35](../../editor-steps-21-35.md) explains the exact source map, examples and
resources; [editor commands](../../scene-editing.md) describes the frontend boundary. Run runtime tests offline/locked
and use [testing](../../testing.md) for final evidence. Interactive Angular layout/child-pane acceptance, full
material/physics/import feature support and measured performance remain explicit limits.
